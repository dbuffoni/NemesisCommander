//! Wildcard masks: `*.jpg;*.png`, `!*.bak`, `report-??.pdf`.

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Error for an unparsable mask.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid mask {mask:?}: {reason}")]
pub struct MaskError {
    /// The offending pattern.
    pub mask: String,
    /// Why it is invalid.
    pub reason: String,
}

/// A set of include and exclude wildcard patterns matched against file
/// names (not paths), case-insensitively.
///
/// Patterns are separated by `;` or `,`. A pattern starting with `!`
/// excludes. A mask with only exclusions includes everything else. `*`
/// matches any run of characters, `?` one character, `[abc]` a class.
/// An empty mask matches everything.
#[derive(Debug, Clone)]
pub struct Mask {
    source: String,
    include: Option<GlobSet>,
    exclude: Option<GlobSet>,
}

impl Mask {
    /// Parses a mask.
    ///
    /// ```
    /// let m = sc_index::Mask::parse("*.jpg; *.PNG; !thumb*").unwrap();
    /// assert!(m.matches("Holiday.JPG"));
    /// assert!(!m.matches("thumb_1.png"));
    /// assert!(!m.matches("notes.txt"));
    /// ```
    pub fn parse(source: &str) -> Result<Self, MaskError> {
        let mut include = GlobSetBuilder::new();
        let mut exclude = GlobSetBuilder::new();
        let (mut has_include, mut has_exclude) = (false, false);
        for raw in source.split([';', ',']).map(str::trim).filter(|s| !s.is_empty()) {
            let (pattern, excluded) = match raw.strip_prefix('!') {
                Some(rest) => (rest, true),
                None => (raw, false),
            };
            let glob = GlobBuilder::new(pattern)
                .case_insensitive(true)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|e| MaskError { mask: raw.to_owned(), reason: e.kind().to_string() })?;
            if excluded {
                exclude.add(glob);
                has_exclude = true;
            } else {
                include.add(glob);
                has_include = true;
            }
        }
        let build = |b: GlobSetBuilder, used: bool| {
            used.then(|| {
                b.build().map_err(|e| MaskError { mask: source.to_owned(), reason: e.to_string() })
            })
            .transpose()
        };
        Ok(Mask {
            source: source.to_owned(),
            include: build(include, has_include)?,
            exclude: build(exclude, has_exclude)?,
        })
    }

    /// A mask matching everything.
    pub fn all() -> Self {
        Mask { source: String::new(), include: None, exclude: None }
    }

    /// The text the mask was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// `true` if the mask matches everything.
    pub fn is_all(&self) -> bool {
        self.include.is_none() && self.exclude.is_none()
    }

    /// Does `name` match?
    pub fn matches(&self, name: &str) -> bool {
        let included = self.include.as_ref().is_none_or(|set| set.is_match(name));
        let excluded = self.exclude.as_ref().is_some_and(|set| set.is_match(name));
        included && !excluded
    }
}

impl Default for Mask {
    fn default() -> Self {
        Mask::all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_exclusion_only_masks() {
        assert!(Mask::parse("").unwrap().matches("anything"));
        assert!(Mask::parse(" ; ").unwrap().is_all());
        let m = Mask::parse("!*.bak").unwrap();
        assert!(m.matches("a.txt"));
        assert!(!m.matches("a.BAK"));
    }

    #[test]
    fn question_mark_and_classes() {
        let m = Mask::parse("img-??.[jp]*").unwrap();
        assert!(m.matches("img-01.jpg"));
        assert!(m.matches("IMG-ab.PNG"));
        assert!(!m.matches("img-1.jpg"));
        assert!(!m.matches("img-01.gif"));
    }

    #[test]
    fn star_does_not_cross_slashes() {
        // Masks match names; a slash can only appear in odd archive entries.
        assert!(!Mask::parse("*.txt").unwrap().matches("dir/a.txt"));
    }

    #[test]
    fn invalid_mask() {
        let err = Mask::parse("*.{rs").unwrap_err();
        assert_eq!(err.mask, "*.{rs");
    }
}
