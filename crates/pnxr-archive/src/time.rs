//! Conversions between archive timestamps and [`SystemTime`].

use std::time::{SystemTime, UNIX_EPOCH};

/// Builds a `SystemTime` from calendar fields on the wall clock of this
/// computer (zip's DOS times carry no zone).
pub fn from_local_civil(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<SystemTime> {
    let civil = jiff::civil::DateTime::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
        i8::try_from(hour).ok()?,
        i8::try_from(minute).ok()?,
        i8::try_from(second).ok()?,
        0,
    )
    .ok()?;
    let zoned = civil.to_zoned(jiff::tz::TimeZone::system()).ok()?;
    Some(SystemTime::from(zoned.timestamp()))
}

/// Splits a `SystemTime` into calendar fields of this computer's wall clock.
pub fn to_local_civil(time: SystemTime) -> Option<(i64, u32, u32, u32, u32, u32)> {
    let timestamp = jiff::Timestamp::try_from(time).ok()?;
    let c = timestamp.to_zoned(jiff::tz::TimeZone::system()).datetime();
    Some((
        i64::from(c.year()),
        c.month() as u32,
        c.day() as u32,
        c.hour() as u32,
        c.minute() as u32,
        c.second() as u32,
    ))
}

/// Seconds since the Unix epoch (0 for earlier times).
pub fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_wall_clock_round_trips() {
        let t = from_local_civil(2026, 11, 15, 12, 0, 0).unwrap();
        assert_eq!(to_local_civil(t), Some((2026, 11, 15, 12, 0, 0)));
        assert!(unix_seconds(t) > 1_794_000_000);
    }
}
