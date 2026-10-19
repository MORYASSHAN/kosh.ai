# Kosh Doctor  Build Specification

> **Read this whole file before writing any code.**
> This README is written for an AI coding agent (Claude Code) and for the human owner.
> It defines *what* to build, *how* to build it, *in what order*, and *how to prove each step works*.
> The owner is not a programmer. Explain what you are doing in plain language, and ask the owner whenever a step needs something only a human can provide (accounts, hardware, keys, decisions).

---

## 0. Rules for the coding agent

1. **Build one milestone at a time** (Section 16). Do not start the next milestone until every acceptance check of the current one passes.
2. **Run the tests after every change**: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all`. Fix failures before moving on.
3. **Never invent system facts.** Before relying on any Windows registry path, WMI class, event ID or command output format, verify it on the owner's real machine (ask the owner to run a PowerShell command and paste the output, or run it yourself if you have shell access). Record verified facts in `docs/VERIFIED_SOURCES.md` with the date and Windows version.
4. **Version 1 is read-only.** No code path may delete, modify, disable, install or uninstall anything on the user's machine. The only allowed "actions" are: opening a Windows Settings page, opening a folder in Explorer, opening a URL in the browser, and copying a command to the clipboard for the user to run themselves.
5. **Detection is deterministic; AI only explains.** Every finding must be produced by a rule in `kosh-rules` from collected evidence. The AI layer may rephrase, translate and prioritize findings. It may never add a finding, a number, a path or a version that is not in the input.
6. **Privacy by default.** Never read file contents (except the explicitly listed config sources). Never upload anything without the user's opt-in. Strip usernames and personal names from anything sent off the device.
7. **Commit after each milestone** with a clear message, and update `CHANGELOG.md`.
8. **When stuck, stop and ask.** Write what you tried, what failed, and the exact error. Do not silently lower a requirement.
9. **Keep it understandable.** Every crate gets a short `README.md` describing what it does. Prefer simple code over clever code.

---

## 1. What we are building

**Kosh Doctor** is a device health assistant. It scans a computer, finds what is broken, slow, risky or failing, and produces a clear report that tells the user:

- what is wrong, with the evidence,
- how serious it is,
- whether they can fix it themselves, Kosh can help, or they need a technician,
- step-by-step instructions to fix it,
- how to confirm the fix worked.

It runs two ways from one engine:

- **CLI:** `kosh scan`, `kosh report`, `kosh doctor <topic>` (built first).
- **Desktop app:** a Tauri app with a "Scan my PC" button, the report, and a chat panel (built after the CLI is solid).

### 1.1 Non-goals for version 1

- Not an antivirus. It never claims to remove malware. It complements Microsoft Defender.
- No automatic fixes, no driver downloads, no "registry cleaning", no "speed boosters".
- No file deletion or storage cleanup (that is a separate Kosh module, planned later).
- No ads, no scare tactics, no fake problem counts. If everything is fine, say so.

---

## 2. Product principles

| Principle | What it means in code |
|---|---|
| Evidence first | Every finding carries a list of evidence items (source, raw value, timestamp). |
| Calm and honest | Severity is computed by rules, never inflated. "All good" is a valid report. |
| Who can fix it | Every finding has a `fix_level`: `self`, `guided_action`, or `technician`. |
| Verifiable | Every finding has a `verify` section: how a re-scan confirms the problem is gone. |
| Precision over recall | It is better to miss a minor issue than to report a false one. When unsure, mark `confidence: low` and phrase it as "possible". |
| Works offline | The full report works without internet and without AI. AI and online vulnerability data are enhancements. |
| Least privilege | Runs as a normal user. Some checks need Administrator; they are skipped (and listed as "skipped, needs admin") when not elevated. |

---

## 3. Platform scope

| Tier | Platform | Version 1 status |
|---|---|---|
| 1 | Windows 10 and 11, x64 | Full support. All milestones target this first. |
| 2 | macOS 13+ (Apple Silicon and Intel) | Collector adapter in Milestone 9. |
| 2 | Linux (Ubuntu/Debian/Fedora) | Collector adapter in Milestone 10. |
| 3 | Android | Not in this repo. A future companion app with limited data (battery, storage, app list). |
| — | iOS / iPadOS | Not possible. Apple does not allow apps to inspect the system. Do not attempt. |

The architecture must keep platform-specific code inside `kosh-collectors/src/<platform>/` so new platforms do not touch the rules, report or AI layers.

---

## 4. Architecture

```
┌──────────────┐   ┌────────────────┐   ┌───────────────┐   ┌──────────────┐   ┌─────────────┐
│  Collectors  │──▶│   Snapshot     │──▶│  Rules engine │──▶│   Findings   │──▶│   Report    │
│ (per platform)│  │ (one JSON doc) │   │ (deterministic)│  │ (with fixes) │   │ terminal/   │
└──────────────┘   └────────────────┘   └───────────────┘   └──────┬───────┘   │ JSON/HTML   │
                                               ▲                   │           └─────────────┘
                                   ┌───────────┴─────────┐   ┌─────▼────────┐
                                   │ Vulnerability DB     │   │  AI narrator │ (optional, explains only)
                                   │ (local SQLite cache) │   └──────────────┘
                                   └─────────────────────┘
```

**Key idea:** a scan produces a **Snapshot** (pure facts). The rules engine turns a Snapshot into **Findings**. Because Snapshots are plain JSON, they can be saved, shared (anonymized) and replayed in tests without the original computer.

### 4.1 Repository layout

```
kosh-doctor/
├── README.md                  ← this file
├── CHANGELOG.md
├── Cargo.toml                 ← Rust workspace
├── crates/
│   ├── kosh-core/             ← shared types: Snapshot, Finding, Evidence, Severity
│   ├── kosh-collectors/       ← gathers facts
│   │   └── src/{windows,macos,linux}/
│   ├── kosh-rules/            ← detection rules → Findings
│   ├── kosh-vuln/             ← software inventory → known vulnerabilities
│   ├── kosh-report/           ← terminal, JSON and HTML output; health score
│   ├── kosh-ai/               ← optional AI narration and chat (explains findings only)
│   └── kosh-cli/              ← the `kosh` command
├── app/                       ← Tauri 2 + React + TypeScript desktop app (Milestone 8)
├── rules/                     ← rule metadata in TOML (titles, fix steps, verify steps, languages)
├── data/
│   └── os_lifecycle.toml      ← OS versions and their support end dates
├── tests/
│   ├── fixtures/              ← saved anonymized Snapshots with expected findings
│   └── lab/                   ← fault-injection lab scripts and expected results
└── docs/
    ├── VERIFIED_SOURCES.md    ← every system fact verified on a real machine
    ├── CHECK_CATALOG.md       ← generated list of all checks
    └── PRIVACY.md
```

### 4.2 Technology choices

| Need | Choice | Notes |
|---|---|---|
| Language | Rust (stable, 2021 edition or newer) | Safe and fast; good Windows support. |
| Windows APIs | `windows` crate (windows-rs), `wmi` crate, `winreg` crate | Prefer native APIs. PowerShell fallback is allowed only where noted, and must be marked in code with `// FALLBACK_POWERSHELL`. |
| Cross-platform process/memory | `sysinfo` crate | |
| CLI | `clap` (derive) | |
| Serialization | `serde`, `serde_json`, `toml` | |
| Local DB | `rusqlite` (bundled) | Vulnerability cache, scan history. |
| HTTP | `reqwest` (rustls) | Only for vulnerability feeds and optional AI. |
| HTML report | `minijinja` templates, single self-contained HTML file | No external assets. |
| Tests | built-in tests + `insta` snapshot tests | |
| Desktop app | Tauri 2, React, TypeScript, Tailwind | Milestone 8. |
| Optional deep data | `smartctl` (smartmontools), osquery | Detect if installed; never required. |

---

## 5. Data model

Define these in `kosh-core`. Everything else depends on them.

```rust
pub struct Snapshot {
    pub schema_version: u32,          // start at 1
    pub scan_id: String,              // UUID
    pub taken_at: DateTime<Utc>,
    pub elevated: bool,               // was the scan run as Administrator?
    pub platform: PlatformInfo,       // OS name, version, build, edition, architecture
    pub hardware: HardwareInfo,       // CPU, RAM sticks/slots, disks, battery, GPU
    pub performance: PerformanceInfo, // memory/CPU/disk usage, top processes
    pub storage: StorageInfo,         // volumes, free space
    pub devices: Vec<DeviceInfo>,     // PnP devices and problem codes, drivers
    pub events: EventSummary,         // counted and sampled log events (last 30 days)
    pub security: SecurityInfo,       // AV, firewall, updates, BitLocker, Secure Boot, TPM, SMBv1...
    pub software: Vec<InstalledApp>,  // name, publisher, version, install date, source
    pub startup: Vec<StartupItem>,    // Run keys, startup folders, tasks, auto services
    pub network: NetworkInfo,         // adapters, status, basic connectivity
    pub skipped: Vec<SkippedCheck>,   // collector + reason (e.g. "needs admin", "not supported")
    pub errors: Vec<CollectorError>,  // collectors that failed, with message
}

pub struct Finding {
    pub id: String,                   // stable rule id, e.g. "DEV-001"
    pub category: Category,           // Performance | Device | Stability | Security | Vulnerability | Hardware | Storage | Network
    pub severity: Severity,           // Critical | High | Medium | Low | Info
    pub confidence: Confidence,       // High | Medium | Low
    pub title: String,                // short, plain language
    pub summary: String,              // 1–2 sentences, plain language
    pub evidence: Vec<Evidence>,      // source + raw value + timestamp
    pub fix_level: FixLevel,          // SelfFix | GuidedAction | Technician
    pub fix_steps: Vec<String>,       // numbered plain-language steps
    pub actions: Vec<SafeAction>,     // only: OpenSettings(uri) | OpenFolder(path) | OpenUrl(url) | CopyCommand(cmd)
    pub verify: String,               // how a re-scan confirms it's fixed
    pub references: Vec<String>,      // official docs URLs (Microsoft, NVD, vendor)
}

pub struct Evidence {
    pub source: String,               // e.g. "WMI Win32_PnPEntity", "EventLog System 1001"
    pub key: String,                  // e.g. "ConfigManagerErrorCode"
    pub value: String,                // raw value as collected
    pub observed_at: Option<DateTime<Utc>>,
}
```

**Rule:** `SafeAction` is a closed enum. Adding a variant that changes the system is forbidden in version 1.

---

## 6. Collectors (Windows, Tier 1)

Each collector is a function `fn collect(ctx: &Ctx) -> Result<Part, CollectorError>`. A failing collector must never stop the scan; it records an error and the scan continues. Each collector has a timeout (default 15 s).

Verify every source below on a real machine before relying on it (Rule 3).

| Area | What to collect | Primary source | Notes |
|---|---|---|---|
| OS | Name, version, build, edition, install date, last boot, uptime | WMI `Win32_OperatingSystem`, registry `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` (`DisplayVersion`, `CurrentBuild`, `UBR`) | Compare against `data/os_lifecycle.toml`. |
| System | Manufacturer, model, BIOS version/date | `Win32_ComputerSystem`, `Win32_BIOS` | |
| CPU | Model, cores, threads, current load | `Win32_Processor`, `sysinfo` | |
| RAM | Total, available, committed, each stick (size, speed, type), slot count, max supported | `sysinfo`, `Win32_PhysicalMemory`, `Win32_PhysicalMemoryArray` (`MemoryDevices`, `MaxCapacity`) | `MaxCapacity` is often wrong and soldered RAM is invisible. Always mark upgrade advice as "confirm with your laptop's manual". |
| Memory pressure | Available MB, pages/sec, commit % sampled for N seconds | Performance counters (PDH): `\Memory\Available MBytes`, `\Memory\Pages/sec`, `\Memory\% Committed Bytes In Use` | Default sample: 30 s at 1 s intervals. `kosh monitor` extends this (Milestone 7). |
| Processes | Top 15 by memory and by CPU, grouped by app name | `sysinfo` | Group child processes (e.g. many `chrome.exe`) under one app. |
| Disks | Model, media type (SSD/HDD), bus type, size, health status, wear, temperature, read errors | `root\Microsoft\Windows\Storage` → `MSFT_PhysicalDisk` (`MediaType`, `BusType`, `HealthStatus`), `MSFT_StorageReliabilityCounter` (needs admin) | If `smartctl` is installed, also run `smartctl -a -j` and store raw attributes. |
| Volumes | Drive letters, total, free, file system, BitLocker state | `Win32_LogicalDisk`, `Win32_EncryptableVolume` (`root\CIMV2\Security\MicrosoftVolumeEncryption`, needs admin) | |
| Battery | Design capacity, full charge capacity, cycle count if available | `root\wmi` → `BatteryStaticData.DesignedCapacity`, `BatteryFullChargedCapacity.FullChargedCapacity`, `BatteryCycleCount` | Desktops have no battery: record "not present", not an error. |
| GPU | Name, driver version, driver date | `Win32_VideoController` | |
| Devices | Every PnP device with status and problem code | `Win32_PnPEntity` (`ConfigManagerErrorCode`, `Status`, `PNPClass`, `Name`) | Map problem codes to plain language in `rules/device_codes.toml` (e.g. 10, 22, 28, 31, 43, 45). |
| Drivers | Name, version, date, provider, signed | `Win32_PnPSignedDriver` | |
| Event logs (last 30 days) | Counts + up to 5 samples per event type | Windows Event Log API (`EvtQuery`); fallback `Get-WinEvent` | See Section 6.1. |
| Reliability | Stability index and records | `Win32_ReliabilityStabilityMetrics`, `Win32_ReliabilityRecords` | May be unavailable; record as skipped. |
| Crash dumps | Count and dates of files in `C:\Windows\Minidump` | File listing only | Do not parse dumps in v1. Bugcheck codes come from event 1001. |
| Pending reboot | Pending flags | Registry: `...\Component Based Servicing\RebootPending`, `...\WindowsUpdate\Auto Update\RebootRequired`, `Session Manager\PendingFileRenameOperations` | |
| Updates | Installed hotfixes and dates, last successful update | `Win32_QuickFixEngineering`; Windows Update Agent API (`Microsoft.Update.Session`) via COM | |
| Antivirus | Products registered, enabled, up to date | `root\SecurityCenter2` → `AntiVirusProduct` (client Windows only); Defender: `root\Microsoft\Windows\Defender` → `MSFT_MpComputerStatus` | |
| Firewall | Profile states | `root\StandardCimv2` → `MSFT_NetFirewallProfile` | |
| Secure Boot / TPM | Enabled, version | Registry `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State\UEFISecureBootEnabled`; `Win32_Tpm` (`root\CIMV2\Security\MicrosoftTpm`, needs admin) | |
| Risky settings | SMBv1 enabled, RDP enabled, UAC disabled, guest account enabled | Optional feature `SMB1Protocol` state, registry `fDenyTSConnections`, `EnableLUA` | |
| Installed software | Name, version, publisher, install date, install location | Registry Uninstall keys: `HKLM\...\Uninstall`, `HKLM\SOFTWARE\WOW6432Node\...\Uninstall`, `HKCU\...\Uninstall`; AppX packages | Skip entries with `SystemComponent=1` or no `DisplayName`. |
| Startup | Name, command, location, publisher, signed, enabled | `HKLM`/`HKCU` `...\CurrentVersion\Run` and `RunOnce`; Startup folders; `StartupApproved` keys (to know if disabled); scheduled tasks with logon/boot triggers; auto-start non-Microsoft services | Check signature with `WinVerifyTrust`. |
| Network | Adapters, status, link speed, IP config present, DNS reachable, internet reachable | `MSFT_NetAdapter`, simple DNS lookup + HTTPS HEAD to a well-known host | Connectivity test is skipped in `--offline` mode. |

### 6.1 Event log queries

Collect counts and up to 5 most recent samples (time, provider, ID, short message) for:

| Meaning | Log | Provider | Event ID(s) |
|---|---|---|---|
| Unexpected shutdown / power loss | System | Microsoft-Windows-Kernel-Power | 41 |
| Previous shutdown was unexpected | System | EventLog | 6008 |
| Blue screen (bugcheck) | System | BugCheck / Microsoft-Windows-WER-SystemErrorReporting | 1001 |
| Hardware error | System | Microsoft-Windows-WHEA-Logger | 17, 18, 19, 47 |
| Disk bad block / I/O retries | System | disk | 7, 153 |
| File system corruption | System | Ntfs | 55 |
| Service failed to start / crashed | System | Service Control Manager | 7000, 7001, 7023, 7031, 7034 |
| App crash / hang | Application | Application Error / Application Hang | 1000, 1002 |
| Low memory (resource exhaustion) | System | Microsoft-Windows-Resource-Exhaustion-Detector | 2004 |
| Windows Update install failure | System | Microsoft-Windows-WindowsUpdateClient | 20 |
| Driver/Device install problems | System | Microsoft-Windows-UserPnp / Kernel-PnP | verify on machine, record in VERIFIED_SOURCES |

Verify each provider name and ID on a real machine; some differ between Windows versions.

### 6.2 macOS and Linux collectors (Milestones 9–10)

Same Snapshot schema. Suggested sources:

- **macOS:** `system_profiler -json` (hardware, battery, storage), `sw_vers`, `softwareupdate -l`, `fdesetup status`, `spctl --status`, `csrutil status`, `/usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate`, `log show` (bounded queries), `/Library/Logs/DiagnosticReports` (crash file listing), LaunchAgents/LaunchDaemons folders, `/Applications` bundle versions, `smartctl` if installed.
- **Linux:** `/etc/os-release`, `/proc/meminfo`, `/proc/pressure/memory` (PSI), `sysinfo`, `lsblk -J`, `smartctl -a -j`, `journalctl -p err --since "30 days ago" -o json` (bounded), `systemctl --failed`, `upower -d`, `dpkg-query`/`rpm -qa`, `ufw status` / `firewall-cmd --state`, `lspci -k` (driver in use).

---

## 7. Rules engine

Rules live in `kosh-rules`. Each rule is:

- **Detection logic** in Rust: a pure function `fn check(snapshot: &Snapshot) -> Vec<Finding>`. Pure means no system calls, so it is fully testable with fixture Snapshots.
- **Text and fix content** in `rules/<rule-id>.toml`: title, summary template, fix steps, verify text, references, translations (`en`, `hi`, `hinglish`).

### 7.1 Initial check catalog (version 1)

Thresholds are starting values; tune them with lab and field data. Every threshold lives in a config file, not hard-coded.

| ID | Check | Detection (simplified) | Severity | Fix level |
|---|---|---|---|---|
| PERF-001 | RAM regularly full | Available memory < 10% for > 50% of samples, or commit > 90% | Medium | SelfFix (close/limit apps) + Hardware advice |
| PERF-002 | Memory-heavy app | One app group > 30% of total RAM | Low | SelfFix |
| PERF-003 | Heavy paging | Pages/sec sustained above threshold while memory is low | Medium | SelfFix |
| PERF-004 | Too many startup apps | > 10 enabled non-Microsoft startup items | Low | SelfFix (Settings → Apps → Startup) |
| PERF-005 | Long uptime | Uptime > 14 days | Info | SelfFix (restart) |
| PERF-006 | Low-memory events | Event 2004 in last 30 days | Medium | SelfFix |
| STOR-001 | System drive almost full | C: free < 10% or < 15 GB | High | SelfFix (point to Storage settings / Kosh storage module later) |
| STOR-002 | Windows on a hard disk (HDD) | System volume on `MediaType = HDD` | Medium | Hardware advice (SSD upgrade) |
| DISK-001 | Disk health warning | `HealthStatus` not Healthy, or SMART failing attribute | Critical | Technician + "back up now" |
| DISK-002 | SSD wear high | Wear ≥ 80% | High | Back up + plan replacement |
| DISK-003 | Disk errors in logs | disk 7/153 or Ntfs 55 events | High | Back up + run `chkdsk` (copy command) + technician if repeated |
| DEV-001 | Device not working | `ConfigManagerErrorCode` ≠ 0 and ≠ 22 | High (network, display, storage, audio) / Medium (others) | SelfFix guided per code |
| DEV-002 | Device disabled | Code 22 | Info | SelfFix (if user wants it enabled) |
| DEV-003 | Missing driver | Code 28 | Medium | SelfFix: Windows Update → Optional updates, or manufacturer support page |
| DEV-004 | Very old display driver | GPU driver date > 2 years | Low | SelfFix: manufacturer site or Windows Update |
| STAB-001 | Blue screens | ≥ 1 event 1001 bugcheck in 30 days | High (≥ 3: Critical) | Guided; include bugcheck code meaning; Technician if repeated |
| STAB-002 | Unexpected shutdowns | Kernel-Power 41 without matching user-initiated shutdown | Medium | Guided (power, overheating, battery) |
| STAB-003 | Hardware errors | WHEA events present | High | Technician |
| STAB-004 | Repeated app crashes | Same app crashes ≥ 3 times | Medium | SelfFix (update/reinstall app) |
| STAB-005 | Services failing | SCM 7000/7023/7031/7034 for the same service ≥ 3 times | Medium | Guided |
| STAB-006 | Pending reboot | Any pending-reboot flag | Low | SelfFix (restart) |
| SEC-001 | Antivirus off or outdated | No enabled AV, or Defender signatures > 7 days old | Critical | SelfFix (Windows Security) |
| SEC-002 | Firewall off | Any profile disabled | High | SelfFix |
| SEC-003 | Updates stale | No successful update in > 45 days, or update failures (event 20) | High | SelfFix (Windows Update) |
| SEC-004 | OS out of support | OS version past support end in `os_lifecycle.toml` | Critical | Guided (upgrade path / hardware check) |
| SEC-005 | SMBv1 enabled | Feature enabled | High | Guided (copy command, needs admin) |
| SEC-006 | UAC disabled | `EnableLUA = 0` | High | SelfFix |
| SEC-007 | Disk not encrypted (laptop) | Laptop and BitLocker/Device Encryption off | Medium | Guided (only where the edition supports it) |
| SEC-008 | Secure Boot off | `UEFISecureBootEnabled = 0` | Low | Technician (BIOS setting) |
| SEC-009 | Suspicious startup item | Unsigned startup executable in `Downloads`, `Temp` or `AppData\Local\Temp`, or a double extension like `.pdf.exe` | High | Guided: scan with Defender, then disable startup entry |
| SEC-010 | Plain-text secret files | File *names* like `passwords.txt`, `*.pem`, `.env` on Desktop/Documents/Downloads | Medium | SelfFix (password manager) — names only in v1, no content scanning |
| VULN-001 | App with known exploited vulnerability | Installed version matches a CISA KEV entry | Critical | SelfFix (update link) |
| VULN-002 | App with known vulnerability | Installed version matches a CVE with CVSS ≥ 7 | High | SelfFix |
| VULN-003 | App outdated | Newer version known, no CVE match | Low | SelfFix |
| HW-001 | RAM upgrade recommended | PERF-001 true AND total RAM ≤ 8 GB AND free slot or higher max capacity detected | Info | Hardware advice |
| HW-002 | Battery worn | Full charge < 60% of design capacity | Medium | Technician (battery replacement) |
| HW-003 | SSD upgrade recommended | STOR-002 true | Info | Hardware advice |
| NET-001 | Network adapter problem | Adapter present but `Status` error, or no adapter up | High | Guided (troubleshooter, driver reinstall steps) |
| NET-002 | DNS or internet unreachable | Adapter up but DNS/HTTPS test fails | Medium | Guided |

Generate `docs/CHECK_CATALOG.md` from the rule files with a small build script so documentation never drifts from code.

### 7.2 Hardware advice rules

- Advice only when the evidence supports a real benefit. Never recommend hardware "just because".
- Always state the evidence ("RAM was above 90% for 22 of 30 samples").
- Always add "confirm compatibility in your laptop's manual or with a technician".
- Price ranges are optional, live in a config file, are shown in local currency, and include "approximate" with the date they were last updated.
- No affiliate links, no specific brands in version 1.

### 7.3 Health score

Start at 100, subtract per finding: Critical 25, High 12, Medium 5, Low 2, Info 0, with a floor of 0. Map to labels: 85–100 Good, 65–84 Fair, 40–64 Needs attention, < 40 Poor. Keep the formula in one place with tests, and show it in the report ("How is this score calculated?").

---

## 8. Vulnerability pipeline (`kosh-vuln`)

Matching installed Windows software to vulnerabilities is the hardest and most error-prone part. **Precision beats coverage.**

1. **Inventory:** from the software collector (name, publisher, version).
2. **Normalize:** map registry display names to a canonical product using a curated table `data/product_map.toml` (e.g. `"7-Zip 23.01 (x64)"` → vendor `7-zip`, product `7-zip`, version `23.01`). Start with the 50 most common Windows apps (archive tools, browsers, PDF readers, media players, chat apps, remote-desktop tools, runtimes like Java/Python/Node, Git, Notepad++, Zoom, OBS). Unknown apps are skipped, not guessed.
3. **Vulnerability data:** a separate updater command `kosh vuln update` downloads and caches into local SQLite:
   - NVD CVE API 2.0 (`https://services.nvd.nist.gov/rest/json/cves/2.0`), queried per product CPE. Respect rate limits; support an optional NVD API key from the `NVD_API_KEY` environment variable.
   - CISA Known Exploited Vulnerabilities catalog (JSON feed published by CISA).
   - Optional: EPSS scores from FIRST (`https://api.first.org/data/v1/epss`) for prioritization.
4. **Match:** compare installed version against CPE version ranges (`versionStartIncluding`, `versionEndExcluding`, etc.). Write thorough version-comparison tests (e.g. `19.00` vs `19.0`, `1.10` vs `1.9`, builds with four parts).
5. **Report:** say "known vulnerable" only on a confirmed range match. Say "outdated" when only a newer version is known. Never print a CVE that was not matched by range.
6. **Offline:** if the cache is missing or older than 14 days, the report says so and skips VULN checks rather than guessing.

---

## 9. AI layer (`kosh-ai`) — optional

The report must be complete without AI. AI adds: plain-language narration, Hindi/Hinglish output, prioritization advice, and a chat that answers questions **about the findings**.

### 9.1 Input and output

- **Input:** the Findings list (JSON) plus a minimal, anonymized Snapshot summary (OS, hardware class, RAM size, disk type). Replace the Windows username and any personal folder names with `<user>`. Never send installed app lists or file names without opt-in.
- **Output:** JSON with `overview` (≤ 120 words), `top_actions` (≤ 5 items, each referencing a finding `id`), and `per_finding` explanations keyed by `id`.

### 9.2 Guardrails (enforced in code, not only in the prompt)

- Parse the AI output as JSON. Reject and fall back to templates if it references an unknown finding ID, or contains numbers, versions or paths that do not appear in the input.
- The AI cannot create findings, change severity, or add actions.
- Timeout 30 s; on failure, use the template report silently.

### 9.3 System prompt (starting point)

```
You are Kosh, a calm and honest PC health assistant. You receive a JSON list of findings
produced by deterministic checks. Explain them to a non-technical person.
Rules:
- Use ONLY facts present in the input. Never add new problems, numbers, versions, or paths.
- Refer to findings by their id.
- Be calm. No fear words. If there are no serious findings, say the computer is in good shape.
- For each finding say: what it means, why it matters, who can fix it, and the first step.
- Respond in the requested language: en, hi, or hinglish.
- Output valid JSON matching the schema provided. No other text.
```

### 9.4 Provider configuration

- Provider: Anthropic Claude API via the Messages endpoint. Model ID read from the `KOSH_AI_MODEL` environment variable with a sensible default (the owner can check current model IDs at https://docs.claude.com). API key from `ANTHROPIC_API_KEY`.
- For development, the owner's own API key is fine. **Before public release**, never ship an API key inside the app: route calls through a small backend proxy (Milestone 11) that holds the key, rate-limits, and strips personal data again.
- Design an `AiProvider` trait so a local model (llama.cpp/Ollama) can be added later for offline/private mode.

---

## 10. CLI specification (`kosh`)

| Command | What it does |
|---|---|
| `kosh scan` | Full scan; prints a short terminal summary; saves Snapshot + Findings to the history folder. |
| `kosh scan --json` | Prints Findings as JSON (for scripts and IT tools). |
| `kosh report [--html PATH] [--lang en\|hi\|hinglish] [--ai]` | Builds a report from the latest scan. HTML is one self-contained file. `--ai` adds narration if configured. |
| `kosh doctor <topic>` | Focused scan and guidance. Topics: `wifi`, `sound`, `display`, `slow`, `boot`, `battery`, `disk`, `updates`, `security`. |
| `kosh memory` | Live view of memory use grouped by app for 30 s. |
| `kosh monitor --hours N` | Samples performance in the background (low overhead) to support PERF and HW rules with real history. |
| `kosh vuln update` | Refreshes the local vulnerability cache. |
| `kosh history` | Lists previous scans and score changes. |
| `kosh verify <finding-id>` | Re-runs only the collectors and rule needed for that finding and says whether it is fixed. |
| `kosh snapshot export --anonymize PATH` | Exports an anonymized Snapshot for support or test fixtures. |

Global flags: `--offline`, `--verbose`, `--no-color`. Exit codes: `0` no High/Critical findings, `1` High/Critical present, `2` scan error.

Storage location for history and cache: `%LOCALAPPDATA%\Kosh` on Windows, platform equivalents elsewhere (use the `directories` crate).

---

## 11. Report specification

**Terminal summary:** score and label, then findings grouped as "Fix now" (Critical/High), "Worth doing" (Medium/Low), "Healthy" (one line per area with no findings), then "Skipped checks" with reasons.

**HTML report sections:**

1. Header: device model, OS, scan date, score with the explanation link.
2. Fix now / Worth doing / Healthy (cards with evidence, fix steps, who can fix it, verify step).
3. Hardware overview (CPU, RAM sticks and slots, disks with type and health, battery).
4. Security overview.
5. Software with known vulnerabilities.
6. Skipped checks and collector errors (honesty section).
7. Footer: "Kosh did not change anything on your computer."

Must look good on light and dark themes, print cleanly to PDF from the browser, and work with no internet.

---

## 12. Privacy and security of Kosh itself

- Runs as a normal user; the user may re-run elevated for the extra checks. Never auto-elevate repeatedly.
- No telemetry by default. If added later: opt-in, documented field by field in `docs/PRIVACY.md`.
- Anonymize before any export or network call: username, computer name, personal folder names, Wi-Fi network names, IP/MAC addresses.
- No file content reading in v1 (SEC-010 uses file names only).
- Dependencies: pin versions, run `cargo audit` in CI, avoid unmaintained crates.
- Release binaries must be code-signed (owner provides the certificate, Milestone 11).

---

## 13. Testing and validation

### 13.1 Unit and fixture tests

- Every rule has tests with hand-written Snapshot fixtures: one that triggers it, one that does not, and one edge case.
- `tests/fixtures/*.json`: anonymized real Snapshots with an `expected_findings` list. A test runs the rules on each and compares (use `insta`).
- Version comparison and CPE matching get their own large test tables.

### 13.2 Fault-injection lab (`tests/lab/`)

Use Hyper-V or VirtualBox Windows VMs with a clean saved snapshot. Each lab case is a script that plants one problem, followed by an expected finding ID. Run `kosh scan --json`, compare, restore the VM. Starting cases:

| Lab case | How to plant it | Expected |
|---|---|---|
| Disabled device | Disable the network adapter in Device Manager | DEV-002 |
| Firewall off | Turn off the firewall for the private profile | SEC-002 |
| SMBv1 on | Enable the SMB1Protocol optional feature | SEC-005 |
| Vulnerable app | Install an old 7-Zip or Notepad++ version known in NVD | VULN-002 (or VULN-001 if in KEV) |
| Startup in Downloads | Add an unsigned test exe to the Run key from Downloads | SEC-009 |
| Disk nearly full | Create a large file until C: free < 10% | STOR-001 |
| Memory pressure | Run a memory-hog test program during the scan | PERF-001 |
| App crash | Run a test program that crashes 3 times | STAB-004 |
| Pending reboot | Install an update without restarting, or set a test registry flag | STAB-006 |
| UAC off | Set `EnableLUA = 0` (lab only) | SEC-006 |
| Clean machine | Nothing planted | No High/Critical findings |

Track **recall** (planted problems found) and **false positives** (findings on the clean machine). Targets for v1 release: recall ≥ 90% on lab cases, zero High/Critical false positives on the clean VM and on 20 real volunteer machines as judged by a technician.

### 13.3 Comparison with reference tools

On at least 5 real machines, compare: startup items vs Sysinternals Autoruns, disk health vs CrystalDiskInfo or `smartctl`, device problems vs Device Manager, AV/firewall vs Windows Security. Record differences and explanations in `docs/VALIDATION.md`.

### 13.4 Fix verification

After a user follows a fix, `kosh verify <id>` must show the problem gone. Field metric: share of SelfFix findings verified fixed within 7 days.

---

## 14. Performance budgets

- Full scan (non-elevated) under 60 s on a 4-core laptop with an SSD; under 120 s on HDD machines.
- CPU use during scan below 50% average; memory below 300 MB.
- `kosh monitor` overhead below 1% CPU and 50 MB RAM.

---

## 15. Coding conventions

- No `unwrap()`/`expect()` in non-test code; use `anyhow` in binaries and `thiserror` in libraries.
- All user-facing text comes from `rules/*.toml` or report templates, never scattered string literals, so translation works.
- Log with `tracing`; logs must not contain usernames or file contents.
- Windows-specific code behind `#[cfg(windows)]`; the workspace must still compile on macOS/Linux with stub collectors.

---

## 16. Milestones (build in this order)

Each milestone ends with: tests green, `CHANGELOG.md` updated, a short note to the owner explaining what now works and how to try it.

| # | Milestone | Acceptance checks |
|---|---|---|
| **M0** | Workspace setup: crates, CI (GitHub Actions on Windows + Linux), formatting, linting, `kosh --version` | `cargo test` passes on CI; owner can run `kosh --version`. |
| **M1** | Core types + collectors: OS, system, CPU, RAM, disks, volumes, battery, processes. `kosh scan --json` prints a Snapshot | Snapshot validates against schema; values match Task Manager / Settings on the owner's PC (owner confirms). |
| **M2** | Rules engine + first 10 rules (PERF, STOR, HW-001/002/003, STAB-006) + terminal report + health score | Fixture tests for each rule; terminal output reviewed by owner. |
| **M3** | Devices, drivers, event logs, reliability, crash dumps + DEV and STAB rules | Lab cases: disabled device, app crash, pending reboot pass. |
| **M4** | Security collectors + SEC rules + OS lifecycle data | Lab cases: firewall off, SMBv1, UAC, startup in Downloads pass. Matches Windows Security app. |
| **M5** | Vulnerability pipeline: product map (top 50 apps), NVD + KEV cache, version matching, VULN rules | Vulnerable-app lab case passes; zero false matches on the clean VM. |
| **M6** | HTML report, history, `kosh verify`, `kosh doctor` topics, anonymized export | HTML opens offline, prints to PDF, works in dark mode; verify detects fixed vs not fixed. |
| **M7** | `kosh monitor` + history-based PERF/HW rules | 24 h run within overhead budget; RAM advice uses history when available. |
| **M8** | AI narration (en/hi/hinglish) with guardrails + Tauri desktop app (Scan button, report view, chat about findings) | Guardrail tests reject hallucinated IDs/numbers; app works fully with AI turned off. |
| **M9** | macOS collectors | Same Snapshot schema; rules that apply produce correct findings on a real Mac. |
| **M10** | Linux collectors | Same, on Ubuntu. |
| **M11** | Release: code signing, installer (MSI/MSIX), auto-update, AI backend proxy, privacy policy page, winget manifest | Signed installer installs on a clean Windows VM without "unknown publisher" warning; no API key inside the binary. |
| **M12** | Field validation: 20 volunteer machines + technician review, metrics in `docs/VALIDATION.md` | Targets in Section 13.2 met, or gaps documented with a plan. |

**Later (not in this repo's v1):** one-click fixes with restore points and per-action confirmation; Kosh storage cleanup module; family/IT fleet dashboard; Android companion.

---

## 17. Definition of done for version 1

- A non-technical user can run one command or click one button and receive an honest report within 2 minutes.
- Every finding shows evidence, a fix, who can fix it, and how to verify.
- Nothing on the computer was changed by Kosh.
- Lab recall ≥ 90%, zero serious false positives in field validation.
- Works offline; AI and online data only improve it.
