//! Collectors gather read-only facts about the computer into a [`Snapshot`].
//!
//! Each collector is independent. A failing collector never stops the scan: it
//! records an error (or a "skipped" reason) and the scan continues. Every
//! collector runs with a timeout.
//!
//! Platform-specific code lives in `src/<platform>/`. Nothing here changes the computer.

use kosh_core::{collectors as names, CollectorError, SkippedCheck, Snapshot};
use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Duration;

pub mod common;
pub mod monitor;

#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
mod macos;

/// Settings for one scan.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub elevated: bool,
    /// Skip network tests.
    pub offline: bool,
    /// How long to sample memory pressure.
    pub sample_secs: u32,
    /// Per-collector timeout.
    pub timeout: Duration,
    /// Folder holding `kosh monitor` history files, if any.
    pub monitor_dir: Option<std::path::PathBuf>,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx {
            elevated: is_elevated(),
            offline: false,
            sample_secs: 30,
            timeout: Duration::from_secs(15),
            monitor_dir: None,
        }
    }
}

/// A change to apply to the Snapshot once a collector succeeds.
pub type Patch = Box<dyn FnOnce(&mut Snapshot) + Send>;

/// Why a collector produced nothing.
#[derive(Debug, Clone)]
pub enum Skip {
    /// The check needs Administrator rights.
    NeedsAdmin,
    /// The check does not apply / is not available here (not an error).
    NotAvailable(String),
    /// The collector failed.
    Failed(String),
}

impl From<String> for Skip {
    fn from(s: String) -> Self {
        Skip::Failed(s)
    }
}

impl From<&str> for Skip {
    fn from(s: &str) -> Self {
        Skip::Failed(s.to_string())
    }
}

pub type CollectResult = Result<Patch, Skip>;

pub struct Collector {
    pub name: &'static str,
    pub run: fn(&Ctx) -> CollectResult,
    /// Needs extra time (e.g. memory sampling). Timeout is extended by the sample length.
    pub long_running: bool,
}

/// All collectors for this platform.
pub fn all_collectors() -> Vec<Collector> {
    let mut list = common::collectors();
    #[cfg(windows)]
    list.extend(windows::collectors());
    #[cfg(target_os = "linux")]
    list.extend(linux::collectors());
    #[cfg(target_os = "macos")]
    list.extend(macos::collectors());
    list
}

/// Was this process started as Administrator / root?
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        windows::elevation::is_elevated()
    }
    #[cfg(unix)]
    {
        // `id -u` prints 0 for root. Read-only command.
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

/// Run a full scan, or only the named collectors when `only` is given.
pub fn scan(ctx: &Ctx, only: Option<&BTreeSet<String>>) -> Snapshot {
    let mut snap = Snapshot::new();
    snap.elevated = ctx.elevated;
    snap.offline = ctx.offline;
    snap.platform.family = std::env::consts::OS.to_string();
    snap.platform.architecture = std::env::consts::ARCH.to_string();

    let selected: Vec<Collector> = all_collectors()
        .into_iter()
        .filter(|c| only.map(|o| o.contains(c.name)).unwrap_or(true))
        .collect();

    // Long-running collectors (memory sampling) run in the background while the
    // rest run one after another, so the whole scan takes about as long as the sampling.
    let (long, short): (Vec<_>, Vec<_>) = selected.into_iter().partition(|c| c.long_running);

    let background: Vec<_> = long
        .into_iter()
        .map(|c| {
            let rx = spawn(&c, ctx);
            let timeout = ctx.timeout + Duration::from_secs(u64::from(ctx.sample_secs) + 10);
            (c.name, rx, timeout)
        })
        .collect();

    for c in short {
        let rx = spawn(&c, ctx);
        apply(&mut snap, c.name, rx, ctx.timeout);
    }
    for (name, rx, timeout) in background {
        apply(&mut snap, name, rx, timeout);
    }

    snap.collected.sort();
    snap.collected.dedup();
    snap
}

fn spawn(c: &Collector, ctx: &Ctx) -> mpsc::Receiver<CollectResult> {
    let (tx, rx) = mpsc::channel();
    let run = c.run;
    let ctx = ctx.clone();
    let name = c.name;
    let spawned = std::thread::Builder::new()
        .name(format!("collector-{name}"))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&ctx)))
                .unwrap_or_else(|_| Err(Skip::Failed("collector panicked".into())));
            // The receiver may have given up after a timeout; that's fine.
            let _ = tx.send(result);
        });
    if let Err(e) = spawned {
        let (tx2, rx2) = mpsc::channel();
        let _ = tx2.send(Err(Skip::Failed(format!("could not start thread: {e}"))));
        return rx2;
    }
    rx
}

fn apply(snap: &mut Snapshot, name: &str, rx: mpsc::Receiver<CollectResult>, timeout: Duration) {
    let started = std::time::Instant::now();
    match rx.recv_timeout(timeout) {
        Ok(Ok(patch)) => {
            patch(snap);
            snap.collected.push(name.to_string());
            tracing::debug!(
                collector = name,
                ms = started.elapsed().as_millis() as u64,
                "ok"
            );
        }
        Ok(Err(Skip::NeedsAdmin)) => snap.skipped.push(SkippedCheck {
            check: name.to_string(),
            reason: "needs admin".to_string(),
        }),
        Ok(Err(Skip::NotAvailable(reason))) => snap.skipped.push(SkippedCheck {
            check: name.to_string(),
            reason,
        }),
        Ok(Err(Skip::Failed(message))) => {
            tracing::warn!(collector = name, "failed: {message}");
            snap.errors.push(CollectorError {
                collector: name.to_string(),
                message,
            })
        }
        Err(_) => snap.errors.push(CollectorError {
            collector: name.to_string(),
            message: format!("timed out after {} s", timeout.as_secs()),
        }),
    }
}

/// The collectors a given rule needs, used by `kosh verify` and `kosh doctor`.
/// Always includes the OS collector so the report header is filled.
pub fn base_collectors() -> BTreeSet<String> {
    [names::OS, names::SYSTEM]
        .iter()
        .map(|s| s.to_string())
        .collect()
}
