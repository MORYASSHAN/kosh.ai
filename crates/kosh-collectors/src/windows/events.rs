//! Event log counts and samples (last 30 days), crash dump listing and the
//! Reliability Monitor stability index.
//!
//! IMPORTANT (verified, see VERIFIED_SOURCES.md): event IDs are reused by many
//! providers, so every query filters by provider name AND event id.

use super::wmi_util::{get_date, get_f64, query_ns};
use crate::{CollectResult, Ctx, Patch, Skip};
use chrono::{DateTime, Utc};
use kosh_core::{EventGroup, EventKind, EventSample};
use regex::Regex;
use std::collections::BTreeMap;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::EventLog::{
    EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
    EvtRenderEventXml, EVT_HANDLE,
};

pub const WINDOW_DAYS: u32 = 30;
/// Stop counting after this many events per group (the count is then "at least").
const MAX_EVENTS: u32 = 2000;
const MAX_SAMPLES: usize = 5;

struct Spec {
    kind: EventKind,
    log: &'static str,
    providers: &'static [&'static str],
    ids: &'static [u32],
}

const SPECS: &[Spec] = &[
    Spec {
        kind: EventKind::UnexpectedShutdown,
        log: "System",
        providers: &["Microsoft-Windows-Kernel-Power"],
        ids: &[41],
    },
    Spec {
        kind: EventKind::PreviousShutdownUnexpected,
        log: "System",
        providers: &["EventLog"],
        ids: &[6008],
    },
    Spec {
        kind: EventKind::BlueScreen,
        log: "System",
        providers: &["Microsoft-Windows-WER-SystemErrorReporting", "BugCheck"],
        ids: &[1001],
    },
    Spec {
        kind: EventKind::HardwareError,
        log: "System",
        providers: &["Microsoft-Windows-WHEA-Logger"],
        ids: &[17, 18, 19, 47],
    },
    Spec {
        kind: EventKind::DiskError,
        log: "System",
        providers: &["disk"],
        ids: &[7, 153],
    },
    Spec {
        kind: EventKind::FileSystemCorruption,
        log: "System",
        providers: &["Ntfs", "Microsoft-Windows-Ntfs"],
        ids: &[55],
    },
    Spec {
        kind: EventKind::ServiceFailure,
        log: "System",
        providers: &["Service Control Manager"],
        ids: &[7000, 7001, 7023, 7031, 7034],
    },
    Spec {
        kind: EventKind::AppCrash,
        log: "Application",
        providers: &["Application Error"],
        ids: &[1000],
    },
    Spec {
        kind: EventKind::AppHang,
        log: "Application",
        providers: &["Application Hang"],
        ids: &[1002],
    },
    Spec {
        kind: EventKind::LowMemory,
        log: "System",
        providers: &["Microsoft-Windows-Resource-Exhaustion-Detector"],
        ids: &[2004],
    },
    Spec {
        kind: EventKind::UpdateFailure,
        log: "System",
        providers: &["Microsoft-Windows-WindowsUpdateClient"],
        ids: &[20],
    },
    Spec {
        kind: EventKind::UpdateSuccess,
        log: "System",
        providers: &["Microsoft-Windows-WindowsUpdateClient"],
        ids: &[19],
    },
];

fn xpath(spec: &Spec) -> String {
    let providers = spec
        .providers
        .iter()
        .map(|p| format!("@Name='{p}'"))
        .collect::<Vec<_>>()
        .join(" or ");
    let ids = spec
        .ids
        .iter()
        .map(|i| format!("EventID={i}"))
        .collect::<Vec<_>>()
        .join(" or ");
    let ms = u64::from(WINDOW_DAYS) * 24 * 3600 * 1000;
    format!("*[System[Provider[{providers}] and ({ids}) and TimeCreated[timediff(@SystemTime) <= {ms}]]]")
}

/// Fields we need from one event's XML.
#[derive(Debug, Default)]
struct Parsed {
    provider: String,
    id: u32,
    time: Option<DateTime<Utc>>,
    data: Vec<(Option<String>, String)>,
}

fn parse_event_xml(xml: &str) -> Option<Parsed> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let mut p = Parsed::default();
    for n in doc.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "Provider" => {
                if let Some(name) = n.attribute("Name") {
                    p.provider = name.to_string();
                }
            }
            "EventID" => p.id = n.text().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
            "TimeCreated" => {
                p.time = n
                    .attribute("SystemTime")
                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&Utc))
            }
            "Data" => p.data.push((
                n.attribute("Name").map(str::to_string),
                n.text().unwrap_or_default().trim().to_string(),
            )),
            _ => {}
        }
    }
    Some(p)
}

fn data_named<'a>(p: &'a Parsed, name: &str) -> Option<&'a str> {
    p.data
        .iter()
        .find(|(n, _)| n.as_deref() == Some(name))
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn data_first(p: &Parsed) -> Option<&str> {
    p.data
        .first()
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn bugcheck_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    #[allow(clippy::unwrap_used)]
    RE.get_or_init(|| Regex::new(r"0x[0-9a-fA-F]{8}").unwrap())
}

/// The "subject" of an event: which app, service, bugcheck code or update it is about.
fn subject(kind: EventKind, p: &Parsed) -> Option<String> {
    match kind {
        EventKind::AppCrash | EventKind::AppHang => data_first(p).map(str::to_string),
        EventKind::ServiceFailure => data_named(p, "param1")
            .or_else(|| data_first(p))
            .map(str::to_string),
        EventKind::BlueScreen => p
            .data
            .iter()
            .find_map(|(_, v)| bugcheck_re().find(v).map(|m| m.as_str().to_lowercase())),
        EventKind::UpdateFailure | EventKind::UpdateSuccess => data_named(p, "updateTitle")
            .or_else(|| data_first(p))
            .map(str::to_string),
        EventKind::DiskError => data_first(p).map(str::to_string),
        _ => None,
    }
}

struct Handle(EVT_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: handle came from EvtQuery/EvtNext.
        unsafe {
            let _ = EvtClose(self.0);
        }
    }
}

fn render_xml(event: EVT_HANDLE) -> Option<String> {
    let mut used = 0u32;
    let mut count = 0u32;
    // SAFETY: first call asks for the needed buffer size.
    unsafe {
        let _ = EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            0,
            None,
            &mut used,
            &mut count,
        );
    }
    if used == 0 {
        return None;
    }
    let mut buf: Vec<u16> = vec![0; (used as usize).div_ceil(2) + 1];
    // SAFETY: buffer is at least `used` bytes.
    unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            (buf.len() * 2) as u32,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut used,
            &mut count,
        )
        .ok()?;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn run_spec(spec: &Spec) -> Result<EventGroup, String> {
    let mut group = EventGroup {
        kind: spec.kind,
        log: spec.log.to_string(),
        providers: spec.providers.iter().map(|s| s.to_string()).collect(),
        ids: spec.ids.to_vec(),
        ..Default::default()
    };
    let path = HSTRING::from(spec.log);
    let q = HSTRING::from(xpath(spec));
    // SAFETY: valid strings; the result handle is closed by `Handle`.
    let results = unsafe {
        EvtQuery(
            None,
            PCWSTR(path.as_ptr()),
            PCWSTR(q.as_ptr()),
            EvtQueryChannelPath.0 | EvtQueryReverseDirection.0,
        )
    }
    .map_err(|e| format!("EvtQuery {}: {e}", spec.log))?;
    let results = Handle(results);

    let mut by_subject: BTreeMap<String, u32> = BTreeMap::new();
    'outer: loop {
        let mut batch = [0isize; 64];
        let mut returned = 0u32;
        // SAFETY: batch buffer is valid; returned handles are wrapped and closed.
        let ok = unsafe { EvtNext(results.0, &mut batch, 1000, 0, &mut returned) };
        if ok.is_err() || returned == 0 {
            break;
        }
        for raw in batch.iter().take(returned as usize) {
            let ev = Handle(EVT_HANDLE(*raw));
            group.count += 1;
            let Some(xml) = render_xml(ev.0) else {
                continue;
            };
            let Some(p) = parse_event_xml(&xml) else {
                continue;
            };
            let subj = subject(spec.kind, &p);
            if let Some(s) = &subj {
                *by_subject.entry(s.clone()).or_insert(0) += 1;
            }
            if group.last_seen.is_none() {
                group.last_seen = p.time;
            }
            if group.samples.len() < MAX_SAMPLES {
                group.samples.push(EventSample {
                    time: p.time,
                    provider: p.provider.clone(),
                    id: p.id,
                    subject: subj,
                    message: None,
                });
            }
            if group.count >= MAX_EVENTS {
                break 'outer;
            }
        }
    }
    group.by_subject = by_subject;
    Ok(group)
}

pub fn events(_ctx: &Ctx) -> CollectResult {
    let mut groups = Vec::new();
    let mut errors = Vec::new();
    for spec in SPECS {
        match run_spec(spec) {
            Ok(g) => groups.push(g),
            Err(e) => errors.push(e),
        }
    }
    if groups.is_empty() {
        return Err(Skip::Failed(errors.join("; ")));
    }
    let patch: Patch = Box::new(move |s| {
        s.events.window_days = WINDOW_DAYS;
        s.events.groups = groups;
        for e in errors {
            s.errors.push(kosh_core::CollectorError {
                collector: "events".into(),
                message: e,
            });
        }
    });
    Ok(patch)
}

pub fn reliability(_ctx: &Ctx) -> CollectResult {
    let rows = query_ns(
        "root\\cimv2",
        "SELECT SystemStabilityIndex, TimeGenerated FROM Win32_ReliabilityStabilityMetrics",
    )
    .map_err(|e| Skip::NotAvailable(format!("reliability data unavailable: {e}")))?;
    let latest = rows
        .iter()
        .filter_map(|r| {
            Some((
                get_date(r, "TimeGenerated")?,
                get_f64(r, "SystemStabilityIndex")?,
            ))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, v)| (v * 100.0).round() / 100.0);
    let Some(index) = latest else {
        return Err(Skip::NotAvailable("reliability data unavailable".into()));
    };
    let patch: Patch = Box::new(move |s| s.events.stability_index = Some(index));
    Ok(patch)
}

pub fn crash_dumps(_ctx: &Ctx) -> CollectResult {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let dir = std::path::Path::new(&windir).join("Minidump");
    if !dir.exists() {
        let patch: Patch = Box::new(|s| s.events.minidumps = Vec::new());
        return Ok(patch);
    }
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Err(Skip::NeedsAdmin),
        Err(e) => return Err(Skip::Failed(format!("listing Minidump: {e}"))),
    };
    let cutoff = Utc::now() - chrono::Duration::days(i64::from(WINDOW_DAYS));
    let mut dumps: Vec<DateTime<Utc>> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case("dmp"))
                .unwrap_or(false)
        })
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .map(DateTime::<Utc>::from)
        .filter(|t| *t >= cutoff)
        .collect();
    dumps.sort();
    let patch: Patch = Box::new(move |s| s.events.minidumps = dumps);
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_CRASH: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Application Error'/><EventID Qualifiers='0'>1000</EventID><TimeCreated SystemTime='2026-10-03T16:20:08.1234567Z'/></System><EventData><Data>UDClientService.exe</Data><Data>26.7.0.7</Data></EventData></Event>"#;

    #[test]
    fn parses_app_crash() {
        let p = parse_event_xml(APP_CRASH).expect("parse");
        assert_eq!(p.provider, "Application Error");
        assert_eq!(p.id, 1000);
        assert!(p.time.is_some());
        assert_eq!(
            subject(EventKind::AppCrash, &p).as_deref(),
            Some("UDClientService.exe")
        );
    }

    #[test]
    fn bugcheck_code_extracted() {
        let p = Parsed {
            data: vec![(
                Some("param1".into()),
                "0x0000009f (0x0000000000000003, ...)".into(),
            )],
            ..Default::default()
        };
        assert_eq!(
            subject(EventKind::BlueScreen, &p).as_deref(),
            Some("0x0000009f")
        );
    }

    #[test]
    fn xpath_filters_provider_and_id() {
        let x = xpath(&SPECS[5]);
        assert!(x.contains("@Name='Ntfs' or @Name='Microsoft-Windows-Ntfs'"));
        assert!(x.contains("EventID=55"));
    }
}
