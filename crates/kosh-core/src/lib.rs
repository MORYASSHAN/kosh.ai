//! Shared types for Kosh Doctor.
//!
//! A scan produces a [`Snapshot`] (pure facts). Rules turn a Snapshot into
//! [`Finding`]s. Both are plain JSON so they can be saved and replayed in tests.

pub mod anonymize;
pub mod finding;
pub mod snapshot;
pub mod updates;

pub use anonymize::Anonymizer;
pub use finding::*;
pub use snapshot::*;

/// Names of the collectors, used in `Snapshot::collected`, skipped checks and `kosh verify`.
pub mod collectors {
    pub const OS: &str = "os";
    pub const SYSTEM: &str = "system";
    pub const CPU: &str = "cpu";
    pub const MEMORY: &str = "memory";
    pub const MEMORY_PRESSURE: &str = "memory_pressure";
    pub const PROCESSES: &str = "processes";
    pub const DISKS: &str = "disks";
    pub const VOLUMES: &str = "volumes";
    pub const BATTERY: &str = "battery";
    pub const GPU: &str = "gpu";
    pub const DEVICES: &str = "devices";
    pub const EVENTS: &str = "events";
    pub const RELIABILITY: &str = "reliability";
    pub const CRASH_DUMPS: &str = "crash_dumps";
    pub const PENDING_REBOOT: &str = "pending_reboot";
    pub const UPDATES: &str = "updates";
    pub const ANTIVIRUS: &str = "antivirus";
    pub const FIREWALL: &str = "firewall";
    pub const SECURE_BOOT: &str = "secure_boot";
    pub const TPM: &str = "tpm";
    pub const RISKY_SETTINGS: &str = "risky_settings";
    pub const SECRET_FILES: &str = "secret_files";
    pub const SOFTWARE: &str = "software";
    pub const STARTUP: &str = "startup";
    pub const NETWORK: &str = "network";
    pub const CONNECTIVITY: &str = "connectivity";
    pub const VULNERABILITIES: &str = "vulnerabilities";
    pub const MONITOR_HISTORY: &str = "monitor_history";

    pub const ALL: [&str; 28] = [
        OS,
        SYSTEM,
        CPU,
        MEMORY,
        MEMORY_PRESSURE,
        PROCESSES,
        DISKS,
        VOLUMES,
        BATTERY,
        GPU,
        DEVICES,
        EVENTS,
        RELIABILITY,
        CRASH_DUMPS,
        PENDING_REBOOT,
        UPDATES,
        ANTIVIRUS,
        FIREWALL,
        SECURE_BOOT,
        TPM,
        RISKY_SETTINGS,
        SECRET_FILES,
        SOFTWARE,
        STARTUP,
        NETWORK,
        CONNECTIVITY,
        VULNERABILITIES,
        MONITOR_HISTORY,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trips_through_json() {
        let mut s = Snapshot::new();
        s.platform.family = "windows".into();
        s.hardware.battery = BatteryState::Present(BatteryInfo {
            design_capacity_mwh: Some(60000),
            full_charge_capacity_mwh: Some(30000),
            ..Default::default()
        });
        let json = serde_json::to_string(&s).expect("serialize");
        let back: Snapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(s, back);
    }

    #[test]
    fn empty_json_object_is_a_valid_snapshot() {
        let s: Snapshot = serde_json::from_str("{}").expect("deserialize");
        assert_eq!(s.hardware.battery, BatteryState::Unknown);
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::Info < Severity::Low);
        assert!(Severity::High.is_urgent());
        assert!(!Severity::Medium.is_urgent());
    }

    #[test]
    fn battery_health() {
        let b = BatteryInfo {
            design_capacity_mwh: Some(50000),
            full_charge_capacity_mwh: Some(25000),
            ..Default::default()
        };
        assert_eq!(b.health_percent(), Some(50.0));
    }

    #[test]
    fn safe_action_serializes_with_type_tag() {
        let a = SafeAction::OpenSettings("ms-settings:windowsupdate".into());
        let json = serde_json::to_string(&a).expect("serialize");
        assert_eq!(
            json,
            r#"{"type":"OpenSettings","target":"ms-settings:windowsupdate"}"#
        );
    }
}
