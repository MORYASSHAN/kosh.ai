//! Read-only registry helpers. Keys are always opened with KEY_READ.

use winreg::enums::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
};
use winreg::RegKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    Hklm64,
    Hklm32,
    Hkcu,
}

impl Hive {
    pub fn label(self) -> &'static str {
        match self {
            Hive::Hklm64 => "HKLM",
            Hive::Hklm32 => "HKLM-WOW64",
            Hive::Hkcu => "HKCU",
        }
    }
}

pub fn open(hive: Hive, path: &str) -> Option<RegKey> {
    let (root, flags) = match hive {
        Hive::Hklm64 => (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        Hive::Hklm32 => (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
        Hive::Hkcu => (HKEY_CURRENT_USER, KEY_READ),
    };
    RegKey::predef(root)
        .open_subkey_with_flags(path, flags)
        .ok()
}

pub fn key_exists(hive: Hive, path: &str) -> bool {
    open(hive, path).is_some()
}

pub fn get_string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn get_dword(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}

pub fn hklm_string(path: &str, name: &str) -> Option<String> {
    open(Hive::Hklm64, path).and_then(|k| get_string(&k, name))
}

pub fn hklm_dword(path: &str, name: &str) -> Option<u32> {
    open(Hive::Hklm64, path).and_then(|k| get_dword(&k, name))
}
