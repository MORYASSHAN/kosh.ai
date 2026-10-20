# kosh-collectors

Gathers read-only facts about the computer into a `Snapshot`.

- `src/common.rs`: collectors that work everywhere (running apps grouped by name, internet check, secret file *names*, monitor history).
- `src/windows/`: Windows collectors (WMI, registry, event log, performance counters, battery, Task Scheduler, Windows Update history).
- `src/macos/`, `src/linux/`: other platforms (Milestones 9 and 10).
- `src/monitor.rs`: `kosh monitor` sampling and its history summary.

Rules:
- A collector never stops the scan. It returns facts, "skipped" (with a reason such as "needs admin") or an error.
- Each collector has a timeout.
- Nothing here changes the computer. Registry keys are opened read-only and only file *names* are read, never contents (except the battery/disk queries, which ask the device for its status).
- Every Windows source is listed in `docs/VERIFIED_SOURCES.md`.
