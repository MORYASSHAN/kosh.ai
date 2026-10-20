//! Startup items: Run/RunOnce keys, Startup folders, scheduled tasks with logon/boot
//! triggers, and auto-start non-Microsoft services. `StartupApproved` keys tell
//! whether the user disabled an item in Task Manager / Settings.

use super::reg::{self, Hive};
use super::winfile;
use super::wmi_util::{get_str, query_ns};
use crate::{CollectResult, Ctx, Patch};
use kosh_core::StartupItem;
use std::path::PathBuf;

const RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const RUN_ONCE: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce";
const APPROVED: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// StartupApproved values: first byte even = enabled, odd = disabled. Missing = enabled.
fn approved(hive: Hive, sub: &str, name: &str) -> bool {
    // StartupApproved for both HKLM views lives in the 64-bit view.
    let hive = if hive == Hive::Hklm32 {
        Hive::Hklm64
    } else {
        hive
    };
    reg::open(hive, &format!(r"{APPROVED}\{sub}"))
        .and_then(|k| k.get_raw_value(name).ok())
        .and_then(|v| v.bytes.first().copied())
        .map(|b| b % 2 == 0)
        .unwrap_or(true)
}

fn windows_dir() -> String {
    std::env::var("SystemRoot")
        .unwrap_or_else(|_| r"C:\Windows".into())
        .to_lowercase()
}

/// Fill publisher, signature and is_microsoft for an item with a known image path.
fn enrich(item: &mut StartupItem) {
    let Some(path) = item.image_path.clone() else {
        return;
    };
    item.publisher = winfile::company_name(&path);
    let in_windows = path.to_lowercase().starts_with(&windows_dir());
    item.is_microsoft = item
        .publisher
        .as_deref()
        .map(|p| p.to_lowercase().contains("microsoft"))
        .unwrap_or(false);
    // Catalog-signed Windows files would wrongly look unsigned; only check others.
    if !in_windows {
        item.signed = winfile::is_signed(&path);
    }
}

fn run_keys(items: &mut Vec<StartupItem>) {
    let sources = [
        (Hive::Hkcu, RUN, "HKCU Run", "Run"),
        (Hive::Hkcu, RUN_ONCE, "HKCU RunOnce", "Run"),
        (Hive::Hklm64, RUN, "HKLM Run", "Run"),
        (Hive::Hklm64, RUN_ONCE, "HKLM RunOnce", "Run"),
        (Hive::Hklm32, RUN, "HKLM Run (32-bit)", "Run32"),
        (Hive::Hklm32, RUN_ONCE, "HKLM RunOnce (32-bit)", "Run32"),
    ];
    for (hive, path, label, approved_sub) in sources {
        let Some(k) = reg::open(hive, path) else {
            continue;
        };
        for (name, _) in k.enum_values().flatten() {
            if name.is_empty() {
                continue;
            }
            let Some(command) = reg::get_string(&k, &name) else {
                continue;
            };
            let mut item = StartupItem {
                image_path: winfile::image_from_command(&command),
                enabled: approved(hive, approved_sub, &name),
                name,
                command,
                location: label.to_string(),
                ..Default::default()
            };
            enrich(&mut item);
            items.push(item);
        }
    }
}

fn startup_folders(items: &mut Vec<StartupItem>) {
    let mut folders: Vec<(PathBuf, Hive, &str)> = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        folders.push((
            PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup"),
            Hive::Hkcu,
            "Startup folder (user)",
        ));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        folders.push((
            PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs\StartUp"),
            Hive::Hklm64,
            "Startup folder (all users)",
        ));
    }
    for (dir, hive, label) in folders {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let file_name = e.file_name().to_string_lossy().to_string();
            if file_name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let full = e.path().to_string_lossy().to_string();
            let (image, command) = if file_name.to_lowercase().ends_with(".lnk") {
                match winfile::resolve_lnk(&full) {
                    Some((t, a)) => (Some(t.clone()), format!("{t} {a}").trim().to_string()),
                    None => (None, full.clone()),
                }
            } else {
                (Some(full.clone()), full.clone())
            };
            let mut item = StartupItem {
                enabled: approved(hive, "StartupFolder", &file_name),
                name: file_name,
                command,
                image_path: image,
                location: label.to_string(),
                ..Default::default()
            };
            enrich(&mut item);
            items.push(item);
        }
    }
}

fn services(items: &mut Vec<StartupItem>) {
    let Ok(rows) = query_ns(
        "root\\cimv2",
        "SELECT Name, DisplayName, PathName FROM Win32_Service WHERE StartMode = 'Auto'",
    ) else {
        return;
    };
    let windir = windows_dir();
    for r in rows {
        let Some(path_name) = get_str(&r, "PathName") else {
            continue;
        };
        let image = winfile::image_from_command(&path_name);
        // Services inside the Windows folder are part of Windows.
        if image
            .as_deref()
            .map(|p| p.to_lowercase().starts_with(&windir))
            .unwrap_or(true)
        {
            continue;
        }
        let mut item = StartupItem {
            name: get_str(&r, "DisplayName")
                .or_else(|| get_str(&r, "Name"))
                .unwrap_or_default(),
            command: path_name,
            image_path: image,
            location: "Service (automatic)".into(),
            enabled: true,
            ..Default::default()
        };
        enrich(&mut item);
        if !item.is_microsoft {
            items.push(item);
        }
    }
}

fn scheduled_tasks(items: &mut Vec<StartupItem>) {
    use windows::core::{Interface, BSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::TaskScheduler::{
        IExecAction, ITaskFolder, ITaskService, TaskScheduler, TASK_ACTION_EXEC, TASK_TRIGGER_BOOT,
        TASK_TRIGGER_LOGON,
    };
    use windows::Win32::System::Variant::VARIANT;

    fn walk(folder: &ITaskFolder, depth: u32, out: &mut Vec<StartupItem>) {
        // SAFETY: COM calls on valid interfaces; out-params are initialised locals.
        unsafe {
            let path = folder.Path().map(|b| b.to_string()).unwrap_or_default();
            // Tasks under \Microsoft are part of Windows.
            if path.to_lowercase().starts_with(r"\microsoft") {
                return;
            }
            if let Ok(tasks) = folder.GetTasks(1 /* TASK_ENUM_HIDDEN */) {
                let count = tasks.Count().unwrap_or(0);
                for i in 1..=count {
                    let Ok(task) = tasks.get_Item(&VARIANT::from(i)) else {
                        continue;
                    };
                    let Ok(def) = task.Definition() else { continue };
                    let mut at_logon_or_boot = false;
                    if let Ok(triggers) = def.Triggers() {
                        let mut n = 0i32;
                        let _ = triggers.Count(&mut n);
                        for t in 1..=n {
                            if let Ok(trig) = triggers.get_Item(t) {
                                let mut ty = Default::default();
                                if trig.Type(&mut ty).is_ok()
                                    && (ty == TASK_TRIGGER_LOGON || ty == TASK_TRIGGER_BOOT)
                                {
                                    at_logon_or_boot = true;
                                }
                            }
                        }
                    }
                    if !at_logon_or_boot {
                        continue;
                    }
                    let mut command = String::new();
                    if let Ok(actions) = def.Actions() {
                        if let Ok(action) = actions.get_Item(1) {
                            let mut ty = Default::default();
                            if action.Type(&mut ty).is_ok() && ty == TASK_ACTION_EXEC {
                                if let Ok(exec) = action.cast::<IExecAction>() {
                                    let mut p = BSTR::new();
                                    let mut a = BSTR::new();
                                    let _ = exec.Path(&mut p);
                                    let _ = exec.Arguments(&mut a);
                                    command = format!("{} {}", p, a).trim().to_string();
                                }
                            }
                        }
                    }
                    if command.is_empty() {
                        continue;
                    }
                    let enabled = task.Enabled().map(|v| v.as_bool()).unwrap_or(true);
                    let mut item = StartupItem {
                        name: task.Name().map(|b| b.to_string()).unwrap_or_default(),
                        image_path: winfile::image_from_command(&command),
                        command,
                        location: "Scheduled task (at logon/boot)".into(),
                        enabled,
                        ..Default::default()
                    };
                    enrich(&mut item);
                    out.push(item);
                }
            }
            if depth < 6 {
                if let Ok(subs) = folder.GetFolders(0) {
                    let n = subs.Count().unwrap_or(0);
                    for i in 1..=n {
                        if let Ok(f) = subs.get_Item(&VARIANT::from(i)) {
                            walk(&f, depth + 1, out);
                        }
                    }
                }
            }
        }
    }

    // SAFETY: COM initialised on this collector thread.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(svc) =
            CoCreateInstance::<_, ITaskService>(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
        else {
            return;
        };
        let empty = VARIANT::default();
        if svc.Connect(&empty, &empty, &empty, &empty).is_err() {
            return;
        }
        if let Ok(root) = svc.GetFolder(&BSTR::from("\\")) {
            walk(&root, 0, items);
        }
    }
}

pub fn startup(_ctx: &Ctx) -> CollectResult {
    let mut items = Vec::new();
    run_keys(&mut items);
    startup_folders(&mut items);
    scheduled_tasks(&mut items);
    services(&mut items);
    let patch: Patch = Box::new(move |s| s.startup = items);
    Ok(patch)
}
