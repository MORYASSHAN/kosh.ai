//! Antivirus, firewall, updates, pending reboot, Secure Boot, TPM and risky settings.
//! Everything here is read-only.

use super::reg::{self, Hive};
use super::wmi_util::{
    connect, get_bool, get_date, get_str, get_u32, is_access_denied, query, query_ns,
};
use crate::{CollectResult, Ctx, Patch, Skip};
use chrono::{DateTime, Duration, TimeZone, Utc};
use kosh_core::updates::is_os_update;
use kosh_core::{AvProduct, DefenderStatus, FirewallProfile, PendingReboot, TpmInfo};

/// Decode Security Center `productState`. Verified on machine A:
/// bit 0x1000 = real-time protection on; bit 0x10 = signatures out of date.
pub fn decode_av_state(state: u32) -> (bool, bool) {
    let enabled = state & 0x1000 != 0;
    let up_to_date = state & 0x10 == 0;
    (enabled, up_to_date)
}

pub fn antivirus(_ctx: &Ctx) -> CollectResult {
    let products: Vec<AvProduct> = match query_ns(
        "root\\SecurityCenter2",
        "SELECT displayName, productState FROM AntiVirusProduct",
    ) {
        Ok(rows) => rows
            .iter()
            .map(|r| {
                let state = get_u32(r, "productState");
                let decoded = state.map(decode_av_state);
                AvProduct {
                    name: get_str(r, "displayName").unwrap_or_default(),
                    enabled: decoded.map(|d| d.0),
                    up_to_date: decoded.map(|d| d.1),
                    raw_state: state,
                }
            })
            .collect(),
        // Security Center exists only on client Windows.
        Err(e) => {
            tracing::debug!("SecurityCenter2: {e}");
            Vec::new()
        }
    };

    let defender = query_ns(
        "root\\Microsoft\\Windows\\Defender",
        "SELECT AntivirusEnabled, RealTimeProtectionEnabled, AntivirusSignatureLastUpdated, AntivirusSignatureAge FROM MSFT_MpComputerStatus",
    )
    .ok()
    .and_then(|r| r.into_iter().next())
    .map(|r| DefenderStatus {
        antivirus_enabled: get_bool(&r, "AntivirusEnabled"),
        realtime_enabled: get_bool(&r, "RealTimeProtectionEnabled"),
        signature_last_updated: get_date(&r, "AntivirusSignatureLastUpdated"),
        // 65535 means "never updated / unknown".
        signature_age_days: get_u32(&r, "AntivirusSignatureAge").filter(|a| *a < 65535),
    });

    if products.is_empty() && defender.is_none() {
        return Err(Skip::NotAvailable("antivirus status not readable".into()));
    }
    let patch: Patch = Box::new(move |s| {
        s.security.antivirus = products;
        s.security.defender = defender;
    });
    Ok(patch)
}

pub fn firewall(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\StandardCimv2",
        "SELECT Name, Enabled FROM MSFT_NetFirewallProfile",
    )?;
    let profiles: Vec<FirewallProfile> = rows
        .iter()
        .filter_map(|r| {
            Some(FirewallProfile {
                name: get_str(r, "Name")?,
                enabled: get_u32(r, "Enabled")
                    .map(|e| e == 1)
                    .or_else(|| get_bool(r, "Enabled"))?,
            })
        })
        .collect();
    if profiles.is_empty() {
        return Err(Skip::Failed("no firewall profiles returned".into()));
    }
    let patch: Patch = Box::new(move |s| s.security.firewall = profiles);
    Ok(patch)
}

pub fn pending_reboot(_ctx: &Ctx) -> CollectResult {
    let p = PendingReboot {
        component_servicing: reg::key_exists(
            Hive::Hklm64,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending",
        ),
        windows_update: reg::key_exists(
            Hive::Hklm64,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired",
        ),
        file_rename_operations: reg::open(
            Hive::Hklm64,
            r"SYSTEM\CurrentControlSet\Control\Session Manager",
        )
        .map(|k| {
            k.get_raw_value("PendingFileRenameOperations")
                .map(|v| v.bytes.len() > 4)
                .unwrap_or(false)
        })
        .unwrap_or(false),
    };
    let patch: Patch = Box::new(move |s| s.security.pending_reboot = p);
    Ok(patch)
}

pub fn secure_boot(_ctx: &Ctx) -> CollectResult {
    // Missing key = legacy BIOS or unknown: leave as None (precision over recall).
    let v = reg::hklm_dword(
        r"SYSTEM\CurrentControlSet\Control\SecureBoot\State",
        "UEFISecureBootEnabled",
    );
    let Some(v) = v else {
        return Err(Skip::NotAvailable(
            "Secure Boot state not reported (legacy BIOS?)".into(),
        ));
    };
    let patch: Patch = Box::new(move |s| s.security.secure_boot = Some(v == 1));
    Ok(patch)
}

pub fn tpm(ctx: &Ctx) -> CollectResult {
    if !ctx.elevated {
        return Err(Skip::NeedsAdmin);
    }
    let rows = match query_ns(
        "root\\CIMV2\\Security\\MicrosoftTpm",
        "SELECT IsEnabled_InitialValue, SpecVersion FROM Win32_Tpm",
    ) {
        Ok(r) => r,
        Err(e) if is_access_denied(&e) => return Err(Skip::NeedsAdmin),
        Err(e) => return Err(Skip::Failed(e)),
    };
    let info = match rows.first() {
        Some(r) => TpmInfo {
            present: true,
            enabled: get_bool(r, "IsEnabled_InitialValue"),
            version: get_str(r, "SpecVersion")
                .and_then(|v| v.split(',').next().map(|s| s.trim().to_string())),
        },
        None => TpmInfo {
            present: false,
            ..Default::default()
        },
    };
    let patch: Patch = Box::new(move |s| s.security.tpm = Some(info));
    Ok(patch)
}

pub fn risky_settings(_ctx: &Ctx) -> CollectResult {
    let uac = reg::hklm_dword(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System",
        "EnableLUA",
    )
    .map(|v| v != 0);
    let rdp = reg::hklm_dword(
        r"SYSTEM\CurrentControlSet\Control\Terminal Server",
        "fDenyTSConnections",
    )
    .map(|v| v == 0);

    let con = connect("root\\cimv2")?;
    // InstallState: 1 = enabled, 2 = disabled, 3 = absent.
    let smb1 = query(
        &con,
        "SELECT InstallState FROM Win32_OptionalFeature WHERE Name = 'SMB1Protocol'",
    )
    .ok()
    .and_then(|r| r.first().and_then(|r| get_u32(r, "InstallState")))
    .map(|s| s == 1);
    let guest = query(
        &con,
        "SELECT Disabled FROM Win32_UserAccount WHERE LocalAccount = TRUE AND SID LIKE '%-501'",
    )
    .ok()
    .and_then(|r| r.first().and_then(|r| get_bool(r, "Disabled")))
    .map(|disabled| !disabled);

    let patch: Patch = Box::new(move |s| {
        s.security.uac_enabled = uac;
        s.security.rdp_enabled = rdp;
        s.security.smb1_enabled = smb1;
        s.security.guest_enabled = guest;
    });
    Ok(patch)
}

pub fn updates(_ctx: &Ctx) -> CollectResult {
    let qfe = query_ns(
        "root\\cimv2",
        "SELECT HotFixID FROM Win32_QuickFixEngineering",
    )
    .unwrap_or_default();
    let hotfix_count = qfe.len() as u32;
    let last_success = wua_last_successful_os_update();
    let patch: Patch = Box::new(move |s| {
        s.security.updates.hotfix_count = hotfix_count;
        s.security.updates.last_successful_update = last_success;
    });
    Ok(patch)
}

/// Read the Windows Update history through the Windows Update Agent COM API.
fn wua_last_successful_os_update() -> Option<DateTime<Utc>> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::UpdateAgent::{
        orcSucceeded, orcSucceededWithErrors, uoInstallation, IUpdateSession, UpdateSession,
    };
    // SAFETY: COM is initialised for this collector thread; interfaces are released on drop.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let session: IUpdateSession =
            CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER).ok()?;
        let searcher = session.CreateUpdateSearcher().ok()?;
        let total = searcher.GetTotalHistoryCount().ok()?;
        if total <= 0 {
            return None;
        }
        let history = searcher.QueryHistory(0, total.min(300)).ok()?;
        let count = history.Count().ok()?;
        let mut best: Option<DateTime<Utc>> = None;
        for i in 0..count {
            let Ok(entry) = history.get_Item(i) else {
                continue;
            };
            let op = entry.Operation().ok();
            let result = entry.ResultCode().ok();
            if op != Some(uoInstallation)
                || !(result == Some(orcSucceeded) || result == Some(orcSucceededWithErrors))
            {
                continue;
            }
            let title = entry.Title().map(|b| b.to_string()).unwrap_or_default();
            if !is_os_update(&title) {
                continue;
            }
            if let Some(d) = entry.Date().ok().and_then(ole_date_to_utc) {
                best = Some(best.map_or(d, |b| b.max(d)));
            }
        }
        best
    }
}

/// OLE Automation date (days since 1899-12-30, UTC for WUA) to DateTime.
fn ole_date_to_utc(days: f64) -> Option<DateTime<Utc>> {
    if !days.is_finite() || days <= 0.0 {
        return None;
    }
    let base = Utc.with_ymd_and_hms(1899, 12, 30, 0, 0, 0).single()?;
    Some(base + Duration::milliseconds((days * 86_400_000.0) as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_verified_av_states() {
        // Values verified on machine A (VERIFIED_SOURCES.md).
        assert_eq!(decode_av_state(397568), (true, true)); // Windows Defender
        assert_eq!(decode_av_state(3424256), (false, true)); // McAfee, not active
        assert_eq!(decode_av_state(0x61110), (true, false));
    }

    #[test]
    fn ole_dates() {
        let d = ole_date_to_utc(45000.5).expect("date");
        assert_eq!(d.to_rfc3339(), "2023-03-15T12:00:00+00:00");
    }
}
