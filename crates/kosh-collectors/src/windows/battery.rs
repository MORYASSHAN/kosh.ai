//! Battery design and full-charge capacity.
//!
//! WMI `BatteryStaticData` fails without admin (see VERIFIED_SOURCES.md), so the
//! design capacity is read from the battery device with IOCTL_BATTERY_QUERY_INFORMATION,
//! which works as a normal user. Opening the device is read-only querying.

use super::wmi_util::{get_str, get_u32, get_u64, query_ns};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::{BatteryInfo, BatteryState};
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, GUID_DEVCLASS_BATTERY,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Power::{
    BatteryInformation, BATTERY_INFORMATION, BATTERY_QUERY_INFORMATION,
    IOCTL_BATTERY_QUERY_INFORMATION, IOCTL_BATTERY_QUERY_TAG,
};
use windows::Win32::System::IO::DeviceIoControl;

pub fn battery(_ctx: &Ctx) -> CollectResult {
    let wmi_rows = query_ns(
        "root\\cimv2",
        "SELECT Name, EstimatedChargeRemaining FROM Win32_Battery",
    )
    .unwrap_or_default();
    let ioctl = read_battery_ioctl();

    if wmi_rows.is_empty() && ioctl.is_none() {
        let patch: Patch = Box::new(|s| s.hardware.battery = BatteryState::NotPresent);
        return Ok(patch);
    }

    let mut info = BatteryInfo {
        name: wmi_rows.first().and_then(|r| get_str(r, "Name")),
        charge_percent: wmi_rows
            .first()
            .and_then(|r| get_u32(r, "EstimatedChargeRemaining")),
        ..Default::default()
    };
    if let Some(b) = ioctl {
        info.design_capacity_mwh = Some(u64::from(b.DesignedCapacity)).filter(|n| *n > 0);
        info.full_charge_capacity_mwh = Some(u64::from(b.FullChargedCapacity)).filter(|n| *n > 0);
        info.cycle_count = Some(b.CycleCount).filter(|n| *n > 0);
    }
    // Fallback for full charge capacity (readable without admin).
    if info.full_charge_capacity_mwh.is_none() {
        info.full_charge_capacity_mwh = query_ns(
            "root\\wmi",
            "SELECT FullChargedCapacity FROM BatteryFullChargedCapacity",
        )
        .ok()
        .and_then(|r| r.first().and_then(|r| get_u64(r, "FullChargedCapacity")))
        .filter(|n| *n > 0);
    }
    let patch: Patch = Box::new(move |s| s.hardware.battery = BatteryState::Present(info));
    Ok(patch)
}

/// First battery's BATTERY_INFORMATION, if any battery device exists.
fn read_battery_ioctl() -> Option<BATTERY_INFORMATION> {
    // SAFETY: SetupAPI calls follow the documented two-call pattern; every handle is released.
    unsafe {
        let devs = SetupDiGetClassDevsW(
            Some(&GUID_DEVCLASS_BATTERY),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        )
        .ok()?;
        let result = (|| {
            let mut did = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            SetupDiEnumDeviceInterfaces(devs, None, &GUID_DEVCLASS_BATTERY, 0, &mut did).ok()?;
            let mut required = 0u32;
            let _ =
                SetupDiGetDeviceInterfaceDetailW(devs, &did, None, 0, Some(&mut required), None);
            if required == 0 {
                return None;
            }
            // Buffer aligned for the detail struct.
            let mut buf = vec![0u64; (required as usize).div_ceil(8)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            SetupDiGetDeviceInterfaceDetailW(devs, &did, Some(detail), required, None, None)
                .ok()?;
            let path = PCWSTR((*detail).DevicePath.as_ptr());

            let handle = CreateFileW(
                path,
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .ok()?;
            let info = query_info(handle);
            let _ = CloseHandle(handle);
            info
        })();
        let _ = SetupDiDestroyDeviceInfoList(devs);
        result
    }
}

unsafe fn query_info(handle: HANDLE) -> Option<BATTERY_INFORMATION> {
    let wait: u32 = 0;
    let mut tag: u32 = 0;
    let mut returned = 0u32;
    DeviceIoControl(
        handle,
        IOCTL_BATTERY_QUERY_TAG,
        Some(&wait as *const u32 as *const core::ffi::c_void),
        4,
        Some(&mut tag as *mut u32 as *mut core::ffi::c_void),
        4,
        Some(&mut returned),
        None,
    )
    .ok()?;
    if tag == 0 {
        return None;
    }
    let q = BATTERY_QUERY_INFORMATION {
        BatteryTag: tag,
        InformationLevel: BatteryInformation,
        AtRate: 0,
    };
    let mut info = BATTERY_INFORMATION::default();
    DeviceIoControl(
        handle,
        IOCTL_BATTERY_QUERY_INFORMATION,
        Some(&q as *const _ as *const core::ffi::c_void),
        std::mem::size_of::<BATTERY_QUERY_INFORMATION>() as u32,
        Some(&mut info as *mut _ as *mut core::ffi::c_void),
        std::mem::size_of::<BATTERY_INFORMATION>() as u32,
        Some(&mut returned),
        None,
    )
    .ok()?;
    Some(info)
}
