//! The Snapshot: every fact a scan collected. Pure data, no logic.
//!
//! All structs use `#[serde(default)]` so older or hand-written fixture files
//! with missing fields still load.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Snapshot {
    pub schema_version: u32,
    pub scan_id: String,
    pub taken_at: Option<DateTime<Utc>>,
    /// Was the scan run as Administrator?
    pub elevated: bool,
    /// True if the scan was run with `--offline` (no network tests).
    pub offline: bool,
    pub platform: PlatformInfo,
    pub hardware: HardwareInfo,
    pub performance: PerformanceInfo,
    pub storage: StorageInfo,
    pub devices: Vec<DeviceInfo>,
    pub events: EventSummary,
    pub security: SecurityInfo,
    pub software: Vec<InstalledApp>,
    pub startup: Vec<StartupItem>,
    pub network: NetworkInfo,
    /// Vulnerability matches, filled in by `kosh-vuln` from the local cache.
    pub vulnerabilities: VulnInfo,
    /// Which collectors ran. Rules only judge areas that were collected.
    pub collected: Vec<String>,
    pub skipped: Vec<SkippedCheck>,
    pub errors: Vec<CollectorError>,
}

impl Snapshot {
    pub fn new() -> Self {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            scan_id: uuid::Uuid::new_v4().to_string(),
            taken_at: Some(Utc::now()),
            ..Default::default()
        }
    }

    /// Did the named collector run (successfully or partly)?
    pub fn has(&self, collector: &str) -> bool {
        self.collected.iter().any(|c| c == collector)
    }

    /// The time rules treat as "now". Using the snapshot time keeps replays deterministic.
    pub fn now(&self) -> DateTime<Utc> {
        self.taken_at.unwrap_or_else(Utc::now)
    }

    pub fn system_volume(&self) -> Option<&Volume> {
        self.storage.volumes.iter().find(|v| v.is_system)
    }
}

// ---------------------------------------------------------------- platform

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PlatformInfo {
    /// "windows", "macos" or "linux".
    pub family: String,
    /// Human name, e.g. "Windows 11 Home Single Language".
    pub name: String,
    /// e.g. "10.0.26200"
    pub version: String,
    /// e.g. "25H2" (Windows feature update) or "14.5" (macOS).
    pub display_version: Option<String>,
    pub build: Option<u32>,
    /// Update build revision (Windows UBR).
    pub ubr: Option<u32>,
    /// Raw edition id, e.g. "Professional", "CoreSingleLanguage".
    pub edition: Option<String>,
    pub architecture: String,
    pub install_date: Option<DateTime<Utc>>,
    pub last_boot: Option<DateTime<Utc>>,
    pub uptime_secs: Option<u64>,
}

// ---------------------------------------------------------------- hardware

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HardwareInfo {
    pub system: SystemInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<PhysicalDisk>,
    pub battery: BatteryState,
    pub gpus: Vec<GpuInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SystemInfo {
    pub manufacturer: String,
    pub model: String,
    pub bios_version: Option<String>,
    pub bios_date: Option<DateTime<Utc>>,
    /// True for laptops/tablets. None when unknown.
    pub is_portable: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CpuInfo {
    pub model: String,
    pub cores: u32,
    pub threads: u32,
    pub load_percent: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub sticks: Vec<MemoryStick>,
    /// Number of slots reported by firmware (often unreliable).
    pub slot_count: Option<u32>,
    /// Maximum supported RAM reported by firmware (often unreliable).
    pub max_capacity_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MemoryStick {
    pub capacity_bytes: u64,
    pub speed_mhz: Option<u32>,
    pub memory_type: Option<String>,
    pub slot: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaType {
    Ssd,
    Hdd,
    Scm,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiskHealth {
    Healthy,
    Warning,
    Unhealthy,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PhysicalDisk {
    pub id: String,
    pub model: String,
    pub media_type: MediaType,
    pub bus_type: String,
    pub size_bytes: u64,
    pub health: DiskHealth,
    /// Percentage of rated life used (SSD), if known.
    pub wear_percent: Option<u32>,
    pub temperature_c: Option<u32>,
    pub read_errors_total: Option<u64>,
    /// Did smartctl (if installed) report the overall SMART test as failed?
    pub smart_failed: Option<bool>,
    /// Drive letters / mount points on this disk.
    pub volumes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state")]
pub enum BatteryState {
    /// The battery was not checked (collector did not run or failed).
    #[default]
    Unknown,
    /// No battery: a desktop. Not an error.
    NotPresent,
    Present(BatteryInfo),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BatteryInfo {
    pub name: Option<String>,
    pub design_capacity_mwh: Option<u64>,
    pub full_charge_capacity_mwh: Option<u64>,
    pub cycle_count: Option<u32>,
    pub charge_percent: Option<u32>,
}

impl BatteryInfo {
    /// Full charge capacity as a percentage of design capacity.
    pub fn health_percent(&self) -> Option<f64> {
        match (self.design_capacity_mwh, self.full_charge_capacity_mwh) {
            (Some(d), Some(f)) if d > 0 => Some(f as f64 * 100.0 / d as f64),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GpuInfo {
    pub name: String,
    pub driver_version: Option<String>,
    pub driver_date: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------- performance

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PerformanceInfo {
    pub sample_interval_secs: u32,
    pub samples: Vec<MemorySample>,
    pub top_by_memory: Vec<AppUsage>,
    pub top_by_cpu: Vec<AppUsage>,
    /// Summary of `kosh monitor` history, when available.
    pub history: Option<MonitorSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MemorySample {
    pub available_mb: u64,
    pub total_mb: u64,
    pub commit_percent: Option<f64>,
    pub pages_per_sec: Option<f64>,
}

impl MemorySample {
    pub fn available_percent(&self) -> f64 {
        if self.total_mb == 0 {
            return 100.0;
        }
        self.available_mb as f64 * 100.0 / self.total_mb as f64
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppUsage {
    /// App name, e.g. "chrome" (all chrome.exe processes grouped).
    pub name: String,
    pub process_count: u32,
    pub memory_bytes: u64,
    pub cpu_percent: f32,
}

/// Summary of background monitoring (`kosh monitor`) over several hours/days.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MonitorSummary {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub sample_count: u32,
    /// Histogram of available-memory percent in 5% buckets: [0,5), [5,10) ... [95,100].
    pub available_hist: Vec<u32>,
    /// Histogram of commit percent in 5% buckets: [0,5) ... [95,100), [100,...).
    pub commit_hist: Vec<u32>,
    pub avg_available_percent: f64,
    pub avg_cpu_percent: f64,
}

impl MonitorSummary {
    pub const BUCKET: f64 = 5.0;

    pub fn bucket(percent: f64, buckets: usize) -> usize {
        let i = (percent.max(0.0) / Self::BUCKET).floor() as usize;
        i.min(buckets.saturating_sub(1))
    }

    /// Share (0..1) of samples with available memory below `percent` (rounded down to 5%).
    pub fn share_available_below(&self, percent: f64) -> f64 {
        let n = (percent / Self::BUCKET).floor() as usize;
        share(&self.available_hist, 0, n)
    }

    /// Share (0..1) of samples with commit at or above `percent` (rounded down to 5%).
    pub fn share_commit_at_least(&self, percent: f64) -> f64 {
        let n = (percent / Self::BUCKET).floor() as usize;
        share(&self.commit_hist, n, self.commit_hist.len())
    }
}

fn share(hist: &[u32], from: usize, to: usize) -> f64 {
    let total: u32 = hist.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let part: u32 = hist.iter().skip(from).take(to.saturating_sub(from)).sum();
    f64::from(part) / f64::from(total)
}

// ---------------------------------------------------------------- storage

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StorageInfo {
    pub volumes: Vec<Volume>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Volume {
    /// "C:" on Windows, "/" on Linux/macOS.
    pub mount: String,
    pub file_system: Option<String>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Is Windows (or the OS) installed on this volume?
    pub is_system: bool,
    /// "On", "Off", or None when unknown (e.g. needs admin).
    pub encryption: Option<EncryptionState>,
    /// Physical disk id this volume lives on.
    pub disk_id: Option<String>,
}

impl Volume {
    pub fn free_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 100.0;
        }
        self.free_bytes as f64 * 100.0 / self.total_bytes as f64
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EncryptionState {
    On,
    Off,
    /// Encryption or decryption in progress.
    InProgress,
}

// ---------------------------------------------------------------- devices

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DeviceInfo {
    pub name: String,
    /// PnP class, e.g. "Net", "Display", "AudioEndpoint", "DiskDrive".
    pub class: String,
    pub status: Option<String>,
    /// Windows ConfigManagerErrorCode. 0 = working.
    pub problem_code: u32,
    pub driver_version: Option<String>,
    pub driver_date: Option<DateTime<Utc>>,
    pub driver_provider: Option<String>,
    pub driver_signed: Option<bool>,
    pub present: bool,
}

// ---------------------------------------------------------------- events

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventSummary {
    pub window_days: u32,
    pub groups: Vec<EventGroup>,
    /// Dates of crash dump files (C:\Windows\Minidump).
    pub minidumps: Vec<DateTime<Utc>>,
    /// Windows Reliability Monitor stability index (1–10), if available.
    pub stability_index: Option<f64>,
}

impl EventSummary {
    pub fn group(&self, kind: EventKind) -> Option<&EventGroup> {
        self.groups.iter().find(|g| g.kind == kind)
    }

    pub fn count(&self, kind: EventKind) -> u32 {
        self.group(kind).map(|g| g.count).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    UnexpectedShutdown,
    PreviousShutdownUnexpected,
    BlueScreen,
    HardwareError,
    DiskError,
    FileSystemCorruption,
    ServiceFailure,
    AppCrash,
    AppHang,
    LowMemory,
    UpdateFailure,
    UpdateSuccess,
    DeviceInstallProblem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventGroup {
    pub kind: EventKind,
    pub log: String,
    pub providers: Vec<String>,
    pub ids: Vec<u32>,
    pub count: u32,
    /// Up to 5 most recent samples.
    pub samples: Vec<EventSample>,
    /// Counts per subject (app name, service name, bugcheck code, update title).
    pub by_subject: BTreeMap<String, u32>,
    /// Most recent event time.
    pub last_seen: Option<DateTime<Utc>>,
}

impl Default for EventGroup {
    fn default() -> Self {
        EventGroup {
            kind: EventKind::AppCrash,
            log: String::new(),
            providers: Vec::new(),
            ids: Vec::new(),
            count: 0,
            samples: Vec::new(),
            by_subject: BTreeMap::new(),
            last_seen: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventSample {
    pub time: Option<DateTime<Utc>>,
    pub provider: String,
    pub id: u32,
    pub subject: Option<String>,
    pub message: Option<String>,
}

// ---------------------------------------------------------------- security

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SecurityInfo {
    pub antivirus: Vec<AvProduct>,
    pub defender: Option<DefenderStatus>,
    pub firewall: Vec<FirewallProfile>,
    pub updates: UpdateInfo,
    pub pending_reboot: PendingReboot,
    pub secure_boot: Option<bool>,
    pub tpm: Option<TpmInfo>,
    pub smb1_enabled: Option<bool>,
    pub rdp_enabled: Option<bool>,
    pub uac_enabled: Option<bool>,
    pub guest_enabled: Option<bool>,
    /// File names that look like plain-text secrets (names only, never contents).
    pub secret_file_names: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AvProduct {
    pub name: String,
    pub enabled: Option<bool>,
    pub up_to_date: Option<bool>,
    pub raw_state: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DefenderStatus {
    pub antivirus_enabled: Option<bool>,
    pub realtime_enabled: Option<bool>,
    pub signature_last_updated: Option<DateTime<Utc>>,
    pub signature_age_days: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FirewallProfile {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateInfo {
    pub hotfix_count: u32,
    pub last_hotfix_installed: Option<DateTime<Utc>>,
    /// Last successful update from the Windows Update history, if readable.
    pub last_successful_update: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PendingReboot {
    pub component_servicing: bool,
    pub windows_update: bool,
    pub file_rename_operations: bool,
}

impl PendingReboot {
    pub fn any(&self) -> bool {
        self.component_servicing || self.windows_update || self.file_rename_operations
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TpmInfo {
    pub present: bool,
    pub enabled: Option<bool>,
    pub version: Option<String>,
}

// ---------------------------------------------------------------- software

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct InstalledApp {
    pub name: String,
    pub version: Option<String>,
    pub publisher: Option<String>,
    pub install_date: Option<String>,
    pub install_location: Option<String>,
    /// "HKLM", "HKLM-WOW64", "HKCU", "AppX", "dpkg", ...
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    /// Executable path extracted from the command, when possible.
    pub image_path: Option<String>,
    /// e.g. "HKCU Run", "Startup folder (user)", "Scheduled task", "Service".
    pub location: String,
    pub publisher: Option<String>,
    /// Has a valid embedded Authenticode signature? None = not checked.
    pub signed: Option<bool>,
    pub enabled: bool,
    pub is_microsoft: bool,
}

// ---------------------------------------------------------------- network

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetworkInfo {
    pub adapters: Vec<NetAdapter>,
    /// None when skipped (offline mode).
    pub dns_ok: Option<bool>,
    pub internet_ok: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct NetAdapter {
    pub name: String,
    pub description: String,
    /// Physical hardware (not virtual / VPN / Bluetooth PAN)?
    pub hardware: bool,
    pub connected: bool,
    pub up: bool,
    /// Device problem reported by the adapter (driver error).
    pub has_error: bool,
    pub link_speed_bps: Option<u64>,
}

// ---------------------------------------------------------------- vulnerabilities

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VulnInfo {
    /// Did vulnerability matching run? False when the cache is missing/stale.
    pub checked: bool,
    pub cache_updated: Option<DateTime<Utc>>,
    pub matches: Vec<VulnMatch>,
    pub outdated: Vec<OutdatedApp>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VulnMatch {
    pub app_name: String,
    pub product: String,
    pub installed_version: String,
    pub cve_id: String,
    pub cvss: Option<f64>,
    pub in_kev: bool,
    /// The range that matched, e.g. "< 23.01".
    pub matched_range: String,
    /// Smallest fixed version we know of, if any.
    pub fixed_version: Option<String>,
    pub update_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OutdatedApp {
    pub app_name: String,
    pub product: String,
    pub installed_version: String,
    pub latest_version: String,
    pub update_url: Option<String>,
}

// ---------------------------------------------------------------- bookkeeping

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SkippedCheck {
    pub check: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CollectorError {
    pub collector: String,
    pub message: String,
}
