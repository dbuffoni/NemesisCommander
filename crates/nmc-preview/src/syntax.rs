//! Syntax colours for source and data files.
//!
//! A small hand-written lexer, not a grammar: it knows comments, strings,
//! numbers, reserved words and a few names per language, which is what makes
//! a file readable at a glance. It keeps no tables of tokens and adds no
//! dependency. The state carried from line to line is only "inside a block
//! comment", "inside a multi-line string" or "inside a tag".

use crate::rich::{Line, Span, Style, Tone};

/// The languages that get colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Bash and other POSIX shells.
    Bash,
    /// YAML.
    Yaml,
    /// JSON.
    Json,
    /// XML and its relatives (SVG, plist, project files).
    Xml,
    /// HTML.
    Html,
    /// SQL.
    Sql,
    /// TypeScript, and JavaScript.
    TypeScript,
    /// Rust.
    Rust,
    /// Python.
    Python,
}

impl Language {
    /// The language of a file from its name, or from the first line when the
    /// name does not tell (`#!/bin/bash`).
    pub fn of(name: &str, first_line: &str) -> Option<Language> {
        let lower = name.to_lowercase();
        let extension = lower.rsplit_once('.').map_or("", |(_, e)| e);
        let by_name = match extension {
            "sh" | "bash" | "zsh" | "ksh" | "bashrc" | "profile" | "zshrc" => Language::Bash,
            "yml" | "yaml" => Language::Yaml,
            "json" | "jsonc" | "json5" | "geojson" | "webmanifest" => Language::Json,
            "xml" | "svg" | "xsd" | "xsl" | "xslt" | "plist" | "csproj" | "fsproj" | "vcxproj"
            | "props" | "targets" | "rss" | "atom" | "kml" | "gpx" | "ui" | "qrc" => Language::Xml,
            "html" | "htm" | "xhtml" => Language::Html,
            "sql" => Language::Sql,
            "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => Language::TypeScript,
            "rs" => Language::Rust,
            "py" | "pyw" | "pyi" => Language::Python,
            _ => match lower.as_str() {
                ".bashrc" | ".profile" | ".zshrc" | ".bash_profile" | "pkgbuild" => Language::Bash,
                _ => return Language::of_shebang(first_line),
            },
        };
        Some(by_name)
    }

    fn of_shebang(first_line: &str) -> Option<Language> {
        let line = first_line.strip_prefix("#!")?;
        let program = line.split_whitespace().map(|w| w.rsplit('/').next().unwrap_or(w));
        for word in program.take(2) {
            return Some(match word {
                "sh" | "bash" | "zsh" | "ksh" | "dash" => Language::Bash,
                w if w.starts_with("python") => Language::Python,
                "node" | "deno" | "bun" | "ts-node" => Language::TypeScript,
                "env" => continue,
                _ => return None,
            });
        }
        None
    }

    /// A short name for the language.
    pub fn label(self) -> &'static str {
        match self {
            Language::Bash => "Shell",
            Language::Yaml => "YAML",
            Language::Json => "JSON",
            Language::Xml => "XML",
            Language::Html => "HTML",
            Language::Sql => "SQL",
            Language::TypeScript => "TypeScript",
            Language::Rust => "Rust",
            Language::Python => "Python",
        }
    }

    fn config(self) -> Config {
        let base = Config {
            keywords: &[],
            ci: false,
            line_comments: &[],
            block: None,
            quotes: "\"'",
            multi: &[],
            types: false,
            keys: false,
            dollar: false,
            decorator: false,
            lifetimes: false,
            dashed: false,
        };
        match self {
            Language::Bash => {
                Config { keywords: BASH, line_comments: &["#"], dollar: true, dashed: true, ..base }
            }
            Language::Yaml => Config {
                keywords: &["true", "false", "null", "yes", "no", "on", "off", "~"],
                line_comments: &["#"],
                keys: true,
                dashed: true,
                ..base
            },
            Language::Json => Config {
                keywords: &["true", "false", "null"],
                quotes: "\"",
                keys: true,
                line_comments: &["//"],
                block: Some(("/*", "*/")),
                ..base
            },
            Language::Sql => Config {
                keywords: SQL,
                ci: true,
                line_comments: &["--"],
                block: Some(("/*", "*/")),
                quotes: "'\"`",
                ..base
            },
            Language::TypeScript => Config {
                keywords: TYPESCRIPT,
                line_comments: &["//"],
                block: Some(("/*", "*/")),
                quotes: "\"'",
                multi: &["`"],
                types: true,
                decorator: true,
                ..base
            },
            Language::Rust => Config {
                keywords: RUST,
                line_comments: &["//"],
                block: Some(("/*", "*/")),
                quotes: "\"'",
                types: true,
                lifetimes: true,
                ..base
            },
            Language::Python => Config {
                keywords: PYTHON,
                line_comments: &["#"],
                multi: &["\"\"\"", "'''"],
                types: true,
                decorator: true,
                ..base
            },
            // Markup has its own lexer.
            Language::Xml | Language::Html => base,
        }
    }

    fn is_markup(self) -> bool {
        matches!(self, Language::Xml | Language::Html)
    }
}

const BASH: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "return", "exit", "break", "continue", "local", "export",
    "readonly", "declare", "unset", "source", "alias", "set", "shift", "trap", "eval", "exec",
    "echo", "cd", "test",
];
const SQL: &[&str] = &[
    "select",
    "from",
    "where",
    "and",
    "or",
    "not",
    "in",
    "is",
    "null",
    "like",
    "between",
    "exists",
    "insert",
    "into",
    "values",
    "update",
    "set",
    "delete",
    "create",
    "alter",
    "drop",
    "table",
    "index",
    "view",
    "database",
    "schema",
    "primary",
    "key",
    "foreign",
    "references",
    "unique",
    "default",
    "check",
    "constraint",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "outer",
    "cross",
    "on",
    "using",
    "group",
    "by",
    "order",
    "having",
    "limit",
    "offset",
    "union",
    "all",
    "distinct",
    "as",
    "case",
    "when",
    "then",
    "else",
    "end",
    "asc",
    "desc",
    "begin",
    "commit",
    "rollback",
    "transaction",
    "with",
    "returning",
    "if",
    "true",
    "false",
    "int",
    "integer",
    "bigint",
    "smallint",
    "text",
    "varchar",
    "char",
    "boolean",
    "date",
    "timestamp",
    "numeric",
    "decimal",
    "float",
    "double",
    "serial",
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "cascade",
    "trigger",
    "function",
    "procedure",
    "grant",
    "revoke",
    "truncate",
    "explain",
    "pragma",
];
const TYPESCRIPT: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "declare",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "implements",
    "import",
    "in",
    "infer",
    "instanceof",
    "interface",
    "is",
    "keyof",
    "let",
    "module",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "of",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unknown",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize", "i8",
    "i16", "i32", "i64", "i128", "isize", "f32", "f64",
];
const PYTHON: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "self", "match", "case",
];

struct Config {
    keywords: &'static [&'static str],
    /// Keywords match whatever the case.
    ci: bool,
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    /// Characters that open a string ending on the same line.
    quotes: &'static str,
    /// Delimiters of strings that may continue on the next line.
    multi: &'static [&'static str],
    /// A capitalised word is a type.
    types: bool,
    /// A string or word followed by `:` is a key.
    keys: bool,
    /// `$name` is a variable.
    dollar: bool,
    /// `@name` is a decorator or annotation.
    decorator: bool,
    /// `'a` is a lifetime (and `'x'` a character).
    lifetimes: bool,
    /// Words may contain `-`.
    dashed: bool,
}

/// What a line starts inside of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    Block,
    Multi(&'static str),
    Tag,
}

/// Colours `lines` as `language`, one styled line for each.
pub fn highlight(language: Language, lines: &[String]) -> Vec<Line> {
    let mut state = State::Normal;
    lines
        .iter()
        .map(|line| {
            let (spans, next) = lex_line(language, line, state);
            state = next;
            spans
        })
        .collect()
}

/// Colours one line on its own (where a line's neighbours are not known).
pub fn highlight_line(language: Language, line: &str) -> Line {
    lex_line(language, line, State::Normal).0
}

fn lex_line(language: Language, line: &str, state: State) -> (Line, State) {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Out::default();
    let next = if language.is_markup() {
        lex_markup(&chars, state, &mut out)
    } else {
        lex_code(&language.config(), &chars, state, &mut out)
    };
    (out.finish(), next)
}

/// Spans being built; neighbours with the same tone are one span.
#[derive(Default)]
struct Out {
    spans: Vec<Span>,
}

impl Out {
    fn push(&mut self, chars: &[char], tone: Tone) {
        if chars.is_empty() {
            return;
        }
        let text: String = chars.iter().collect();
        match self.spans.last_mut() {
            Some(last) if last.style.tone == tone => last.text.push_str(&text),
            _ => self.spans.push(Span::styled(text, Style { tone, ..Style::default() })),
        }
    }

    fn finish(self) -> Line {
        self.spans
    }
}

fn starts_with(chars: &[char], at: usize, pattern: &str) -> bool {
    pattern.chars().zip(at..).all(|(c, index)| chars.get(index) == Some(&c))
}

/// The index just after `close` searching from `from`, if it is there.
fn find_after(chars: &[char], from: usize, close: &str) -> Option<usize> {
    let width = close.chars().count();
    (from..chars.len()).find(|&i| starts_with(chars, i, close)).map(|i| i + width)
}

/// The index after a quoted string starting at `start` (the quote), and
/// whether it was closed on this line.
fn string_end(chars: &[char], start: usize) -> (usize, bool) {
    let quote = chars[start];
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            c if c == quote => return (i + 1, true),
            _ => i += 1,
        }
    }
    (chars.len(), false)
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word(c: char, dashed: bool) -> bool {
    c.is_alphanumeric() || c == '_' || (dashed && c == '-')
}

/// Whether the next thing after `from`, spaces apart, is a `:`.
fn colon_follows(chars: &[char], from: usize) -> bool {
    let mut i = from;
    while chars.get(i) == Some(&' ') {
        i += 1;
    }
    chars.get(i) == Some(&':') && chars.get(i + 1) != Some(&':')
}

fn lex_code(config: &Config, chars: &[char], mut state: State, out: &mut Out) -> State {
    let mut i = 0;
    // Where the line left off inside something from the line before.
    match state {
        State::Block => {
            let close = config.block.map_or("", |b| b.1);
            match find_after(chars, 0, close) {
                Some(end) => {
                    out.push(&chars[..end], Tone::Comment);
                    i = end;
                    state = State::Normal;
                }
                None => {
                    out.push(chars, Tone::Comment);
                    return State::Block;
                }
            }
        }
        State::Multi(delimiter) => match find_closing(chars, 0, delimiter) {
            Some(end) => {
                out.push(&chars[..end], Tone::Str);
                i = end;
                state = State::Normal;
            }
            None => {
                out.push(chars, Tone::Str);
                return State::Multi(delimiter);
            }
        },
        _ => {}
    }
    let _ = state;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            let end =
                chars[i..].iter().position(|c| !c.is_whitespace()).map_or(chars.len(), |n| i + n);
            out.push(&chars[i..end], Tone::Plain);
            i = end;
            continue;
        }
        if let Some((open, close)) = config.block.filter(|(open, _)| starts_with(chars, i, open)) {
            match find_after(chars, i + open.chars().count(), close) {
                Some(end) => {
                    out.push(&chars[i..end], Tone::Comment);
                    i = end;
                    continue;
                }
                None => {
                    out.push(&chars[i..], Tone::Comment);
                    return State::Block;
                }
            }
        }
        // `#` starts a comment only at a word boundary (`$#`, `${#a}`).
        let boundary = i == 0 || chars[i - 1].is_whitespace();
        if config
            .line_comments
            .iter()
            .any(|m| starts_with(chars, i, m) && (*m != "#" || boundary || !config.dollar))
        {
            out.push(&chars[i..], Tone::Comment);
            return State::Normal;
        }
        if let Some(delimiter) = config.multi.iter().find(|d| starts_with(chars, i, d)).copied() {
            let from = i + delimiter.chars().count();
            match find_closing(chars, from, delimiter) {
                Some(end) => {
                    out.push(&chars[i..end], Tone::Str);
                    i = end;
                    continue;
                }
                None => {
                    out.push(&chars[i..], Tone::Str);
                    return State::Multi(delimiter);
                }
            }
        }
        if c == '\'' && config.lifetimes {
            let literal = chars.get(i + 1) == Some(&'\\') || chars.get(i + 2) == Some(&'\'');
            if !literal {
                let end = chars[i + 1..]
                    .iter()
                    .position(|c| !is_word(*c, false))
                    .map_or(chars.len(), |n| i + 1 + n);
                out.push(&chars[i..end], Tone::Name);
                i = end;
                continue;
            }
        }
        if config.quotes.contains(c) {
            let (end, _) = string_end(chars, i);
            let tone =
                if config.keys && colon_follows(chars, end) { Tone::Name } else { Tone::Str };
            out.push(&chars[i..end], tone);
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            let mut end = i;
            while end < chars.len()
                && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '.')
            {
                // `1..5` is two numbers around a range.
                if chars[end] == '.' && chars.get(end + 1) == Some(&'.') {
                    break;
                }
                end += 1;
            }
            out.push(&chars[i..end], Tone::Number);
            i = end;
            continue;
        }
        if c == '$' && config.dollar {
            let mut end = i + 1;
            if chars.get(end) == Some(&'{') {
                end = find_after(chars, end, "}").unwrap_or(chars.len());
            } else if chars.get(end).is_some_and(|c| is_word_start(*c)) {
                while end < chars.len() && is_word(chars[end], false) {
                    end += 1;
                }
            } else if chars.get(end).is_some_and(|c| "@*#?!$0123456789".contains(*c)) {
                end += 1;
            }
            out.push(&chars[i..end], if end > i + 1 { Tone::Name } else { Tone::Plain });
            i = end;
            continue;
        }
        if c == '@' && config.decorator && chars.get(i + 1).is_some_and(|c| is_word_start(*c)) {
            let mut end = i + 1;
            while end < chars.len() && (is_word(chars[end], false) || chars[end] == '.') {
                end += 1;
            }
            out.push(&chars[i..end], Tone::Name);
            i = end;
            continue;
        }
        if is_word_start(c) || (config.keys && config.dashed && c == '~') {
            let mut end = i + 1;
            while end < chars.len() && is_word(chars[end], config.dashed) {
                end += 1;
            }
            let word: String = chars[i..end].iter().collect();
            let keyword = if config.ci {
                let lower = word.to_lowercase();
                config.keywords.contains(&lower.as_str())
            } else {
                config.keywords.contains(&word.as_str())
            };
            let tone = if config.keys && colon_follows(chars, end) {
                Tone::Name
            } else if keyword {
                Tone::Keyword
            } else if config.types && c.is_uppercase() {
                Tone::Name
            } else {
                Tone::Plain
            };
            out.push(&chars[i..end], tone);
            i = end;
            continue;
        }
        out.push(&chars[i..=i], Tone::Plain);
        i += 1;
    }
    State::Normal
}

/// The index after `delimiter` searching from `from`, skipping escaped
/// characters.
fn find_closing(chars: &[char], from: usize, delimiter: &str) -> Option<usize> {
    let width = delimiter.chars().count();
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '\\' {
            i += 2;
        } else if starts_with(chars, i, delimiter) {
            return Some(i + width);
        } else {
            i += 1;
        }
    }
    None
}

/// XML and HTML: tags, attributes, values, comments and entities.
fn lex_markup(chars: &[char], mut state: State, out: &mut Out) -> State {
    let mut i = 0;
    while i < chars.len() {
        match state {
            State::Block => match find_after(chars, i, "-->") {
                Some(end) => {
                    out.push(&chars[i..end], Tone::Comment);
                    i = end;
                    state = State::Normal;
                }
                None => {
                    out.push(&chars[i..], Tone::Comment);
                    return State::Block;
                }
            },
            State::Tag => {
                let c = chars[i];
                if c == '>' {
                    out.push(&chars[i..=i], Tone::Plain);
                    state = State::Normal;
                    i += 1;
                } else if c == '"' || c == '\'' {
                    let (end, _) = string_end(chars, i);
                    out.push(&chars[i..end], Tone::Str);
                    i = end;
                } else if is_word_start(c) {
                    let mut end = i + 1;
                    while end < chars.len()
                        && (is_word(chars[end], true) || chars[end] == ':' || chars[end] == '.')
                    {
                        end += 1;
                    }
                    out.push(&chars[i..end], Tone::Name);
                    i = end;
                } else {
                    out.push(&chars[i..=i], Tone::Plain);
                    i += 1;
                }
            }
            _ => {
                if starts_with(chars, i, "<!--") {
                    state = State::Block;
                    out.push(&chars[i..i + 4], Tone::Comment);
                    i += 4;
                } else if chars[i] == '<' {
                    let mut end = i + 1;
                    while end < chars.len() && "/?!".contains(chars[end]) {
                        end += 1;
                    }
                    out.push(&chars[i..end], Tone::Plain);
                    let name_end = chars[end..]
                        .iter()
                        .position(|c| !(is_word(*c, true) || *c == ':' || *c == '.'))
                        .map_or(chars.len(), |n| end + n);
                    out.push(&chars[end..name_end], Tone::Keyword);
                    i = name_end;
                    state = State::Tag;
                } else if chars[i] == '&' {
                    let end = find_after(chars, i, ";").filter(|e| e - i <= 10).unwrap_or(i + 1);
                    out.push(&chars[i..end], if end > i + 1 { Tone::Number } else { Tone::Plain });
                    i = end;
                } else {
                    let end = chars[i..]
                        .iter()
                        .position(|c| *c == '<' || *c == '&')
                        .map_or(chars.len(), |n| i + n.max(1));
                    out.push(&chars[i..end], Tone::Plain);
                    i = end;
                }
            }
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The (text, tone) pairs of a line, without the plain ones merged.
    fn tones(language: Language, line: &str) -> Vec<(String, Tone)> {
        highlight_line(language, line).into_iter().map(|s| (s.text, s.style.tone)).collect()
    }

    fn has(language: Language, line: &str, text: &str, tone: Tone) -> bool {
        tones(language, line).iter().any(|(t, k)| t == text && *k == tone)
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn languages_come_from_names_and_shebangs() {
        assert_eq!(Language::of("main.rs", ""), Some(Language::Rust));
        assert_eq!(Language::of("App.TSX", ""), Some(Language::TypeScript));
        assert_eq!(Language::of("a.yml", ""), Some(Language::Yaml));
        assert_eq!(Language::of("deploy", "#!/usr/bin/env python3"), Some(Language::Python));
        assert_eq!(Language::of("run", "#!/bin/bash"), Some(Language::Bash));
        assert_eq!(Language::of("notes.txt", "hello"), None);
        assert_eq!(Language::of(".bashrc", ""), Some(Language::Bash));
    }

    #[test]
    fn rust_tokens() {
        let line = "fn main() { let x: u32 = 42; // go\n";
        assert!(has(Language::Rust, line, "fn", Tone::Keyword));
        assert!(has(Language::Rust, line, "42", Tone::Number));
        assert!(has(Language::Rust, line, "// go\n", Tone::Comment));
        assert!(has(Language::Rust, "let s = \"a\\\"b\";", "\"a\\\"b\"", Tone::Str));
        assert!(has(Language::Rust, "fn f<'a>(x: &'a str)", "'a", Tone::Name));
        assert!(has(Language::Rust, "let c = 'x';", "'x'", Tone::Str));
        assert!(has(Language::Rust, "struct Point;", "Point", Tone::Name));
    }

    #[test]
    fn block_comments_span_lines() {
        let out = highlight(Language::Rust, &lines("a /* one\ntwo\nthree */ b"));
        assert_eq!(out[1].len(), 1);
        assert_eq!(out[1][0].style.tone, Tone::Comment);
        assert_eq!(out[2][0].style.tone, Tone::Comment);
        assert_eq!(out[2].last().unwrap().style.tone, Tone::Plain);
    }

    #[test]
    fn python_triple_quotes_and_decorators() {
        let out =
            highlight(Language::Python, &lines("\"\"\"doc\nmore\"\"\"\n@cache\ndef f(): pass"));
        assert_eq!(out[0][0].style.tone, Tone::Str);
        assert_eq!(out[1][0].style.tone, Tone::Str);
        assert!(has(Language::Python, "@cache", "@cache", Tone::Name));
        assert!(has(Language::Python, "def f(): pass", "def", Tone::Keyword));
        assert!(has(Language::Python, "x = 1  # one", "# one", Tone::Comment));
    }

    #[test]
    fn bash_variables_and_comments() {
        assert!(has(Language::Bash, "echo \"$HOME\" # note", "# note", Tone::Comment));
        assert!(has(Language::Bash, "for f in ${list}; do", "${list}", Tone::Name));
        assert!(has(Language::Bash, "for f in ${list}; do", "for", Tone::Keyword));
        // `$#` is not a comment.
        assert!(!has(Language::Bash, "echo $#", "#", Tone::Comment));
        assert!(has(Language::Bash, "if [ -n 'x' ]; then", "'x'", Tone::Str));
    }

    #[test]
    fn json_keys_are_names_and_values_are_not() {
        let line = r#"  "port": 8080, "on": true, "name": "x""#;
        assert!(has(Language::Json, line, "\"port\"", Tone::Name));
        assert!(has(Language::Json, line, "8080", Tone::Number));
        assert!(has(Language::Json, line, "true", Tone::Keyword));
        assert!(has(Language::Json, line, "\"x\"", Tone::Str));
    }

    #[test]
    fn yaml_keys_values_and_comments() {
        assert!(has(Language::Yaml, "name: build # job", "name", Tone::Name));
        assert!(has(Language::Yaml, "name: build # job", "# job", Tone::Comment));
        assert!(has(Language::Yaml, "  - enabled: true", "true", Tone::Keyword));
        assert!(has(Language::Yaml, "  my-key: 3", "my-key", Tone::Name));
        assert!(has(Language::Yaml, "port: 80", "80", Tone::Number));
    }

    #[test]
    fn sql_keywords_ignore_case() {
        let line = "Select name FROM users WHERE id = 7 -- one";
        assert!(has(Language::Sql, line, "Select", Tone::Keyword));
        assert!(has(Language::Sql, line, "FROM", Tone::Keyword));
        assert!(has(Language::Sql, line, "7", Tone::Number));
        assert!(has(Language::Sql, line, "-- one", Tone::Comment));
        assert!(has(Language::Sql, "insert into t values ('a''b')", "'a''b'", Tone::Str));
    }

    #[test]
    fn typescript_templates_continue_on_the_next_line() {
        let out = highlight(
            Language::TypeScript,
            &lines("const a = `one\ntwo`;\ninterface Box<T> { v: T }"),
        );
        assert_eq!(out[1][0].style.tone, Tone::Str);
        assert!(out[0].iter().any(|s| s.text == "const" && s.style.tone == Tone::Keyword));
        assert!(has(Language::TypeScript, "interface Box<T> { v: T }", "Box", Tone::Name));
        assert!(has(Language::TypeScript, "let n: number = 1.5;", "number", Tone::Keyword));
    }

    #[test]
    fn markup_tags_attributes_and_comments() {
        let line = r#"<div class="a" id=b>text &amp; more</div>"#;
        assert!(has(Language::Html, line, "div", Tone::Keyword));
        assert!(has(Language::Html, line, "class", Tone::Name));
        assert!(has(Language::Html, line, "\"a\"", Tone::Str));
        assert!(has(Language::Html, line, "&amp;", Tone::Number));
        let out = highlight(Language::Xml, &lines("<!-- a\nb -->\n<x\n  y=\"1\"/>"));
        assert_eq!(out[0][0].style.tone, Tone::Comment);
        assert_eq!(out[1][0].style.tone, Tone::Comment);
        assert!(out[3].iter().any(|s| s.text == "y" && s.style.tone == Tone::Name));
        assert!(out[3].iter().any(|s| s.text == "\"1\"" && s.style.tone == Tone::Str));
    }

    #[test]
    fn the_text_is_never_changed() {
        let source = "fn \u{e8}()\t{ \"unterminated\n'x\n\n/* open";
        for language in [
            Language::Bash,
            Language::Yaml,
            Language::Json,
            Language::Xml,
            Language::Html,
            Language::Sql,
            Language::TypeScript,
            Language::Rust,
            Language::Python,
        ] {
            let input = lines(source);
            let out = highlight(language, &input);
            assert_eq!(out.len(), input.len());
            for (line, original) in out.iter().zip(&input) {
                let text: String = line.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(&text, original, "{language:?}");
            }
        }
    }
}
