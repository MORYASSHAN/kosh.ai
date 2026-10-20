//! Collectors that work the same on every platform.

use crate::{CollectResult, Collector, Ctx, Patch, Skip};
use kosh_core::{collectors as names, AppUsage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::{ProcessesToUpdate, System};

pub fn collectors() -> Vec<Collector> {
    #[allow(unused_mut)]
    let mut list = vec![
        Collector {
            name: names::PROCESSES,
            run: processes,
            long_running: false,
        },
        Collector {
            name: names::CONNECTIVITY,
            run: connectivity,
            long_running: false,
        },
        Collector {
            name: names::SECRET_FILES,
            run: secret_files,
            long_running: false,
        },
        Collector {
            name: names::MONITOR_HISTORY,
            run: monitor_history,
            long_running: false,
        },
    ];
    #[cfg(not(windows))]
    list.push(Collector {
        name: names::MEMORY_PRESSURE,
        run: memory_pressure_sysinfo,
        long_running: true,
    });
    list
}

/// How many apps to keep in the top lists.
const TOP_N: usize = 15;

/// Turn a process name into an app group name: "chrome.exe" -> "chrome".
pub fn app_group_name(process_name: &str) -> String {
    let n = process_name.trim();
    let lower = n.to_ascii_lowercase();
    if lower.ends_with(".exe") {
        n[..n.len() - 4].to_string()
    } else {
        n.to_string()
    }
}

/// Group processes by app name and return (top by memory, top by CPU).
pub fn group_processes(sys: &System) -> (Vec<AppUsage>, Vec<AppUsage>) {
    let cpu_count = sys.cpus().len().max(1) as f32;
    let mut groups: HashMap<String, AppUsage> = HashMap::new();
    for p in sys.processes().values() {
        // Skip threads listed as processes on Linux.
        if p.thread_kind().is_some() {
            continue;
        }
        let name = app_group_name(&p.name().to_string_lossy());
        if name.is_empty() {
            continue;
        }
        let entry = groups
            .entry(name.to_lowercase())
            .or_insert_with(|| AppUsage {
                name: name.clone(),
                ..Default::default()
            });
        entry.process_count += 1;
        entry.memory_bytes += p.memory();
        entry.cpu_percent += p.cpu_usage() / cpu_count;
    }
    let mut all: Vec<AppUsage> = groups
        .into_values()
        // "Idle" / "System Idle Process" is not a real app.
        .filter(|a| {
            !a.name.eq_ignore_ascii_case("system idle process")
                && !a.name.eq_ignore_ascii_case("idle")
        })
        .collect();

    all.sort_by_key(|a| std::cmp::Reverse(a.memory_bytes));
    let by_mem: Vec<AppUsage> = all.iter().take(TOP_N).cloned().collect();
    all.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent));
    let by_cpu: Vec<AppUsage> = all
        .into_iter()
        .take(TOP_N)
        .map(|mut a| {
            a.cpu_percent = (a.cpu_percent * 10.0).round() / 10.0;
            a
        })
        .collect();
    (by_mem, by_cpu)
}

fn processes(_ctx: &Ctx) -> CollectResult {
    let mut sys = System::new();
    sys.refresh_cpu_usage();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    // CPU usage needs two measurements some time apart.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(1000)));
    sys.refresh_cpu_usage();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let (by_mem, by_cpu) = group_processes(&sys);
    let load = sys.global_cpu_usage();
    let patch: Patch = Box::new(move |s| {
        s.performance.top_by_memory = by_mem;
        s.performance.top_by_cpu = by_cpu;
        s.hardware.cpu.load_percent = Some((load * 10.0).round() / 10.0);
    });
    Ok(patch)
}

/// Memory sampling for macOS / Linux using sysinfo (Windows uses performance counters).
#[cfg(not(windows))]
fn memory_pressure_sysinfo(ctx: &Ctx) -> CollectResult {
    let mut sys = System::new();
    let mut samples = Vec::new();
    for i in 0..ctx.sample_secs.max(1) {
        if i > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
        sys.refresh_memory();
        let total = sys.total_memory();
        let avail = sys.available_memory();
        let swap_used = sys.used_swap();
        let commit = if total > 0 {
            Some(((total - avail + swap_used) as f64 * 100.0 / total as f64).min(200.0))
        } else {
            None
        };
        samples.push(kosh_core::MemorySample {
            available_mb: avail / 1_048_576,
            total_mb: total / 1_048_576,
            commit_percent: commit,
            pages_per_sec: None,
        });
    }
    let patch: Patch = Box::new(move |s| {
        s.performance.sample_interval_secs = 1;
        s.performance.samples = samples;
    });
    Ok(patch)
}

/// Is DNS working and can we reach a well-known HTTPS site?
fn connectivity(ctx: &Ctx) -> CollectResult {
    if ctx.offline {
        return Err(Skip::NotAvailable("offline mode".into()));
    }
    use std::net::ToSocketAddrs;
    let dns_ok = ("www.microsoft.com", 443)
        .to_socket_addrs()
        .map(|mut a| a.next().is_some())
        .unwrap_or(false);
    let internet_ok = if dns_ok {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .ok()
            .and_then(|c| c.head("https://www.microsoft.com/").send().ok())
            .is_some()
    } else {
        false
    };
    let patch: Patch = Box::new(move |s| {
        s.network.dns_ok = Some(dns_ok);
        s.network.internet_ok = Some(internet_ok);
    });
    Ok(patch)
}

/// File *names* that suggest secrets are stored in plain text. Never reads contents.
pub fn looks_like_secret_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let (stem, ext) = match n.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => (n.clone(), String::new()),
    };
    // Templates hold placeholder values, not secrets.
    let template = ["example", "sample", "template", "dist", "defaults"]
        .iter()
        .any(|t| n.ends_with(&format!(".{t}")));
    if template {
        return false;
    }
    if n == ".env" || n.starts_with(".env.") {
        return true;
    }
    if matches!(n.as_str(), "id_rsa" | "id_ed25519" | "id_ecdsa" | "id_dsa") {
        return true;
    }
    if ext == "pem" || ext == "ppk" {
        return true;
    }
    let plain_ext = matches!(
        ext.as_str(),
        "txt" | "csv" | "xlsx" | "xls" | "docx" | "doc" | "json" | "rtf"
    );
    let secret_words = [
        "password",
        "passwords",
        "passwd",
        "credential",
        "credentials",
        "pin codes",
        "secret",
    ];
    plain_ext && secret_words.iter().any(|w| stem.contains(w))
}

/// Personal folders to check for secret file names.
fn personal_folders() -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from);
    let Some(home) = home else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for label in ["Desktop", "Documents", "Downloads"] {
        out.push((label.to_string(), home.join(label)));
        let od = home.join("OneDrive").join(label);
        if od.is_dir() {
            out.push((format!("OneDrive\\{label}"), od));
        }
    }
    out
}

fn secret_files(_ctx: &Ctx) -> CollectResult {
    let mut found = Vec::new();
    let mut budget = 20_000usize; // never walk forever
    for (label, dir) in personal_folders() {
        scan_names(&dir, &label, 0, &mut found, &mut budget);
    }
    found.sort();
    found.truncate(50);
    let patch: Patch = Box::new(move |s| s.security.secret_file_names = found);
    Ok(patch)
}

fn scan_names(dir: &Path, label: &str, depth: u32, found: &mut Vec<String>, budget: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            // Skip big tool folders that are full of keys by design.
            let skip = matches!(
                name.to_ascii_lowercase().as_str(),
                "node_modules" | ".git" | "target" | "venv" | ".venv" | "site-packages"
            );
            if depth < 2 && !skip {
                scan_names(
                    &e.path(),
                    &format!("{label}\\{name}"),
                    depth + 1,
                    found,
                    budget,
                );
            }
        } else if looks_like_secret_file(&name) {
            found.push(format!("{label}\\{name}"));
        }
    }
}

fn monitor_history(ctx: &Ctx) -> CollectResult {
    let Some(dir) = &ctx.monitor_dir else {
        return Err(Skip::NotAvailable("no monitor history".into()));
    };
    match crate::monitor::summarize(dir, 7) {
        Some(summary) => {
            let patch: Patch = Box::new(move |s| s.performance.history = Some(summary));
            Ok(patch)
        }
        None => Err(Skip::NotAvailable(
            "no monitor history (run `kosh monitor` to collect it)".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_exe_names() {
        assert_eq!(app_group_name("chrome.exe"), "chrome");
        assert_eq!(app_group_name("Code.EXE"), "Code");
        assert_eq!(app_group_name("firefox"), "firefox");
    }

    #[test]
    fn secret_file_names() {
        for yes in [
            "passwords.txt",
            "My Passwords.xlsx",
            ".env",
            "server.pem",
            "id_rsa",
            "bank-credentials.csv",
            "key.ppk",
        ] {
            assert!(looks_like_secret_file(yes), "{yes}");
        }
        for no in [
            "password-manager-setup.exe",
            "notes.txt",
            "photo.jpg",
            "env.txt",
            "secrets.kdbx",
            "README.md",
            ".env.example",
            ".env.sample",
        ] {
            assert!(!looks_like_secret_file(no), "{no}");
        }
    }
}
