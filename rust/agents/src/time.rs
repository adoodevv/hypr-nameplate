//! Turning the timestamps the agents write into local calendar days.
//!
//! They use ISO 8601 strings ("2026-11-15T23:10:04.120Z", with an offset, or
//! naive local time) and epoch numbers in seconds, milliseconds or microseconds.

use nameplate::ffi;
use serde_json::Value;

/// A local day as "YYYY-MM-DD".
pub type Day = String;

/// (local day, epoch seconds) for a timestamp value, like the Python version's
/// `local_day`.
pub fn local_day(stamp: &Value) -> Option<(Day, f64)> {
    match stamp {
        Value::Number(n) => from_epoch(n.as_f64()?),
        Value::String(s) => {
            let t = s.trim();
            if is_digits(t) {
                from_epoch(t.parse().ok()?)
            } else {
                from_iso(t)
            }
        }
        _ => None,
    }
}

/// 9-20 digits with an optional fraction: an epoch number written as a string.
fn is_digits(s: &str) -> bool {
    let (int, frac) = s.split_once('.').unwrap_or((s, "0"));
    (9..=20).contains(&int.len())
        && int.bytes().all(|b| b.is_ascii_digit())
        && !frac.is_empty()
        && frac.bytes().all(|b| b.is_ascii_digit())
}

pub fn from_epoch(value: f64) -> Option<(Day, f64)> {
    // Also rejects NaN.
    if value.is_nan() || value <= 0.0 {
        return None;
    }
    let seconds = if value > 1e14 {
        value / 1e6
    } else if value > 1e11 {
        value / 1e3
    } else {
        value
    };
    Some((day_of(seconds)?, seconds))
}

pub fn day_of(seconds: f64) -> Option<Day> {
    let (y, m, d) = ffi::local_date(seconds.floor() as i64)?;
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Today, locally.
pub fn today() -> (i32, u32, u32) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    ffi::local_date(now.as_secs() as i64).unwrap_or((1970, 1, 1))
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day number from [`days_from_civil`].
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// What Python's `datetime.fromisoformat` accepts for these files: a date,
/// then optionally `T` or a space, a time with optional fraction, and an
/// optional `Z` or `±HH:MM` offset. Without an offset it's local time.
pub fn from_iso(s: &str) -> Option<(Day, f64)> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<u32> {
        let part = s.get(from..from + len)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let year = num(0, 4)? as i32;
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let (month, day) = (num(5, 2)?, num(8, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (mut hour, mut min, mut sec, mut frac) = (0, 0, 0, 0.0);
    let mut rest = &s[10..];
    if let Some(first) = rest.chars().next() {
        if first != 'T' && first != ' ' {
            return None;
        }
        let t = &rest[1..];
        hour = t.get(0..2)?.parse().ok()?;
        if t.as_bytes().get(2) != Some(&b':') {
            return None;
        }
        min = t.get(3..5)?.parse().ok()?;
        let mut i = 5;
        if t.as_bytes().get(5) == Some(&b':') {
            sec = t.get(6..8)?.parse().ok()?;
            i = 8;
            if t.as_bytes().get(8) == Some(&b'.') {
                let digits: String = t[9..].chars().take_while(char::is_ascii_digit).collect();
                if digits.is_empty() {
                    return None;
                }
                frac = format!("0.{digits}").parse().ok()?;
                i = 9 + digits.len();
            }
        }
        rest = &t[i..];
        if hour > 23 || min > 59 || sec > 59 {
            return None;
        }
    }
    let base = days_from_civil(year, month, day) * 86_400 + (hour * 3600 + min * 60 + sec) as i64;
    let epoch = match rest {
        "" => ffi::local_to_epoch(year, month, day, hour, min, sec)? as f64,
        "Z" | "z" => base as f64,
        off => {
            let sign = match off.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits = off[1..].replace(':', "");
            if digits.len() != 4 || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let h: i64 = digits[..2].parse().ok()?;
            let m: i64 = digits[2..].parse().ok()?;
            (base - sign * (h * 3600 + m * 60)) as f64
        }
    } + frac;
    Some((day_of(epoch)?, epoch))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn utc_env() {
        std::env::set_var("TZ", "UTC");
    }

    #[test]
    fn civil_round_trip() {
        for z in [-1000, 0, 1, 18_000, 20_418, 60_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(days_from_civil(2026, 11, 26), 20_783);
    }

    #[test]
    fn parses_the_formats_agents_write() {
        utc_env();
        let (day, t) = local_day(&json!("2026-11-15T23:10:04.500Z")).unwrap();
        assert_eq!(day, "2026-11-15");
        assert_eq!(t, 1_794_784_204.5);
        assert_eq!(
            local_day(&json!("2026-11-15T23:10:04+01:00")).unwrap().1,
            1_794_780_604.0
        );
        assert_eq!(
            local_day(&json!(1_794_784_204_000_i64)).unwrap().1,
            1_794_784_204.0
        ); // ms
        assert_eq!(
            local_day(&json!(1_794_784_204_000_000_i64)).unwrap().1,
            1_794_784_204.0
        ); // µs
        assert_eq!(local_day(&json!("1794784204")).unwrap().0, "2026-11-15");
        assert!(local_day(&json!("yesterday")).is_none());
        assert!(local_day(&json!(0)).is_none());
        assert!(local_day(&json!(null)).is_none());
    }
}
