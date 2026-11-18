//! Time arguments: `24h`, `7d`, `90m`, `now`, `2026-11-01`, `2026-11-01T10:00+01:00`.

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};

use crate::{Error, Result};

fn usage(input: &str) -> Error {
    Error::Usage(format!(
        "'{input}' is not a time. Use 24h, 7d, 90m, a date like 2026-11-01 or 2026-11-01T10:00+01:00"
    ))
}

/// Seconds of a relative span (`30s`, `5m`, `2h`, `7d`, `2w`; a bare number is seconds). Must be > 0.
pub fn parse_duration(input: &str) -> Result<i64> {
    let s = input.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: i64 = num.parse().map_err(|_| duration_usage(input))?;
    let factor = match unit {
        "" | "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 604_800,
        _ => return Err(duration_usage(input)),
    };
    if n <= 0 {
        return Err(duration_usage(input));
    }
    n.checked_mul(factor).ok_or_else(|| duration_usage(input))
}

fn duration_usage(input: &str) -> Error {
    Error::Usage(format!(
        "'{input}' is not a duration. Use 30s, 5m, 2h, 7d or 2w"
    ))
}

/// A point in time. Relative spans count back from `now`; dates and times without an offset are
/// read in `tz`.
pub fn parse_instant(input: &str, now: Timestamp, tz: &TimeZone) -> Result<Timestamp> {
    let s = input.trim();
    if s.is_empty() {
        return Err(usage(input));
    }
    if s.eq_ignore_ascii_case("now") {
        return Ok(now);
    }
    if s.as_bytes()[0].is_ascii_digit() && !s.contains('-') && !s.contains(':') {
        let secs = parse_duration(s).map_err(|_| usage(input))?;
        return now
            .checked_sub(SignedDuration::from_secs(secs))
            .map_err(|_| usage(input));
    }
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    if let Ok(dt) = s.parse::<DateTime>() {
        if s.contains('T') || s.contains(' ') {
            return dt
                .to_zoned(tz.clone())
                .map(|z| z.timestamp())
                .map_err(|_| usage(input));
        }
    }
    if let Ok(date) = s.parse::<Date>() {
        return date
            .to_zoned(tz.clone())
            .map(|z| z.timestamp())
            .map_err(|_| usage(input));
    }
    Err(usage(input))
}

/// Same with the system clock and zone.
pub fn parse_instant_now(input: &str) -> Result<Timestamp> {
    parse_instant(input, Timestamp::now(), &TimeZone::system())
}

/// `"45s"`, `"12m"`, `"3h"`, `"2d"` between two instants.
pub fn ago(then: Timestamp, now: Timestamp) -> String {
    let secs = (now.as_second() - then.as_second()).max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}
