//! Small helpers around the `wmi` crate: connect, query, read typed values.

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use std::collections::HashMap;
use wmi::{Variant, WMIConnection};

pub type Row = HashMap<String, Variant>;

pub fn connect(namespace: &str) -> Result<WMIConnection, String> {
    if namespace.eq_ignore_ascii_case("root\\cimv2") {
        WMIConnection::new().map_err(|e| format!("WMI connect {namespace}: {e}"))
    } else {
        WMIConnection::with_namespace_path(namespace)
            .map_err(|e| format!("WMI connect {namespace}: {e}"))
    }
}

/// Run a WQL query. Access-denied errors are reported as `NeedsAdmin` by callers
/// via [`is_access_denied`].
pub fn query(con: &WMIConnection, wql: &str) -> Result<Vec<Row>, String> {
    con.raw_query::<Row>(wql)
        .map_err(|e| format!("WMI query `{wql}`: {e}"))
}

pub fn query_ns(namespace: &str, wql: &str) -> Result<Vec<Row>, String> {
    let con = connect(namespace)?;
    query(&con, wql)
}

pub fn is_access_denied(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("access denied") || m.contains("0x80041003") || m.contains("access is denied")
}

pub fn get_str(row: &Row, key: &str) -> Option<String> {
    match row.get(key)? {
        Variant::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Variant::Empty | Variant::Null => None,
        other => get_u64_variant(other).map(|n| n.to_string()),
    }
}

fn get_u64_variant(v: &Variant) -> Option<u64> {
    match v {
        Variant::I1(n) => u64::try_from(*n).ok(),
        Variant::I2(n) => u64::try_from(*n).ok(),
        Variant::I4(n) => u64::try_from(*n).ok(),
        Variant::I8(n) => u64::try_from(*n).ok(),
        Variant::UI1(n) => Some(u64::from(*n)),
        Variant::UI2(n) => Some(u64::from(*n)),
        Variant::UI4(n) => Some(u64::from(*n)),
        Variant::UI8(n) => Some(*n),
        Variant::R4(f) if *f >= 0.0 => Some(*f as u64),
        Variant::R8(f) if *f >= 0.0 => Some(*f as u64),
        // WMI returns uint64 as strings.
        Variant::String(s) => s.trim().parse().ok(),
        Variant::Bool(b) => Some(u64::from(*b)),
        _ => None,
    }
}

pub fn get_u64(row: &Row, key: &str) -> Option<u64> {
    get_u64_variant(row.get(key)?)
}

pub fn get_u32(row: &Row, key: &str) -> Option<u32> {
    get_u64(row, key).and_then(|n| u32::try_from(n).ok())
}

pub fn get_f64(row: &Row, key: &str) -> Option<f64> {
    match row.get(key)? {
        Variant::R4(f) => Some(f64::from(*f)),
        Variant::R8(f) => Some(*f),
        Variant::String(s) => s.trim().parse().ok(),
        other => get_u64_variant(other).map(|n| n as f64),
    }
}

pub fn get_bool(row: &Row, key: &str) -> Option<bool> {
    match row.get(key)? {
        Variant::Bool(b) => Some(*b),
        Variant::Empty | Variant::Null => None,
        other => get_u64_variant(other).map(|n| n != 0),
    }
}

/// WMI dates come as CIM_DATETIME strings ("20260708195124.000000+330", offset in minutes).
pub fn get_date(row: &Row, key: &str) -> Option<DateTime<Utc>> {
    let s = get_str(row, key)?;
    parse_cim_datetime(&s)
}

pub fn parse_cim_datetime(s: &str) -> Option<DateTime<Utc>> {
    if s.len() < 14 {
        return None;
    }
    let main = s.get(0..14)?;
    let naive = NaiveDateTime::parse_from_str(main, "%Y%m%d%H%M%S").ok()?;
    let offset_minutes: i32 = s
        .get(21..)
        .and_then(|o| {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            o.get(1..)?.trim().parse::<i32>().ok().map(|m| sign * m)
        })
        .unwrap_or(0);
    let tz = FixedOffset::east_opt(offset_minutes * 60)?;
    tz.from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cim_datetime_with_offset() {
        let d = parse_cim_datetime("20260708195124.000000+330").expect("date");
        assert_eq!(d.to_rfc3339(), "2026-07-08T14:21:24+00:00");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_cim_datetime("abc").is_none());
    }
}
