//! Memory pressure sampling with Windows performance counters (PDH).

use crate::{CollectResult, Ctx, Patch, Skip};
use kosh_core::MemorySample;
use std::time::Duration;
use windows::core::w;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// System-wide commit charge as a percentage of the commit limit.
pub fn commit_percent() -> Option<f64> {
    let s = memory_status()?;
    if s.ullTotalPageFile == 0 {
        return None;
    }
    let used = s.ullTotalPageFile.saturating_sub(s.ullAvailPageFile);
    Some(used as f64 * 100.0 / s.ullTotalPageFile as f64)
}

fn memory_status() -> Option<MEMORYSTATUSEX> {
    let mut s = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as the API requires.
    unsafe { GlobalMemoryStatusEx(&mut s).ok()? };
    Some(s)
}

struct Query {
    handle: PDH_HQUERY,
}

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: handle came from PdhOpenQueryW.
        unsafe {
            let _ = PdhCloseQuery(self.handle);
        }
    }
}

fn check(status: u32, what: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("{what} failed: PDH status 0x{status:08X}"))
    }
}

fn add_counter(q: &Query, path: windows::core::PCWSTR) -> Result<PDH_HCOUNTER, String> {
    let mut c = PDH_HCOUNTER::default();
    // SAFETY: valid query handle and output pointer.
    check(
        unsafe { PdhAddEnglishCounterW(q.handle, path, 0, &mut c) },
        "PdhAddEnglishCounterW",
    )?;
    Ok(c)
}

fn read(c: PDH_HCOUNTER) -> Option<f64> {
    let mut v = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: valid counter handle and output pointer.
    let status = unsafe { PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) };
    if status != 0 {
        return None;
    }
    // SAFETY: we asked for PDH_FMT_DOUBLE.
    Some(unsafe { v.Anonymous.doubleValue })
}

pub fn memory_pressure(ctx: &Ctx) -> CollectResult {
    let mut handle = PDH_HQUERY::default();
    // SAFETY: output pointer is valid.
    check(
        unsafe { PdhOpenQueryW(None, 0, &mut handle) },
        "PdhOpenQueryW",
    )?;
    let q = Query { handle };
    let available = add_counter(&q, w!("\\Memory\\Available MBytes"))?;
    let pages = add_counter(&q, w!("\\Memory\\Pages/sec")).ok();
    let committed = add_counter(&q, w!("\\Memory\\% Committed Bytes In Use")).ok();

    let total_mb = memory_status()
        .map(|s| s.ullTotalPhys / 1_048_576)
        .ok_or_else(|| Skip::Failed("GlobalMemoryStatusEx failed".into()))?;

    // Rates such as Pages/sec need a first collection before the first reading.
    // SAFETY: valid query handle.
    check(
        unsafe { PdhCollectQueryData(q.handle) },
        "PdhCollectQueryData",
    )?;
    let mut samples = Vec::new();
    for _ in 0..ctx.sample_secs.max(1) {
        std::thread::sleep(Duration::from_secs(1));
        // SAFETY: valid query handle.
        if unsafe { PdhCollectQueryData(q.handle) } != 0 {
            continue;
        }
        let Some(avail) = read(available) else {
            continue;
        };
        samples.push(MemorySample {
            available_mb: avail.max(0.0) as u64,
            total_mb,
            commit_percent: committed.and_then(read).map(round1),
            pages_per_sec: pages.and_then(read).map(round1),
        });
    }
    if samples.is_empty() {
        return Err(Skip::Failed(
            "no performance counter samples could be read".into(),
        ));
    }
    let patch: Patch = Box::new(move |s| {
        s.performance.sample_interval_secs = 1;
        s.performance.samples = samples;
    });
    Ok(patch)
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
