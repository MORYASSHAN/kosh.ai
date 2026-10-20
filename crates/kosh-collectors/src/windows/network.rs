//! Network adapters: connected, up, driver errors.

use super::wmi_util::{get_bool, get_str, get_u32, get_u64, query_ns};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::NetAdapter;
use std::collections::HashMap;

pub fn network(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\StandardCimv2",
        "SELECT Name, InterfaceDescription, HardwareInterface, MediaConnectState, InterfaceOperationalStatus, Speed FROM MSFT_NetAdapter",
    )?;
    // Driver problem codes for physical adapters, keyed by description.
    let errors: HashMap<String, u32> = query_ns(
        "root\\cimv2",
        "SELECT Name, ConfigManagerErrorCode FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE",
    )
    .unwrap_or_default()
    .iter()
    .filter_map(|r| Some((get_str(r, "Name")?.to_lowercase(), get_u32(r, "ConfigManagerErrorCode")?)))
    .collect();

    let adapters: Vec<NetAdapter> = rows
        .iter()
        .map(|r| {
            let description = get_str(r, "InterfaceDescription").unwrap_or_default();
            let code = errors
                .get(&description.to_lowercase())
                .copied()
                .unwrap_or(0);
            NetAdapter {
                name: get_str(r, "Name").unwrap_or_default(),
                hardware: get_bool(r, "HardwareInterface").unwrap_or(false),
                // MediaConnectState 1 = connected; InterfaceOperationalStatus 1 = up.
                connected: get_u32(r, "MediaConnectState") == Some(1),
                up: get_u32(r, "InterfaceOperationalStatus") == Some(1),
                // Code 22 = disabled by the user: not an error.
                has_error: code != 0 && code != 22,
                link_speed_bps: get_u64(r, "Speed").filter(|s| *s > 0 && *s < u64::MAX / 2),
                description,
            }
        })
        .collect();

    let patch: Patch = Box::new(move |s| s.network.adapters = adapters);
    Ok(patch)
}
