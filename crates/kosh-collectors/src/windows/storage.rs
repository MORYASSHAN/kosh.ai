//! Physical disks (type, health, wear) and volumes (free space, encryption).

use super::wmi_util::{connect, get_str, get_u32, get_u64, is_access_denied, query, Row};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::{DiskHealth, EncryptionState, MediaType, PhysicalDisk, SkippedCheck, Volume};
use std::collections::HashMap;
use std::path::PathBuf;

const STORAGE_NS: &str = "root\\Microsoft\\Windows\\Storage";

fn media_type(code: Option<u32>) -> MediaType {
    match code {
        Some(3) => MediaType::Hdd,
        Some(4) => MediaType::Ssd,
        Some(5) => MediaType::Scm,
        _ => MediaType::Unknown,
    }
}

fn health(code: Option<u32>) -> DiskHealth {
    match code {
        Some(0) => DiskHealth::Healthy,
        Some(1) => DiskHealth::Warning,
        Some(2) => DiskHealth::Unhealthy,
        _ => DiskHealth::Unknown,
    }
}

fn bus_type(code: Option<u32>) -> String {
    match code {
        Some(1) => "SCSI",
        Some(3) => "ATA",
        Some(7) => "USB",
        Some(8) => "RAID",
        Some(10) => "SAS",
        Some(11) => "SATA",
        Some(12) => "SD",
        Some(13) => "MMC",
        Some(15) => "File-backed virtual",
        Some(16) => "Storage Spaces",
        Some(17) => "NVMe",
        _ => "Unknown",
    }
    .to_string()
}

/// Drive letter -> disk number, from MSFT_Partition.
fn partition_map(con: &wmi::WMIConnection) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for r in query(con, "SELECT DiskNumber, DriveLetter FROM MSFT_Partition").unwrap_or_default() {
        let letter = match r.get("DriveLetter") {
            Some(wmi::Variant::UI2(c)) if *c != 0 => char::from_u32(u32::from(*c)),
            Some(wmi::Variant::String(s)) => s.chars().next(),
            _ => None,
        };
        if let (Some(l), Some(n)) = (
            letter.filter(|c| c.is_ascii_alphabetic()),
            get_u32(&r, "DiskNumber"),
        ) {
            map.insert(format!("{}:", l.to_ascii_uppercase()), n.to_string());
        }
    }
    map
}

pub fn disks(ctx: &Ctx) -> CollectResult {
    let con = connect(STORAGE_NS)?;
    let rows = query(
        &con,
        "SELECT DeviceId, FriendlyName, MediaType, BusType, HealthStatus, Size FROM MSFT_PhysicalDisk",
    )?;
    let parts = partition_map(&con);

    let mut disks: Vec<PhysicalDisk> = rows
        .iter()
        .map(|r| {
            let id = get_str(r, "DeviceId").unwrap_or_default();
            let mut volumes: Vec<String> = parts
                .iter()
                .filter(|(_, d)| **d == id)
                .map(|(l, _)| l.clone())
                .collect();
            volumes.sort();
            PhysicalDisk {
                model: get_str(r, "FriendlyName").unwrap_or_default(),
                media_type: media_type(get_u32(r, "MediaType")),
                bus_type: bus_type(get_u32(r, "BusType")),
                size_bytes: get_u64(r, "Size").unwrap_or(0),
                health: health(get_u32(r, "HealthStatus")),
                volumes,
                id,
                ..Default::default()
            }
        })
        .collect();

    let mut skipped = Vec::new();
    // Reliability counters (wear, temperature, errors) need Administrator.
    if ctx.elevated {
        match query(
            &con,
            "SELECT DeviceId, Wear, Temperature, ReadErrorsTotal FROM MSFT_StorageReliabilityCounter",
        ) {
            Ok(rel) => apply_reliability(&mut disks, &rel),
            Err(e) => tracing::debug!("reliability counters: {e}"),
        }
    } else {
        skipped.push(SkippedCheck {
            check: "disk_reliability".into(),
            reason: "needs admin".into(),
        });
    }
    apply_smartctl(&mut disks);

    let patch: Patch = Box::new(move |s| {
        s.hardware.disks = disks;
        s.skipped.extend(skipped);
    });
    Ok(patch)
}

fn apply_reliability(disks: &mut [PhysicalDisk], rows: &[Row]) {
    for r in rows {
        let Some(id) = get_str(r, "DeviceId") else {
            continue;
        };
        if let Some(d) = disks.iter_mut().find(|d| d.id == id) {
            // Wear 0 is also what drives without support report, so only trust > 0.
            d.wear_percent = get_u32(r, "Wear")
                .filter(|w| *w > 0 && *w <= 100)
                .or(d.wear_percent);
            d.temperature_c = get_u32(r, "Temperature").filter(|t| *t > 0 && *t < 120);
            d.read_errors_total = get_u64(r, "ReadErrorsTotal");
        }
    }
}

/// Find smartctl if the user installed smartmontools. Never required.
fn smartctl_path() -> Option<PathBuf> {
    let default = PathBuf::from(r"C:\Program Files\smartmontools\bin\smartctl.exe");
    if default.is_file() {
        return Some(default);
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("smartctl.exe"))
            .find(|f| f.is_file())
    })
}

fn apply_smartctl(disks: &mut [PhysicalDisk]) {
    let Some(exe) = smartctl_path() else { return };
    let Ok(out) = std::process::Command::new(&exe)
        .args(["--scan", "-j"])
        .output()
    else {
        return;
    };
    let Ok(scan) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return;
    };
    let devices = scan["devices"].as_array().cloned().unwrap_or_default();
    for dev in devices {
        let Some(name) = dev["name"].as_str() else {
            continue;
        };
        let Ok(o) = std::process::Command::new(&exe)
            .args(["-a", "-j", name])
            .output()
        else {
            continue;
        };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) else {
            continue;
        };
        let model = v["model_name"].as_str().unwrap_or_default().to_lowercase();
        if model.is_empty() {
            continue;
        }
        if let Some(d) = disks.iter_mut().find(|d| {
            d.model.to_lowercase().contains(&model) || model.contains(&d.model.to_lowercase())
        }) {
            if let Some(passed) = v["smart_status"]["passed"].as_bool() {
                d.smart_failed = Some(!passed);
            }
            if let Some(used) = v["nvme_smart_health_information_log"]["percentage_used"].as_u64() {
                d.wear_percent = u32::try_from(used).ok();
            }
            if let Some(t) = v["temperature"]["current"].as_u64() {
                d.temperature_c = u32::try_from(t).ok();
            }
        }
    }
}

pub fn volumes(ctx: &Ctx) -> CollectResult {
    let con = connect("root\\cimv2")?;
    let rows = query(
        &con,
        "SELECT DeviceID, FileSystem, Size, FreeSpace FROM Win32_LogicalDisk WHERE DriveType = 3",
    )?;
    let system_drive = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".into())
        .to_uppercase();
    let parts = connect(STORAGE_NS)
        .map(|c| partition_map(&c))
        .unwrap_or_default();

    let mut skipped = Vec::new();
    let encryption = if ctx.elevated {
        bitlocker_states()
    } else {
        skipped.push(SkippedCheck {
            check: "bitlocker".into(),
            reason: "needs admin".into(),
        });
        HashMap::new()
    };

    let volumes: Vec<Volume> = rows
        .iter()
        .filter_map(|r| {
            let mount = get_str(r, "DeviceID")?.to_uppercase();
            Some(Volume {
                file_system: get_str(r, "FileSystem"),
                total_bytes: get_u64(r, "Size").unwrap_or(0),
                free_bytes: get_u64(r, "FreeSpace").unwrap_or(0),
                is_system: mount == system_drive,
                encryption: encryption.get(&mount).copied(),
                disk_id: parts.get(&mount).cloned(),
                mount,
            })
        })
        .collect();

    let patch: Patch = Box::new(move |s| {
        s.storage.volumes = volumes;
        s.skipped.extend(skipped);
    });
    Ok(patch)
}

/// BitLocker / Device Encryption state per drive letter (needs admin).
fn bitlocker_states() -> HashMap<String, EncryptionState> {
    let mut map = HashMap::new();
    let rows = match connect("root\\CIMV2\\Security\\MicrosoftVolumeEncryption").and_then(|c| {
        query(
            &c,
            "SELECT DriveLetter, ProtectionStatus, ConversionStatus FROM Win32_EncryptableVolume",
        )
    }) {
        Ok(r) => r,
        Err(e) => {
            if !is_access_denied(&e) {
                tracing::debug!("bitlocker: {e}");
            }
            return map;
        }
    };
    for r in rows {
        let Some(letter) = get_str(&r, "DriveLetter") else {
            continue;
        };
        // ConversionStatus: 0 fully decrypted, 1 fully encrypted, 2 encrypting, 3 decrypting,
        // 4 encryption paused, 5 decryption paused.
        let state = match (
            get_u32(&r, "ConversionStatus"),
            get_u32(&r, "ProtectionStatus"),
        ) {
            (Some(0), _) => EncryptionState::Off,
            (Some(1), _) => EncryptionState::On,
            (Some(2..=5), _) => EncryptionState::InProgress,
            (_, Some(1)) => EncryptionState::On,
            (_, Some(0)) => EncryptionState::Off,
            _ => continue,
        };
        map.insert(letter.to_uppercase(), state);
    }
    map
}
