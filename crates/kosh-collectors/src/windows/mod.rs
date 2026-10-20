//! Windows collectors (Tier 1). Every source used here is listed in
//! `docs/VERIFIED_SOURCES.md` with the machine it was verified on.

use crate::Collector;
use kosh_core::collectors as names;

mod battery;
mod devices;
pub mod elevation;
mod events;
mod network;
mod os;
pub mod perf;
mod reg;
mod security;
mod software;
mod startup;
mod storage;
mod winfile;
mod wmi_util;

pub fn collectors() -> Vec<Collector> {
    fn c(name: &'static str, run: fn(&crate::Ctx) -> crate::CollectResult) -> Collector {
        Collector {
            name,
            run,
            long_running: false,
        }
    }
    vec![
        c(names::OS, os::os),
        c(names::SYSTEM, os::system),
        c(names::CPU, os::cpu),
        c(names::MEMORY, os::memory),
        c(names::GPU, os::gpu),
        Collector {
            name: names::MEMORY_PRESSURE,
            run: perf::memory_pressure,
            long_running: true,
        },
        c(names::DISKS, storage::disks),
        c(names::VOLUMES, storage::volumes),
        c(names::BATTERY, battery::battery),
        c(names::DEVICES, devices::devices),
        c(names::EVENTS, events::events),
        c(names::RELIABILITY, events::reliability),
        c(names::CRASH_DUMPS, events::crash_dumps),
        c(names::PENDING_REBOOT, security::pending_reboot),
        c(names::UPDATES, security::updates),
        c(names::ANTIVIRUS, security::antivirus),
        c(names::FIREWALL, security::firewall),
        c(names::SECURE_BOOT, security::secure_boot),
        c(names::TPM, security::tpm),
        c(names::RISKY_SETTINGS, security::risky_settings),
        c(names::SOFTWARE, software::software),
        c(names::STARTUP, startup::startup),
        c(names::NETWORK, network::network),
    ]
}
