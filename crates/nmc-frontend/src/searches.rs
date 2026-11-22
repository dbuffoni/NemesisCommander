//! Search conditions typed as text, and searches kept by name.

use std::time::{Duration, SystemTime};

use nmc_config::SavedSearch;
use nmc_index::{ContentQuery, Mask, SearchCriteria};
use nmc_panel::format::parse_size;

/// Parses the size and age conditions: words such as `>10M`, `<1G`,
/// `newer:7d`, `older:2w` (units `h`, `d`, `w`, `m` for 30 days, `y`), and
/// `archives` to search inside archive files too.
pub fn apply_filters(
    criteria: &mut SearchCriteria,
    text: &str,
    now: SystemTime,
) -> Result<(), String> {
    for word in text.split_whitespace() {
        if let Some(size) = word.strip_prefix('>') {
            criteria.min_size =
                Some(parse_size(size).ok_or_else(|| format!("Not a size: {size}"))?);
        } else if let Some(size) = word.strip_prefix('<') {
            criteria.max_size =
                Some(parse_size(size).ok_or_else(|| format!("Not a size: {size}"))?);
        } else if let Some(age) = word.strip_prefix("newer:") {
            criteria.modified_after = Some(now - parse_age(age)?);
        } else if let Some(age) = word.strip_prefix("older:") {
            criteria.modified_before = Some(now - parse_age(age)?);
        } else if word == "archives" {
            criteria.archives = true;
        } else {
            return Err(format!(
                "Not understood: {word} (use >10M, <1G, newer:7d, older:2w, archives)"
            ));
        }
    }
    Ok(())
}

fn parse_age(text: &str) -> Result<Duration, String> {
    let split = text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: u64 = number.parse().map_err(|_| format!("Not an age: {text}"))?;
    let hours = match unit {
        "h" => 1,
        "" | "d" => 24,
        "w" => 24 * 7,
        "m" => 24 * 30,
        "y" => 24 * 365,
        _ => return Err(format!("Not an age: {text} (use 12h, 7d, 2w, 3m, 1y)")),
    };
    Ok(Duration::from_secs(number * hours * 3600))
}

/// The conditions of a search: what the dialogs ask for.
pub fn criteria(
    mask: &str,
    content: &str,
    filters: &str,
    now: SystemTime,
) -> Result<SearchCriteria, String> {
    let mut criteria = SearchCriteria {
        name: Mask::parse(mask).map_err(|e| e.to_string())?,
        content: (!content.is_empty())
            .then(|| ContentQuery::parse(content, false))
            .transpose()
            .map_err(|e| format!("Not a regular expression: {e}"))?,
        ..SearchCriteria::default()
    };
    apply_filters(&mut criteria, filters, now)?;
    Ok(criteria)
}

/// The conditions of a saved search.
pub fn saved_criteria(search: &SavedSearch, now: SystemTime) -> Result<SearchCriteria, String> {
    criteria(&search.mask, &search.content, &search.filters, now)
}

/// One line describing a saved search, for the list.
pub fn describe(search: &SavedSearch) -> String {
    let mut parts = vec![if search.mask.is_empty() { "*".to_owned() } else { search.mask.clone() }];
    if !search.content.is_empty() {
        parts.push(format!("containing \"{}\"", search.content));
    }
    if !search.filters.is_empty() {
        parts.push(search.filters.clone());
    }
    format!("{}  ({})", search.name, parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_and_age_words() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 86_400);
        let c = criteria("*.log", "", ">10k <1M newer:7d older:1d", now).unwrap();
        assert_eq!(c.min_size, Some(10_240));
        assert_eq!(c.max_size, Some(1 << 20));
        assert_eq!(c.modified_after, Some(now - Duration::from_secs(7 * 86_400)));
        assert_eq!(c.modified_before, Some(now - Duration::from_secs(86_400)));
        assert!(c.content.is_none());
        assert!(!c.archives);
        assert!(criteria("*", "", "archives >1k", now).unwrap().archives);
    }

    #[test]
    fn names_and_contents_can_be_regular_expressions() {
        let now = SystemTime::now();
        let c = criteria(r"re:^img-\d+\.jpe?g$", r"re:TODO\(\w+\)", "", now).unwrap();
        assert!(c.name.matches("IMG-12.jpeg") && !c.name.matches("img-x.jpg"));
        assert!(c.content.is_some());
        assert!(criteria("*", "re:(", "", now).unwrap_err().contains("regular expression"));
        assert!(criteria("re:[", "", "", now).is_err());
    }

    #[test]
    fn mistakes_are_explained() {
        let now = SystemTime::now();
        assert!(criteria("*", "", ">lots", now).unwrap_err().contains("Not a size"));
        assert!(criteria("*", "", "newer:2q", now).unwrap_err().contains("Not an age"));
        assert!(criteria("*", "", "big", now).unwrap_err().contains("Not understood"));
    }

    #[test]
    fn saved_searches_are_described() {
        let search = SavedSearch {
            name: "big logs".into(),
            mask: "*.log".into(),
            content: "error".into(),
            filters: ">1M".into(),
        };
        assert_eq!(describe(&search), "big logs  (*.log, containing \"error\", >1M)");
        assert!(saved_criteria(&search, SystemTime::now()).unwrap().content.is_some());
    }
}
