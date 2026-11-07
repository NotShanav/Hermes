//! Calendar helpers without a date/time dependency. Timestamps are UTC microseconds; the viewer shows and
//! filters in local time given a UTC offset in minutes.

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// "YYYY-MM-DD" -> days since 1970-01-01 (validated).
pub fn parse_day(s: &str) -> Option<i64> {
    let mut it = s.trim().split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1970..=9999).contains(&y) {
        return None;
    }
    let days = days_from_civil(y, m, d);
    (civil_from_days(days) == (y, m, d)).then_some(days) // rejects 2025-02-30
}

const DAY_US: i64 = 86_400 * 1_000_000;

/// `today`, `yesterday`, `YYYY-MM-DD`, `A..B` (inclusive days) -> [from_us, to_us) in UTC microseconds, for
/// days defined in local time with the given offset. `all` -> None.
pub fn date_range(spec: &str, offset_min: i32, now_us: i64) -> Result<Option<(i64, i64)>, String> {
    let off = offset_min as i64 * 60 * 1_000_000;
    let today = (now_us + off).div_euclid(DAY_US);
    let day = |t: &str| -> Result<i64, String> {
        match t.trim() {
            "today" => Ok(today),
            "yesterday" => Ok(today - 1),
            other => parse_day(other).ok_or_else(|| format!("`{other}` is not a date (use YYYY-MM-DD, today, yesterday)")),
        }
    };
    let spec = spec.trim();
    if spec.is_empty() || spec == "all" {
        return Ok(None);
    }
    let (a, b) = match spec.split_once("..") {
        Some((a, b)) => (day(a)?, day(b)?),
        None => (day(spec)?, day(spec)?),
    };
    if b < a {
        return Err("the end date is before the start date".into());
    }
    Ok(Some((a * DAY_US - off, (b + 1) * DAY_US - off)))
}

pub fn fmt_offset(offset_min: i32) -> String {
    let a = offset_min.abs();
    format!("{}{:02}:{:02}", if offset_min < 0 { '-' } else { '+' }, a / 60, a % 60)
}

/// "YYYY-MM-DD HH:MM:SS" in the given offset.
pub fn fmt_time(ts_us: i64, offset_min: i32) -> String {
    let secs = (ts_us + offset_min as i64 * 60_000_000).div_euclid(1_000_000);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let r = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", r / 3600, r % 3600 / 60, r % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_roundtrip_and_validation() {
        assert_eq!(parse_day("1970-01-01"), Some(0));
        assert_eq!(parse_day("2000-02-29"), Some(11_016));
        assert_eq!(parse_day("2025-02-30"), None);
        assert_eq!(parse_day("2025-13-01"), None);
        assert_eq!(parse_day("nonsense"), None);
        for d in [-1000i64, 0, 365, 19_000, 40_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn formatting_with_offsets() {
        let t = 1_730_000_000_123_456; // 2024-10-27 03:33:20 UTC
        assert_eq!(fmt_time(t, 0), "2024-10-27 03:33:20");
        assert_eq!(fmt_time(t, 330), "2024-10-27 09:03:20");
        assert_eq!(fmt_time(t, -300), "2024-10-26 22:33:20");
        assert_eq!(fmt_offset(330), "+05:30");
        assert_eq!(fmt_offset(-300), "-05:00");
        assert_eq!(fmt_offset(0), "+00:00");
    }

    #[test]
    fn date_ranges_follow_the_local_offset() {
        let now = 1_730_000_000_000_000; // 2024-10-27 03:33 UTC = 09:03 local (+05:30)
        assert_eq!(date_range("all", 0, now).unwrap(), None);
        let (a, b) = date_range("2024-10-27", 330, now).unwrap().unwrap();
        assert_eq!(fmt_time(a, 330), "2024-10-27 00:00:00");
        assert_eq!(b - a, DAY_US);
        assert_eq!(date_range("today", 330, now).unwrap(), date_range("2024-10-27", 330, now).unwrap());
        assert_eq!(date_range("yesterday", 330, now).unwrap(), date_range("2024-10-26", 330, now).unwrap());
        // at UTC offset 0 "today" is still 10-27, but at -05:00 (22:33 on the 26th) it is 10-26
        assert_eq!(date_range("today", -300, now).unwrap(), date_range("2024-10-26", -300, now).unwrap());
        let (a, b) = date_range("2024-10-25..2024-10-27", 0, now).unwrap().unwrap();
        assert_eq!(b - a, 3 * DAY_US);
        assert!(date_range("2024-10-27..2024-10-25", 0, now).is_err());
        assert!(date_range("garbage", 0, now).is_err());
    }
}
