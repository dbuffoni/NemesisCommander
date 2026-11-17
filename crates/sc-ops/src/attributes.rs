//! Changing modification times and permissions of files and folders (the
//! `touch` and `chmod` of the application).

use std::time::SystemTime;

use sc_vfs::{EntryKind, ProviderRegistry, Result, SetAttributes, VfsError, VfsPath};

/// What to change, as typed by the user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeChange {
    /// The attributes to set.
    pub attributes: SetAttributes,
    /// Also change what is inside folders.
    pub recursive: bool,
}

/// Reads a change from text made of these words, in any order:
/// `now`, a date `2026-11-17`, a time `10:30` or `10:30:15` (local time),
/// `mode=644` (octal permissions) and `-r` (inside folders too).
/// A date alone means midnight, a time alone means today.
pub fn parse_change(text: &str, now: SystemTime) -> std::result::Result<AttributeChange, String> {
    use jiff::civil::{Date, Time};
    let mut change = AttributeChange::default();
    let (mut date, mut time): (Option<Date>, Option<Time>) = (None, None);
    let mut set_now = false;
    for word in text.split_whitespace() {
        if word == "now" {
            set_now = true;
        } else if word == "-r" {
            change.recursive = true;
        } else if let Some(mode) = word.strip_prefix("mode=") {
            let mode = u32::from_str_radix(mode, 8)
                .ok()
                .filter(|m| *m <= 0o7777)
                .ok_or_else(|| format!("Not an octal mode: {mode}"))?;
            change.attributes.permissions = Some(mode);
        } else if let Ok(d) = word.parse::<Date>() {
            date = Some(d);
        } else if let Ok(t) = word.parse::<Time>() {
            time = Some(t);
        } else {
            return Err(format!(
                "Not understood: {word} (use a date 2026-11-17, a time 10:30, now, mode=644, -r)"
            ));
        }
    }
    if set_now && (date.is_some() || time.is_some()) {
        return Err("Use either now or a date and time".into());
    }
    if set_now {
        change.attributes.modified = Some(now);
    } else if date.is_some() || time.is_some() {
        let zone = jiff::tz::TimeZone::system();
        let today = jiff::Timestamp::try_from(now)
            .map_err(|e| e.to_string())?
            .to_zoned(zone.clone())
            .date();
        let moment = date.unwrap_or(today).to_datetime(time.unwrap_or(Time::midnight()));
        let zoned = moment.to_zoned(zone).map_err(|e| e.to_string())?;
        change.attributes.modified = Some(SystemTime::from(zoned.timestamp()));
    }
    if change.attributes == SetAttributes::default() {
        return Err("Nothing to change: give a date, a time, now or mode=644".into());
    }
    Ok(change)
}

/// Applies `change` to `paths`; with `recursive`, to everything inside
/// folders too. Returns how many items were changed.
pub async fn apply_change(
    registry: &ProviderRegistry,
    paths: &[VfsPath],
    change: &AttributeChange,
) -> Result<usize> {
    let mut count = 0;
    let mut todo: Vec<VfsPath> = paths.to_vec();
    while let Some(path) = todo.pop() {
        let provider = registry.provider_for(&path).await?;
        let entry = provider.stat(&path).await?;
        // A symbolic link keeps its target untouched unless asked directly.
        provider.set_attributes(&path, &change.attributes).await?;
        count += 1;
        if change.recursive && entry.kind == EntryKind::Directory {
            for child in sc_vfs::collect_listing(provider.as_ref(), &path).await? {
                if child.name != ".." && child.name != "." {
                    todo.push(path.join(&child.name));
                }
            }
        }
    }
    if count == 0 {
        return Err(VfsError::Other("Nothing to change".into()));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_a_change() {
        let now = SystemTime::now();
        let c = parse_change("2026-11-17 10:30 mode=640 -r", now).unwrap();
        assert!(c.recursive);
        assert_eq!(c.attributes.permissions, Some(0o640));
        assert!(c.attributes.modified.is_some());
        assert_eq!(parse_change("now", now).unwrap().attributes.modified, Some(now));
        assert!(parse_change("", now).is_err());
        assert!(parse_change("now 2026-11-17", now).is_err());
        assert!(parse_change("mode=999", now).is_err());
        assert!(parse_change("tomorrow", now).is_err());
        let date_only = parse_change("2026-11-17", now).unwrap().attributes.modified.unwrap();
        let with_time = parse_change("2026-11-17 01:00", now).unwrap().attributes.modified.unwrap();
        assert_eq!(with_time.duration_since(date_only).unwrap().as_secs(), 3600);
    }
}
