# Verified sources

Every Windows fact Kosh relies on, checked on a real machine (README Rule 3).
Add a row whenever a new source is used. If a later Windows version behaves differently, add a new row instead of editing the old one.

**Reference machine A:** Lenovo 83GS laptop, Windows 11 Home Single Language, version 25H2, build 26200.9711, x64, not elevated. Verified 2026-10-20.

## OS and system

| Source | Verified behaviour |
|---|---|
| WMI `Win32_OperatingSystem` | `Caption`, `Version` (`10.0.26200`), `BuildNumber`, `OSArchitecture`, `InstallDate`, `LastBootUpTime` all readable without admin. |
| Registry `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` | `DisplayVersion` = `25H2`, `CurrentBuild` = `26200`, `UBR` = `9711`, `EditionID` = `CoreSingleLanguage`. **`ProductName` still says "Windows 10 ..." on Windows 11.** Kosh decides Windows 10 vs 11 from the build number (>= 22000 is Windows 11), never from `ProductName`. |
| WMI `Win32_ComputerSystem` | `Manufacturer`, `Model`, `PCSystemType` (2 = mobile/laptop), `TotalPhysicalMemory` readable. |
| WMI `Win32_BIOS` | `SMBIOSBIOSVersion`, `ReleaseDate` readable. |
| WMI `Win32_Processor` | `Name`, `NumberOfCores`, `NumberOfLogicalProcessors` readable. |

## Memory

| Source | Verified behaviour |
|---|---|
| WMI `Win32_PhysicalMemory` | One row per stick: `Capacity`, `Speed`, `SMBIOSMemoryType` (34 = DDR5), `DeviceLocator`. |
| WMI `Win32_PhysicalMemoryArray` | `MemoryDevices` = 2, `MaxCapacity` = 67108864 (KB, i.e. 64 GB). As the README warns, treat as a hint only. |

## Storage

| Source | Verified behaviour |
|---|---|
| WMI `root\Microsoft\Windows\Storage` `MSFT_PhysicalDisk` | Readable without admin. `MediaType` is numeric: 3 = HDD, 4 = SSD, 5 = SCM, 0 = unspecified. `BusType` numeric (17 = NVMe, 11 = SATA, 7 = USB). `HealthStatus` numeric: 0 = Healthy, 1 = Warning, 2 = Unhealthy, 5 = Unknown. |
| PowerShell `Get-Partition` (`MSFT_Partition`) | Maps drive letters to `DiskNumber`; used to find which physical disk holds C:. |
| WMI `Win32_LogicalDisk` | `DeviceID`, `DriveType` (3 = local disk), `FileSystem`, `Size`, `FreeSpace`. |
| `Win32_EncryptableVolume` | Needs admin (reported as skipped when not elevated). |

## Battery

| Source | Verified behaviour |
|---|---|
| WMI `root\wmi` `BatteryStaticData` | **Fails with "Generic failure" (0x80041001) when not elevated.** Kosh therefore reads design capacity through the battery device IOCTL (`IOCTL_BATTERY_QUERY_INFORMATION`), which works as a normal user. |
| WMI `root\wmi` `BatteryFullChargedCapacity` | Readable: 53890 mWh. |
| WMI `root\wmi` `BatteryCycleCount` | Returned no data on machine A (treated as "not available"). |
| WMI `Win32_Battery` | `Name`, `EstimatedChargeRemaining` readable. |

## GPU and devices

| Source | Verified behaviour |
|---|---|
| WMI `Win32_VideoController` | Two GPUs (Intel UHD, NVIDIA RTX 3050) with `DriverVersion` and `DriverDate`. |
| WMI `Win32_PnPEntity` | `ConfigManagerErrorCode` readable; no problem devices on machine A. |
| Event providers `Microsoft-Windows-UserPnp`, `Microsoft-Windows-Kernel-PnP` | Both exist on machine A. |

## Security

| Source | Verified behaviour |
|---|---|
| WMI `root\SecurityCenter2` `AntiVirusProduct` | Lists `Windows Defender` (productState 397568 = 0x61100) and `McAfee` (3424256 = 0x344000). Bit 0x1000 set = real-time protection on; bit 0x10 set = signatures out of date. Defender: on, up to date. McAfee: registered but not active. |
| WMI `root\Microsoft\Windows\Defender` `MSFT_MpComputerStatus` | `AntivirusEnabled`, `RealTimeProtectionEnabled`, `AntivirusSignatureLastUpdated`, `AntivirusSignatureAge` readable without admin. |
| WMI `root\StandardCimv2` `MSFT_NetFirewallProfile` | Domain/Private/Public with `Enabled` = 1. |
| Registry `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State\UEFISecureBootEnabled` | 1. |
| WMI `root\CIMV2\Security\MicrosoftTpm` `Win32_Tpm` | **Access denied when not elevated** (skipped, needs admin). |
| Registry `...\Policies\System\EnableLUA` | 1. |
| Registry `...\Control\Terminal Server\fDenyTSConnections` | 1 (RDP off). |
| WMI `Win32_OptionalFeature` where `Name='SMB1Protocol'` | Readable without admin; `InstallState` 1 = enabled, 2 = disabled. |
| Pending reboot keys (`Component Based Servicing\RebootPending`, `WindowsUpdate\Auto Update\RebootRequired`) | Absent = no pending reboot. |
| WMI `Win32_QuickFixEngineering` | `HotFixID`, `InstalledOn` readable. |

## Network

| Source | Verified behaviour |
|---|---|
| WMI `root\StandardCimv2` `MSFT_NetAdapter` | `Name`, `InterfaceDescription`, `MediaConnectState` (1 = connected, 2 = disconnected), `InterfaceOperationalStatus` (1 = up, 2 = down), `HardwareInterface`. |

## Event logs (last 30 days)

**Important finding:** event IDs are reused by many providers. Counting by ID alone gave large false counts on machine A:

| Query by ID only | Count | Real provider(s) |
|---|---|---|
| System 55 | 108 | `Microsoft-Windows-Kernel-Processor-Power` (not NTFS!) |
| System 17/18/19/47 | 391 | BTHUSB, Kernel-Boot, WindowsUpdateClient, TPM (not WHEA!) |
| System 7/153 | 9 | Kernel-Boot (not disk!) |

**Kosh always filters by provider name and event ID together.** With the provider filter on machine A:

| Log / Provider / ID | Count |
|---|---|
| System / Microsoft-Windows-Kernel-Power / 41 | 0 |
| System / EventLog / 6008 | 0 |
| System / Microsoft-Windows-WER-SystemErrorReporting / 1001 | 0 |
| System / BugCheck / 1001 | 0 (provider name accepted) |
| System / Microsoft-Windows-WHEA-Logger / 17,18,19,47 | 0 |
| System / disk / 7,153 | 0 |
| System / Ntfs and Microsoft-Windows-Ntfs / 55 | 0 |
| System / Service Control Manager / 7000,7001,7023,7031,7034 | 3. Service name is event data item 0 (e.g. "Phone Service"). |
| Application / Application Error / 1000 | 1. Faulting app name is event data item 0 (e.g. `UDClientService.exe`). |
| Application / Application Hang / 1002 | 2 |
| System / Microsoft-Windows-Resource-Exhaustion-Detector / 2004 | 0 |
| System / Microsoft-Windows-WindowsUpdateClient / 20 | 25. **Most were Microsoft Store app updates failing with 0x80073D02 (app was running), not Windows updates.** Kosh only counts failures whose update title contains a `KB` number as OS update failures. |
| System / Microsoft-Windows-WindowsUpdateClient / 19 | 73 (successful installs) |

## Startup

| Source | Verified behaviour |
|---|---|
| `HKCU`/`HKLM` `...\Explorer\StartupApproved\Run` | Binary values; first byte observed as 3 (OneDrive) and 1 (BraveSoftware Update) for items disabled by the user; 2/6 mean enabled. Kosh treats odd = disabled, even or missing = enabled. |
| Task Scheduler COM (`ITaskService`) | Enumerates non-Microsoft tasks with logon/boot triggers without admin (NVIDIA, Lenovo, McAfee, OneDrive tasks on machine A). |
| `WinVerifyTrust` | Works without admin; Windows-folder files are catalog-signed, so Kosh only checks files outside `%SystemRoot%`. |
| Windows Update Agent COM (`IUpdateSearcher::QueryHistory`) | Works without admin. History contains daily Defender definition updates (KB2267602), so Kosh ignores those when finding the last Windows update. |
| Battery `IOCTL_BATTERY_QUERY_INFORMATION` | Works without admin: DesignedCapacity 60000, FullChargedCapacity 53890, CycleCount 550. |

## Other files and folders

| Source | Verified behaviour |
|---|---|
| `C:\Windows\Minidump` | Exists; file listing readable. |
| `Win32_ReliabilityStabilityMetrics` | Readable: `SystemStabilityIndex` = 10. |

## Windows lifecycle (for `data/os_lifecycle.toml`)

Fetched 2026-10-20 from Microsoft Learn:
- https://learn.microsoft.com/en-us/lifecycle/products/windows-11-home-and-pro
- https://learn.microsoft.com/en-us/lifecycle/products/windows-11-enterprise-and-education
- https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro
