//! Multi-rename: compute new names for many files from one rule.
//!
//! # Template syntax
//!
//! | Placeholder | Meaning                                            |
//! |-------------|----------------------------------------------------|
//! | `[N]`       | name without extension                             |
//! | `[N2-5]`    | characters 2 to 5 of the name (1-based, inclusive) |
//! | `[N3-]`     | from character 3 to the end                        |
//! | `[E]`       | extension without the dot                          |
//! | `[X]`       | extension with the dot, or nothing                 |
//! | `[C]`       | counter (see [`RenameRule::counter_start`] etc.)   |
//! | `[P]`       | name of the parent directory                       |
//! | `[[`, `]]`  | literal `[` and `]`                                 |
//!
//! After the template, an optional find/replace (literal or regex) runs on
//! the result, then an optional case change. The default rule
//! (`[N][X]`, no replace, no case change) leaves names untouched.

use std::collections::HashSet;

use pnxr_vfs::{ProviderRegistry, Result, VfsError, VfsPath};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Change of letter case applied last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CaseChange {
    /// Keep as is.
    #[default]
    Unchanged,
    /// all lowercase
    Lower,
    /// ALL UPPERCASE
    Upper,
    /// First letter of each word uppercase.
    Title,
}

/// A complete multi-rename rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameRule {
    /// Name template, see the [module documentation](self).
    pub template: String,
    /// Text or pattern to find in the templated name.
    pub find: String,
    /// Replacement (may use `$1` groups when `regex` is set).
    pub replace: String,
    /// Treat `find` as a regular expression.
    pub regex: bool,
    /// Case change applied at the end.
    pub case: CaseChange,
    /// First counter value.
    pub counter_start: u64,
    /// Counter increment.
    pub counter_step: u64,
    /// Minimum number of digits (zero padded).
    pub counter_width: usize,
}

impl Default for RenameRule {
    fn default() -> Self {
        RenameRule {
            template: "[N][X]".into(),
            find: String::new(),
            replace: String::new(),
            regex: false,
            case: CaseChange::Unchanged,
            counter_start: 1,
            counter_step: 1,
            counter_width: 1,
        }
    }
}

/// Why a rule cannot be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenameError {
    /// The template has an unknown or unterminated placeholder.
    #[error("invalid template: {0}")]
    Template(String),
    /// The find pattern is not a valid regular expression.
    #[error("invalid regular expression: {0}")]
    Regex(String),
    /// A resulting name is empty, `.`/`..`, or contains a `/`.
    #[error("invalid resulting name {0:?}")]
    InvalidName(String),
    /// Two files would get the same name.
    #[error("two files would be named {0:?}")]
    Duplicate(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Literal(String),
    Name { from: usize, to: Option<usize> },
    Extension { with_dot: bool },
    Counter,
    Parent,
}

fn parse_template(template: &str) -> Result<Vec<Token>, RenameError> {
    let mut tokens = Vec::new();
    let mut literal = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' if chars.peek() == Some(&'[') => {
                chars.next();
                literal.push('[');
            }
            ']' if chars.peek() == Some(&']') => {
                chars.next();
                literal.push(']');
            }
            '[' => {
                let body: String = chars.by_ref().take_while(|&c| c != ']').collect();
                if !literal.is_empty() {
                    tokens.push(Token::Literal(std::mem::take(&mut literal)));
                }
                tokens.push(parse_placeholder(&body)?);
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        tokens.push(Token::Literal(literal));
    }
    Ok(tokens)
}

fn parse_placeholder(body: &str) -> Result<Token, RenameError> {
    let bad = || RenameError::Template(format!("[{body}]"));
    Ok(match body {
        "N" => Token::Name { from: 1, to: None },
        "E" => Token::Extension { with_dot: false },
        "X" => Token::Extension { with_dot: true },
        "C" => Token::Counter,
        "P" => Token::Parent,
        _ if body.starts_with('N') => {
            let range = &body[1..];
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            let from: usize = from.parse().map_err(|_| bad())?;
            let to = if to.is_empty() { None } else { Some(to.parse().map_err(|_| bad())?) };
            if from == 0 || to.is_some_and(|t| t < from) {
                return Err(bad());
            }
            Token::Name { from, to }
        }
        _ => return Err(bad()),
    })
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.').filter(|&i| i > 0 && i + 1 < name.len()) {
        Some(i) => (&name[..i], &name[i + 1..]),
        None => (name, ""),
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start_of_word = true;
    for c in s.chars() {
        if start_of_word {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
        start_of_word = !c.is_alphanumeric();
    }
    out
}

/// A compiled [`RenameRule`], ready to produce names.
struct Compiled<'a> {
    rule: &'a RenameRule,
    tokens: Vec<Token>,
    regex: Option<Regex>,
}

impl<'a> Compiled<'a> {
    fn new(rule: &'a RenameRule) -> Result<Self, RenameError> {
        let tokens = parse_template(&rule.template)?;
        let regex = if rule.find.is_empty() {
            None
        } else if rule.regex {
            Some(Regex::new(&rule.find).map_err(|e| RenameError::Regex(e.to_string()))?)
        } else {
            Some(Regex::new(&regex::escape(&rule.find)).expect("escaped literal is valid"))
        };
        Ok(Compiled { rule, tokens, regex })
    }

    fn apply(&self, name: &str, parent: &str, index: u64) -> String {
        let (stem, ext) = split_extension(name);
        let mut out = String::new();
        for token in &self.tokens {
            match token {
                Token::Literal(s) => out.push_str(s),
                Token::Name { from: 1, to: None } => out.push_str(stem),
                Token::Name { from, to } => {
                    let take = to.map_or(usize::MAX, |t| t - from + 1);
                    out.extend(stem.chars().skip(from - 1).take(take));
                }
                Token::Extension { with_dot } => {
                    if !ext.is_empty() {
                        if *with_dot {
                            out.push('.');
                        }
                        out.push_str(ext);
                    }
                }
                Token::Counter => {
                    let value = self.rule.counter_start + index * self.rule.counter_step;
                    out.push_str(&format!("{value:0width$}", width = self.rule.counter_width));
                }
                Token::Parent => out.push_str(parent),
            }
        }
        if let Some(regex) = &self.regex {
            let replacement = if self.rule.regex {
                self.rule.replace.clone()
            } else {
                // A literal replacement must not interpret `$`.
                self.rule.replace.replace('$', "$$")
            };
            out = regex.replace_all(&out, replacement.as_str()).into_owned();
        }
        match self.rule.case {
            CaseChange::Unchanged => out,
            CaseChange::Lower => out.to_lowercase(),
            CaseChange::Upper => out.to_uppercase(),
            CaseChange::Title => title_case(&out),
        }
    }
}

/// Computes the new name of every path, in order, and checks the result
/// for invalid and duplicate names. Nothing is renamed.
pub fn preview_renames(paths: &[VfsPath], rule: &RenameRule) -> Result<Vec<String>, RenameError> {
    let compiled = Compiled::new(rule)?;
    let mut seen = HashSet::new();
    let mut names = Vec::with_capacity(paths.len());
    for (index, path) in paths.iter().enumerate() {
        let name = path.file_name().unwrap_or_default();
        let parent =
            path.parent().and_then(|p| p.file_name().map(str::to_owned)).unwrap_or_default();
        let new_name = compiled.apply(name, &parent, index as u64);
        if new_name.is_empty() || new_name == "." || new_name == ".." || new_name.contains('/') {
            return Err(RenameError::InvalidName(new_name));
        }
        let parent_key = path.parent().map(|p| p.to_string()).unwrap_or_default();
        if !seen.insert((parent_key, new_name.clone())) {
            return Err(RenameError::Duplicate(new_name));
        }
        names.push(new_name);
    }
    Ok(names)
}

/// The path `from` gets with its new `name` (same folder).
pub fn renamed(from: &VfsPath, name: &str) -> VfsPath {
    from.parent().unwrap_or_else(|| from.clone()).join(name)
}

/// The new paths that already belong to a file outside the rename: renaming
/// would replace it, so [`apply_renames`] refuses. A preview shows them.
///
/// Files of the selection that move away themselves (swaps, chains) and
/// letter-case-only changes are not conflicts.
pub async fn rename_conflicts(
    registry: &ProviderRegistry,
    paths: &[VfsPath],
    names: &[String],
) -> Result<Vec<VfsPath>> {
    let sources: HashSet<&VfsPath> = paths.iter().collect();
    let mut taken = Vec::new();
    for (from, name) in paths.iter().zip(names) {
        let to = renamed(from, name);
        if &to == from || sources.contains(&to) || is_case_change(from, &to) {
            continue;
        }
        match registry.provider_for(&to).await?.stat(&to).await {
            Ok(_) => taken.push(to),
            Err(e) if e.is_not_found() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(taken)
}

/// Renames each path to the matching name from [`preview_renames`].
///
/// Nothing is renamed when a new name belongs to another file (see
/// [`rename_conflicts`]). Swaps and chains (`a→b`, `b→a`) are handled by
/// first moving every affected file to a temporary name. Unchanged names
/// are skipped.
pub async fn apply_renames(
    registry: &ProviderRegistry,
    paths: &[VfsPath],
    names: &[String],
) -> Result<()> {
    if paths.len() != names.len() {
        return Err(VfsError::Other("number of names does not match number of paths".into()));
    }
    if let Some(taken) = rename_conflicts(registry, paths, names).await?.into_iter().next() {
        return Err(VfsError::AlreadyExists(taken));
    }
    let moves: Vec<(VfsPath, VfsPath)> = paths
        .iter()
        .zip(names)
        .map(|(from, name)| (from.clone(), renamed(from, name)))
        .filter(|(from, to)| from != to)
        .collect();
    let sources: HashSet<&VfsPath> = moves.iter().map(|(from, _)| from).collect();
    // A change of letter case only (`a.txt` → `A.TXT`) also goes through a
    // temporary name: on case-insensitive file systems (Windows, macOS) the
    // new name "exists" because it is the file itself.
    let needs_two_phases =
        moves.iter().any(|(from, to)| sources.contains(to) || is_case_change(from, to));

    if !needs_two_phases {
        for (from, to) in &moves {
            rename_no_clobber(registry, from, to).await?;
        }
        return Ok(());
    }
    let mut staged = Vec::with_capacity(moves.len());
    for (i, (from, to)) in moves.iter().enumerate() {
        let temp = from
            .parent()
            .unwrap_or_else(|| from.clone())
            .join(&format!(".pnxr-rename-{i}-{}", std::process::id()));
        rename_no_clobber(registry, from, &temp).await?;
        staged.push((temp, to));
    }
    for (temp, to) in staged {
        rename_no_clobber(registry, &temp, to).await?;
    }
    Ok(())
}

/// `true` when `to` is `from` with only the letter case changed.
fn is_case_change(from: &VfsPath, to: &VfsPath) -> bool {
    from.parent() == to.parent()
        && match (from.file_name(), to.file_name()) {
            (Some(a), Some(b)) => a != b && a.to_lowercase() == b.to_lowercase(),
            _ => false,
        }
}

/// Renames, refusing to replace an existing file.
async fn rename_no_clobber(
    registry: &ProviderRegistry,
    from: &VfsPath,
    to: &VfsPath,
) -> Result<()> {
    let provider = registry.provider_for(from).await?;
    match provider.stat(to).await {
        Ok(_) => return Err(VfsError::AlreadyExists(to.clone())),
        Err(e) if e.is_not_found() => {}
        Err(e) => return Err(e),
    }
    provider.rename(from, to).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnxr_providers::memory::MemoryProvider;
    use std::sync::Arc;

    fn paths(names: &[&str]) -> Vec<VfsPath> {
        names.iter().map(|n| VfsPath::parse(&format!("mem://m/dir/{n}")).unwrap()).collect()
    }

    #[test]
    fn case_changes_are_detected() {
        let p = |s: &str| VfsPath::parse(&format!("mem://m/dir/{s}")).unwrap();
        assert!(is_case_change(&p("a.txt"), &p("A.TXT")));
        assert!(!is_case_change(&p("a.txt"), &p("b.txt")));
        assert!(!is_case_change(&p("a.txt"), &p("a.txt")));
        assert!(!is_case_change(&p("a.txt"), &VfsPath::parse("mem://m/other/A.txt").unwrap()));
    }

    fn preview(names: &[&str], rule: RenameRule) -> Result<Vec<String>, RenameError> {
        preview_renames(&paths(names), &rule)
    }

    #[test]
    fn default_rule_keeps_names() {
        let names = ["a.txt", "README", ".hidden", "x.tar.gz"];
        assert_eq!(preview(&names, RenameRule::default()).unwrap(), names);
    }

    #[test]
    fn counters_and_extensions() {
        let rule = RenameRule {
            template: "photo_[C].[E]".into(),
            counter_start: 9,
            counter_width: 3,
            ..Default::default()
        };
        assert_eq!(preview(&["a.JPG", "b.jpg"], rule).unwrap(), ["photo_009.JPG", "photo_010.jpg"]);
    }

    #[test]
    fn name_ranges_and_parent() {
        let rule = RenameRule { template: "[P]-[N2-3][N5-][X]".into(), ..Default::default() };
        assert_eq!(preview(&["abcdefg.txt"], rule).unwrap(), ["dir-bcefg.txt"]);
    }

    #[test]
    fn literal_and_regex_replace() {
        let literal = RenameRule {
            find: ".".into(),
            replace: "$_".into(),
            template: "[N]".into(),
            ..Default::default()
        };
        assert_eq!(preview(&["a.b.c"], literal).unwrap(), ["a$_b"]);
        let regex = RenameRule {
            find: r"(\d+)-(\d+)".into(),
            replace: "$2-$1".into(),
            regex: true,
            ..Default::default()
        };
        assert_eq!(preview(&["img 12-34.png"], regex).unwrap(), ["img 34-12.png"]);
    }

    #[test]
    fn case_changes() {
        let rule = |case| RenameRule { case, ..Default::default() };
        assert_eq!(preview(&["My File.TXT"], rule(CaseChange::Lower)).unwrap(), ["my file.txt"]);
        assert_eq!(preview(&["My File.TXT"], rule(CaseChange::Upper)).unwrap(), ["MY FILE.TXT"]);
        assert_eq!(
            preview(&["my fILE-name.txt"], rule(CaseChange::Title)).unwrap(),
            ["My File-Name.Txt"]
        );
    }

    #[test]
    fn escaped_brackets() {
        let rule = RenameRule { template: "[[[N]]][X]".into(), ..Default::default() };
        assert_eq!(preview(&["a.txt"], rule).unwrap(), ["[a].txt"]);
    }

    #[test]
    fn errors() {
        let bad =
            |t: &str| preview(&["a"], RenameRule { template: t.into(), ..Default::default() });
        assert!(matches!(bad("[Q]"), Err(RenameError::Template(_))));
        assert!(matches!(bad("[N0-2]"), Err(RenameError::Template(_))));
        assert!(matches!(bad("[N3-1]"), Err(RenameError::Template(_))));
        assert!(matches!(bad("x/y"), Err(RenameError::InvalidName(_))));
        assert!(matches!(bad(""), Err(RenameError::InvalidName(_))));
        assert!(matches!(
            preview(&["a", "b"], RenameRule { template: "same".into(), ..Default::default() }),
            Err(RenameError::Duplicate(_))
        ));
        let regex = RenameRule { find: "(".into(), regex: true, ..Default::default() };
        assert!(matches!(preview(&["a"], regex), Err(RenameError::Regex(_))));
    }

    #[tokio::test]
    async fn apply_handles_swaps() {
        let registry = ProviderRegistry::new();
        let mem = MemoryProvider::new();
        mem.put_file("/dir/a", "A");
        mem.put_file("/dir/b", "B");
        registry.insert(&VfsPath::parse("mem://m/").unwrap(), Arc::new(mem.clone()));
        let files = paths(&["a", "b"]);
        apply_renames(&registry, &files, &["b".into(), "a".into()]).await.unwrap();
        assert_eq!(mem.get_file("/dir/a").unwrap(), b"B");
        assert_eq!(mem.get_file("/dir/b").unwrap(), b"A");
        assert_eq!(mem.paths(), ["/", "/dir", "/dir/a", "/dir/b"]);
    }

    #[tokio::test]
    async fn apply_refuses_to_clobber_outsiders() {
        let registry = ProviderRegistry::new();
        let mem = MemoryProvider::new();
        mem.put_file("/dir/a", "A");
        mem.put_file("/dir/other", "O");
        registry.insert(&VfsPath::parse("mem://m/").unwrap(), Arc::new(mem.clone()));
        let err = apply_renames(&registry, &paths(&["a"]), &["other".into()]).await.unwrap_err();
        assert!(matches!(err, VfsError::AlreadyExists(_)));
        assert_eq!(mem.get_file("/dir/other").unwrap(), b"O");
    }

    #[tokio::test]
    async fn conflicts_are_found_before_anything_is_renamed() {
        let registry = ProviderRegistry::new();
        let mem = MemoryProvider::new();
        for name in ["a", "b", "c", "taken"] {
            mem.put_file(&format!("/dir/{name}"), name);
        }
        registry.insert(&VfsPath::parse("mem://m/").unwrap(), Arc::new(mem.clone()));
        let files = paths(&["a", "b", "c"]);
        // a→b and b→a are a swap; c→taken would replace another file.
        let names = ["b".to_owned(), "a".to_owned(), "taken".to_owned()];
        let conflicts = rename_conflicts(&registry, &files, &names).await.unwrap();
        assert_eq!(conflicts, paths(&["taken"]));
        assert!(apply_renames(&registry, &files, &names).await.is_err());
        assert_eq!(mem.get_file("/dir/a").unwrap(), b"a", "nothing renamed");
        assert_eq!(mem.get_file("/dir/taken").unwrap(), b"taken");
    }
}
