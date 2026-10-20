# Changelog

## M0 + M1 — Workspace and collectors (2026-10-20)

- Rust workspace with crates `kosh-core`, `kosh-collectors`, `kosh-rules`, `kosh-vuln`, `kosh-report`, `kosh-ai`, `kosh-cli`.
- CI for Windows, Linux and macOS (format, lint, test, `cargo audit`).
- Lint rules: no `unwrap()`/`expect()` outside tests.
- `kosh-core`: Snapshot, Finding, Evidence, SafeAction (closed list of safe actions), anonymizer.
- Windows collectors: OS, system, CPU, RAM (sticks, slots), memory pressure (performance counters), running apps, disks, volumes, battery, GPU, devices and drivers, event logs (provider + ID filtered), reliability, crash dumps, pending reboot, Windows Update history, antivirus, firewall, Secure Boot, TPM, risky settings, installed software, startup items, network adapters, internet check, secret file names.
- `kosh scan` prints the Snapshot.
- Facts verified on the owner's laptop recorded in `docs/VERIFIED_SOURCES.md`.
