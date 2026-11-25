//! Comparing names across systems (ADR 0053). macOS file systems treat
//! `é` written as one character (NFC) and as `e` plus an accent (NFD) as the
//! same name, and macOS and Windows volumes usually ignore case; Linux and
//! most servers do neither. These pure functions say when two names would
//! be the same file *on the destination*.

use unicode_normalization::UnicodeNormalization;

/// How a destination volume compares names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameRules {
    /// `A` and `a` are the same name.
    pub ignore_case: bool,
    /// NFC and NFD spellings of a name are the same name.
    pub ignore_normalization: bool,
}

impl NameRules {
    /// Exact comparison (Linux, servers, archives).
    pub const EXACT: NameRules = NameRules { ignore_case: false, ignore_normalization: false };

    /// What a system's usual local volume does when it cannot be asked:
    /// Windows ignores case; macOS ignores case and normalization.
    pub fn usual() -> NameRules {
        if cfg!(windows) {
            NameRules { ignore_case: true, ignore_normalization: false }
        } else if cfg!(target_os = "macos") {
            NameRules { ignore_case: true, ignore_normalization: true }
        } else {
            NameRules::EXACT
        }
    }

    /// The key two names share when they are the same file under these rules.
    pub fn key(self, name: &str) -> String {
        let name: String =
            if self.ignore_normalization { name.nfc().collect() } else { name.to_owned() };
        if self.ignore_case { name.to_lowercase() } else { name }
    }

    /// Whether `a` and `b` are the same name under these rules.
    pub fn same(self, a: &str, b: &str) -> bool {
        a == b || self.key(a) == self.key(b)
    }
}

/// Pairs of names in `names` that collide under `rules` without being
/// identical: copying all of them into one folder would overwrite one with
/// another. Each pair is reported once, in input order.
pub fn clashes<'a>(
    names: impl IntoIterator<Item = &'a str>,
    rules: NameRules,
) -> Vec<(&'a str, &'a str)> {
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    let mut found = Vec::new();
    for name in names {
        match seen.get(&rules.key(name)) {
            Some(first) if *first != name => found.push((*first, name)),
            Some(_) => {}
            None => {
                seen.insert(rules.key(name), name);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const NFC: &str = "caf\u{e9}.txt";
    const NFD: &str = "cafe\u{301}.txt";

    #[test]
    fn exact_rules_keep_everything_apart() {
        assert!(!NameRules::EXACT.same("A.txt", "a.txt"));
        assert!(!NameRules::EXACT.same(NFC, NFD));
        assert!(NameRules::EXACT.same("a", "a"));
    }

    #[test]
    fn case_and_normalization_are_separate_choices() {
        let case = NameRules { ignore_case: true, ignore_normalization: false };
        assert!(case.same("README.md", "readme.MD"));
        assert!(!case.same(NFC, NFD));
        let mac = NameRules { ignore_case: true, ignore_normalization: true };
        assert!(mac.same(NFC, NFD));
        assert!(mac.same(&NFC.to_uppercase(), NFD));
        let norm = NameRules { ignore_case: false, ignore_normalization: true };
        assert!(norm.same(NFC, NFD));
        assert!(!norm.same("A", "a"));
    }

    #[test]
    fn clashes_are_found_in_a_batch() {
        let names = ["Report.txt", "a.txt", "report.TXT", NFC, "b", NFD, "a.txt"];
        let mac = NameRules { ignore_case: true, ignore_normalization: true };
        assert_eq!(clashes(names, mac), [("Report.txt", "report.TXT"), (NFC, NFD)]);
        assert!(clashes(names, NameRules::EXACT).is_empty());
    }
}
