//! OS, system, CPU, memory and GPU facts.

use super::reg;
use super::wmi_util::{connect, get_date, get_str, get_u32, get_u64, query, query_ns};
use crate::{CollectResult, Ctx, Patch};
use chrono::Utc;
use kosh_core::{GpuInfo, MemoryStick};

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

pub fn os(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\cimv2",
        "SELECT Caption, Version, BuildNumber, OSArchitecture, InstallDate, LastBootUpTime FROM Win32_OperatingSystem",
    )?;
    let row = rows
        .into_iter()
        .next()
        .ok_or("Win32_OperatingSystem returned no rows")?;

    let caption = get_str(&row, "Caption").unwrap_or_default();
    let name = caption.trim_start_matches("Microsoft ").to_string();
    let version = get_str(&row, "Version").unwrap_or_default();
    let build = get_u32(&row, "BuildNumber")
        .or_else(|| reg::hklm_string(CURRENT_VERSION, "CurrentBuild").and_then(|b| b.parse().ok()));
    let install_date = get_date(&row, "InstallDate");
    let last_boot = get_date(&row, "LastBootUpTime");
    let arch = get_str(&row, "OSArchitecture");

    // DisplayVersion (20H2 and later) or ReleaseId (older Windows 10).
    let display_version = reg::hklm_string(CURRENT_VERSION, "DisplayVersion")
        .or_else(|| reg::hklm_string(CURRENT_VERSION, "ReleaseId"));
    let ubr = reg::hklm_dword(CURRENT_VERSION, "UBR");
    let edition = reg::hklm_string(CURRENT_VERSION, "EditionID");

    let uptime = last_boot.map(|b| (Utc::now() - b).num_seconds().max(0) as u64);

    let patch: Patch = Box::new(move |s| {
        let p = &mut s.platform;
        p.family = "windows".into();
        p.name = name;
        p.version = version;
        p.build = build;
        p.ubr = ubr;
        p.display_version = display_version;
        p.edition = edition;
        if let Some(a) = arch {
            p.architecture = a;
        }
        p.install_date = install_date;
        p.last_boot = last_boot;
        p.uptime_secs = uptime;
    });
    Ok(patch)
}

/// SMBIOS chassis types that are portable: laptop, notebook, sub-notebook, handheld,
/// tablet, convertible, detachable.
const PORTABLE_CHASSIS: [u64; 8] = [8, 9, 10, 11, 14, 30, 31, 32];

pub fn system(_ctx: &Ctx) -> CollectResult {
    let con = connect("root\\cimv2")?;
    let cs = query(
        &con,
        "SELECT Manufacturer, Model, PCSystemType FROM Win32_ComputerSystem",
    )?;
    let cs = cs
        .into_iter()
        .next()
        .ok_or("Win32_ComputerSystem returned no rows")?;
    let bios = query(
        &con,
        "SELECT SMBIOSBIOSVersion, ReleaseDate FROM Win32_BIOS",
    )
    .ok()
    .and_then(|r| r.into_iter().next());
    let chassis_portable = query(&con, "SELECT ChassisTypes FROM Win32_SystemEnclosure")
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|row| match row.get("ChassisTypes") {
            Some(wmi::Variant::Array(items)) => Some(items.iter().any(|v| {
                let n = match v {
                    wmi::Variant::UI2(n) => u64::from(*n),
                    wmi::Variant::I4(n) => u64::try_from(*n).unwrap_or(0),
                    wmi::Variant::UI4(n) => u64::from(*n),
                    _ => 0,
                };
                PORTABLE_CHASSIS.contains(&n)
            })),
            _ => None,
        });

    let manufacturer = get_str(&cs, "Manufacturer").unwrap_or_default();
    let model = get_str(&cs, "Model").unwrap_or_default();
    // PCSystemType 2 = Mobile.
    let pc_mobile = get_u32(&cs, "PCSystemType").map(|t| t == 2);
    let is_portable = match (pc_mobile, chassis_portable) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        (Some(false), None) | (None, Some(false)) => Some(false),
        (None, None) => None,
    };
    let bios_version = bios.as_ref().and_then(|b| get_str(b, "SMBIOSBIOSVersion"));
    let bios_date = bios.as_ref().and_then(|b| get_date(b, "ReleaseDate"));

    let patch: Patch = Box::new(move |s| {
        let sys = &mut s.hardware.system;
        sys.manufacturer = manufacturer;
        sys.model = model;
        sys.bios_version = bios_version;
        sys.bios_date = bios_date;
        sys.is_portable = is_portable;
    });
    Ok(patch)
}

pub fn cpu(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\cimv2",
        "SELECT Name, NumberOfCores, NumberOfLogicalProcessors FROM Win32_Processor",
    )?;
    let model = rows
        .first()
        .and_then(|r| get_str(r, "Name"))
        .unwrap_or_default();
    let cores = rows
        .iter()
        .filter_map(|r| get_u32(r, "NumberOfCores"))
        .sum();
    let threads = rows
        .iter()
        .filter_map(|r| get_u32(r, "NumberOfLogicalProcessors"))
        .sum();
    let patch: Patch = Box::new(move |s| {
        s.hardware.cpu.model = model;
        s.hardware.cpu.cores = cores;
        s.hardware.cpu.threads = threads;
    });
    Ok(patch)
}

fn memory_type_name(code: u32) -> Option<&'static str> {
    // SMBIOS memory type codes (SMBIOSMemoryType).
    Some(match code {
        20 => "DDR",
        21 => "DDR2",
        24 => "DDR3",
        26 => "DDR4",
        27 => "LPDDR",
        28 => "LPDDR2",
        29 => "LPDDR3",
        30 => "LPDDR4",
        34 => "DDR5",
        35 => "LPDDR5",
        _ => return None,
    })
}

pub fn memory(_ctx: &Ctx) -> CollectResult {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory();
    let available = sys.available_memory();

    let con = connect("root\\cimv2")?;
    let sticks: Vec<MemoryStick> = query(
        &con,
        "SELECT Capacity, Speed, ConfiguredClockSpeed, SMBIOSMemoryType, DeviceLocator FROM Win32_PhysicalMemory",
    )
    .unwrap_or_default()
    .iter()
    .map(|r| MemoryStick {
        capacity_bytes: get_u64(r, "Capacity").unwrap_or(0),
        speed_mhz: get_u32(r, "Speed").or_else(|| get_u32(r, "ConfiguredClockSpeed")),
        memory_type: get_u32(r, "SMBIOSMemoryType")
            .and_then(memory_type_name)
            .map(str::to_string),
        slot: get_str(r, "DeviceLocator"),
    })
    .collect();

    // Several arrays may exist (e.g. flash); the system memory array has the most slots.
    let arrays = query(
        &con,
        "SELECT MemoryDevices, MaxCapacity, MaxCapacityEx FROM Win32_PhysicalMemoryArray",
    )
    .unwrap_or_default();
    let main = arrays
        .iter()
        .max_by_key(|r| get_u32(r, "MemoryDevices").unwrap_or(0));
    let slot_count = main.and_then(|r| get_u32(r, "MemoryDevices"));
    // Both values are in kilobytes.
    let max_kb = main.and_then(|r| {
        get_u64(r, "MaxCapacityEx")
            .filter(|n| *n > 0)
            .or_else(|| get_u64(r, "MaxCapacity"))
    });

    let patch: Patch = Box::new(move |s| {
        let m = &mut s.hardware.memory;
        m.total_bytes = total;
        m.available_bytes = available;
        m.sticks = sticks;
        m.slot_count = slot_count;
        m.max_capacity_bytes = max_kb.map(|kb| kb * 1024);
    });
    Ok(patch)
}

pub fn gpu(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\cimv2",
        "SELECT Name, DriverVersion, DriverDate, AdapterCompatibility FROM Win32_VideoController",
    )?;
    let gpus: Vec<GpuInfo> = rows
        .iter()
        // Skip remote-desktop / virtual display adapters.
        .filter(|r| {
            let n = get_str(r, "Name").unwrap_or_default().to_lowercase();
            !(n.contains("remote display") || n.contains("virtual") || n.contains("mirage"))
        })
        .map(|r| GpuInfo {
            name: get_str(r, "Name").unwrap_or_default(),
            driver_version: get_str(r, "DriverVersion"),
            driver_date: get_date(r, "DriverDate"),
        })
        .collect();
    let patch: Patch = Box::new(move |s| s.hardware.gpus = gpus);
    Ok(patch)
}
