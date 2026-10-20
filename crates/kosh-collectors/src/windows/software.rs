//! Installed programs from the registry Uninstall keys (64-bit, 32-bit and per-user).

use super::reg::{self, Hive};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::InstalledApp;
use std::collections::HashSet;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

pub fn software(_ctx: &Ctx) -> CollectResult {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for hive in [Hive::Hklm64, Hive::Hklm32, Hive::Hkcu] {
        let Some(root) = reg::open(hive, UNINSTALL) else {
            continue;
        };
        for sub in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&sub) else {
                continue;
            };
            let Some(name) = reg::get_string(&k, "DisplayName") else {
                continue;
            };
            // Skip hidden system components and updates/patches of other products.
            if reg::get_dword(&k, "SystemComponent") == Some(1) {
                continue;
            }
            if reg::get_string(&k, "ParentKeyName").is_some() {
                continue;
            }
            if matches!(
                reg::get_string(&k, "ReleaseType").as_deref(),
                Some("Update" | "Hotfix" | "Security Update")
            ) {
                continue;
            }
            let version = reg::get_string(&k, "DisplayVersion");
            if !seen.insert((name.to_lowercase(), version.clone())) {
                continue;
            }
            apps.push(InstalledApp {
                name,
                version,
                publisher: reg::get_string(&k, "Publisher"),
                install_date: reg::get_string(&k, "InstallDate"),
                install_location: reg::get_string(&k, "InstallLocation"),
                source: hive.label().to_string(),
            });
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    let patch: Patch = Box::new(move |s| s.software = apps);
    Ok(patch)
}
