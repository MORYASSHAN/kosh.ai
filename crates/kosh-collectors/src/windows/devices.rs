//! Plug and Play devices with their problem codes, joined with driver details.

use super::wmi_util::{connect, get_bool, get_date, get_str, get_u32, query};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::DeviceInfo;
use std::collections::HashMap;

pub fn devices(_ctx: &Ctx) -> CollectResult {
    let con = connect("root\\cimv2")?;
    let entities = query(
        &con,
        "SELECT DeviceID, Name, PNPClass, Status, ConfigManagerErrorCode, Present FROM Win32_PnPEntity",
    )?;

    let mut drivers: HashMap<String, super::wmi_util::Row> = HashMap::new();
    for r in query(
        &con,
        "SELECT DeviceID, DriverVersion, DriverDate, DriverProviderName, IsSigned FROM Win32_PnPSignedDriver",
    )
    .unwrap_or_default()
    {
        if let Some(id) = get_str(&r, "DeviceID") {
            drivers.insert(id.to_uppercase(), r);
        }
    }

    let devices: Vec<DeviceInfo> = entities
        .iter()
        .filter_map(|r| {
            let name = get_str(r, "Name")?;
            let id = get_str(r, "DeviceID").unwrap_or_default().to_uppercase();
            let d = drivers.get(&id);
            Some(DeviceInfo {
                name,
                class: get_str(r, "PNPClass").unwrap_or_default(),
                status: get_str(r, "Status"),
                problem_code: get_u32(r, "ConfigManagerErrorCode").unwrap_or(0),
                driver_version: d.and_then(|d| get_str(d, "DriverVersion")),
                driver_date: d.and_then(|d| get_date(d, "DriverDate")),
                driver_provider: d.and_then(|d| get_str(d, "DriverProviderName")),
                driver_signed: d.and_then(|d| get_bool(d, "IsSigned")),
                // `Present` exists on Windows 10+; assume present when missing.
                present: get_bool(r, "Present").unwrap_or(true),
            })
        })
        .collect();

    let patch: Patch = Box::new(move |s| s.devices = devices);
    Ok(patch)
}
