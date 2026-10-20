//! `kosh monitor`: light background sampling of memory and CPU, saved as
//! one JSON line per sample in daily files. Rules use the summary as real history.

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use kosh_core::MonitorSummary;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use sysinfo::System;

const AVAILABLE_BUCKETS: usize = 20;
const COMMIT_BUCKETS: usize = 21;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitorRecord {
    pub t: DateTime<Utc>,
    pub available_percent: f64,
    pub commit_percent: Option<f64>,
    pub cpu_percent: f64,
}

/// Keeps one `System` alive between samples so CPU usage can be measured cheaply.
pub struct Sampler {
    sys: System,
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Sampler {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        Sampler { sys }
    }

    pub fn sample(&mut self) -> MonitorRecord {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let total = self.sys.total_memory();
        let avail = self.sys.available_memory();
        let available_percent = if total > 0 {
            avail as f64 * 100.0 / total as f64
        } else {
            100.0
        };
        MonitorRecord {
            t: Utc::now(),
            available_percent: round1(available_percent),
            commit_percent: commit_percent().map(round1),
            cpu_percent: round1(f64::from(self.sys.global_cpu_usage())),
        }
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// System-wide commit charge as a percentage of the commit limit.
pub fn commit_percent() -> Option<f64> {
    #[cfg(windows)]
    {
        crate::windows::perf::commit_percent()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn file_for(dir: &Path, t: DateTime<Utc>) -> PathBuf {
    dir.join(format!("monitor-{}.jsonl", t.format("%Y-%m-%d")))
}

pub fn append(dir: &Path, rec: &MonitorRecord) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_for(dir, rec.t))?;
    let line = serde_json::to_string(rec).map_err(std::io::Error::other)?;
    writeln!(f, "{line}")
}

/// Read the last `days` days of monitor files and summarize them.
pub fn summarize(dir: &Path, days: i64) -> Option<MonitorSummary> {
    let now = Utc::now();
    let mut records = Vec::new();
    for d in 0..=days {
        let path = file_for(dir, now - ChronoDuration::days(d));
        let Ok(f) = std::fs::File::open(&path) else {
            continue;
        };
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            if let Ok(r) = serde_json::from_str::<MonitorRecord>(&line) {
                if now - r.t <= ChronoDuration::days(days) {
                    records.push(r);
                }
            }
        }
    }
    summarize_records(&records)
}

pub fn summarize_records(records: &[MonitorRecord]) -> Option<MonitorSummary> {
    if records.is_empty() {
        return None;
    }
    let mut s = MonitorSummary {
        available_hist: vec![0; AVAILABLE_BUCKETS],
        commit_hist: vec![0; COMMIT_BUCKETS],
        ..Default::default()
    };
    let mut sum_avail = 0.0;
    let mut sum_cpu = 0.0;
    for r in records {
        s.available_hist[MonitorSummary::bucket(r.available_percent, AVAILABLE_BUCKETS)] += 1;
        if let Some(c) = r.commit_percent {
            s.commit_hist[MonitorSummary::bucket(c, COMMIT_BUCKETS)] += 1;
        }
        sum_avail += r.available_percent;
        sum_cpu += r.cpu_percent;
        s.from = Some(s.from.map_or(r.t, |f| f.min(r.t)));
        s.to = Some(s.to.map_or(r.t, |t| t.max(r.t)));
    }
    s.sample_count = records.len() as u32;
    s.avg_available_percent = round1(sum_avail / records.len() as f64);
    s.avg_cpu_percent = round1(sum_cpu / records.len() as f64);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(avail: f64, commit: f64) -> MonitorRecord {
        MonitorRecord {
            t: Utc::now(),
            available_percent: avail,
            commit_percent: Some(commit),
            cpu_percent: 10.0,
        }
    }

    #[test]
    fn summary_histograms() {
        let recs = vec![
            rec(4.0, 95.0),
            rec(8.0, 50.0),
            rec(50.0, 40.0),
            rec(100.0, 120.0),
        ];
        let s = summarize_records(&recs).expect("summary");
        assert_eq!(s.sample_count, 4);
        assert!((s.share_available_below(10.0) - 0.5).abs() < 1e-9);
        assert!((s.share_commit_at_least(90.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn writes_and_reads_files() {
        let dir = std::env::temp_dir().join(format!("kosh-monitor-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        append(&dir, &rec(5.0, 80.0)).expect("append");
        append(&dir, &rec(50.0, 30.0)).expect("append");
        let s = summarize(&dir, 7).expect("summary");
        assert_eq!(s.sample_count, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
