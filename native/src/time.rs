//! Minimal time helpers without a date crate: ISO-8601 UTC stamps for files,
//! and a local-time label for the restore notice.

use std::time::{SystemTime, UNIX_EPOCH};

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Now as "YYYY-MM-DDTHH:MM:SS.mmmZ", the same shape JavaScript writes.
pub fn now_iso() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs() as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, now.subsec_millis())
}

fn parse_iso(text: &str) -> Option<i64> {
    let num = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    Some(days_from_civil(y, mo as u32, d as u32) * 86_400 + h * 3600 + mi * 60 + s)
}

/// "2026-11-04 23:01" in local time, or the raw text when it does not parse.
pub fn local_label(iso: &str) -> String {
    let Some(epoch) = parse_iso(iso) else { return iso.to_owned() };
    let Some(tm) = local_tm(epoch) else { return iso.to_owned() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

#[cfg(unix)]
fn local_tm(epoch: i64) -> Option<libc::tm> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time = epoch as libc::time_t;
    // SAFETY: localtime_r only writes into `tm`.
    (!unsafe { libc::localtime_r(&time, &mut tm) }.is_null()).then_some(tm)
}

#[cfg(windows)]
fn local_tm(epoch: i64) -> Option<libc::tm> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time = epoch as libc::time_t;
    // SAFETY: localtime_s only writes into `tm`; it returns 0 on success.
    (unsafe { libc::localtime_s(&mut tm, &time) } == 0).then_some(tm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trips() {
        let now = now_iso();
        assert_eq!(now.len(), 24);
        assert!(parse_iso(&now).is_some());
        assert_eq!(parse_iso("1970-01-02T00:00:00.000Z"), Some(86_400));
        assert_eq!(parse_iso("2026-11-04T14:01:07.000Z").map(|s| civil_from_days(s / 86_400)), Some((2026, 11, 4)));
        assert_eq!(local_label("garbage"), "garbage");
    }
}
