//! Human-friendly formatting of sizes, dates and permissions.

use std::time::SystemTime;

/// `1536` → `"1.5 KiB"`. Exact bytes below 1 KiB.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// Modification time in the local time zone, `YYYY-MM-DD HH:MM`.
pub fn local_time(time: SystemTime) -> String {
    jiff::Timestamp::try_from(time)
        .map(|ts| ts.to_zoned(jiff::tz::TimeZone::system()).strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| "????-??-?? ??:??".into())
}

/// `0o755` → `"rwxr-xr-x"` (with `s`/`t` for setuid/setgid/sticky).
pub fn permissions(mode: u32) -> String {
    let bit = |mask: u32, c: char| if mode & mask != 0 { c } else { '-' };
    let special = |exec: u32, special: u32, set: char| match (mode & exec != 0, mode & special != 0)
    {
        (true, true) => set,
        (false, true) => set.to_ascii_uppercase(),
        (true, false) => 'x',
        (false, false) => '-',
    };
    [
        bit(0o400, 'r'),
        bit(0o200, 'w'),
        special(0o100, 0o4000, 's'),
        bit(0o040, 'r'),
        bit(0o020, 'w'),
        special(0o010, 0o2000, 's'),
        bit(0o004, 'r'),
        bit(0o002, 'w'),
        special(0o001, 0o1000, 't'),
    ]
    .iter()
    .collect()
}

/// Parses sizes typed by users: `1500`, `10k`, `2.5M`, `1G` (binary units).
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.trim().parse().ok()?;
    let multiplier =
        match unit.trim().to_ascii_lowercase().trim_end_matches("ib").trim_end_matches('b') {
            "" => 1u64,
            "k" => 1 << 10,
            "m" => 1 << 20,
            "g" => 1 << 30,
            "t" => 1 << 40,
            _ => return None,
        };
    (number >= 0.0).then(|| (number * multiplier as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(50 * 1024 * 1024), "50 MiB");
        assert_eq!(parse_size("10k"), Some(10_240));
        assert_eq!(parse_size("2.5 MiB"), Some(2_621_440));
        assert_eq!(parse_size("1G"), Some(1 << 30));
        assert_eq!(parse_size("12"), Some(12));
        assert_eq!(parse_size("lots"), None);
    }

    #[test]
    fn modes() {
        assert_eq!(permissions(0o755), "rwxr-xr-x");
        assert_eq!(permissions(0o640), "rw-r-----");
        assert_eq!(permissions(0o4755), "rwsr-xr-x");
        assert_eq!(permissions(0o1777), "rwxrwxrwt");
        assert_eq!(permissions(0o2644), "rw-r-Sr--");
    }

    #[test]
    fn time_has_the_expected_shape() {
        let text =
            local_time(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000));
        assert_eq!(text.len(), 16, "{text}");
        assert_eq!(&text[4..5], "-");
    }
}
