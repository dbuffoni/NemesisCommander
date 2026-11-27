//! The multi-rename question: the text typed is a template, optionally
//! followed by options separated by ` | `
//! (`photo-[C] | width=3 | lower | s/ /_/`). This module reads that text,
//! lists ready-made rules, explains the syntax and builds the live preview.

use nmc_ops::{CaseChange, RenameRule};

/// Ready-made rules: what they do, and the text for the field.
pub const PRESETS: [(&str, &str); 8] = [
    ("Keep the names", "[N][X]"),
    ("Number the files", "[N]-[C] | width=3"),
    ("Only numbers", "[C][X] | width=3"),
    ("Lowercase", "[N][X] | lower"),
    ("Uppercase", "[N][X] | upper"),
    ("Spaces to underscores", "[N][X] | s/ /_/"),
    ("Prefix with the folder name", "[P]-[N][X]"),
    ("Remove digits (regular expression)", "[N][X] | re/[0-9]+//"),
];

/// Reads the text of the field.
pub fn parse_rule(text: &str) -> Result<RenameRule, String> {
    let mut parts = text.split(" | ");
    let mut rule = RenameRule {
        template: parts.next().unwrap_or_default().to_owned(),
        ..RenameRule::default()
    };
    for part in parts {
        let part = part.trim();
        let number = |value: &str, what: &str| {
            value.trim().parse::<u64>().map_err(|_| format!("{what} must be a number: {value}"))
        };
        match part {
            "lower" => rule.case = CaseChange::Lower,
            "upper" => rule.case = CaseChange::Upper,
            "title" => rule.case = CaseChange::Title,
            _ if part.starts_with("start=") => rule.counter_start = number(&part[6..], "start")?,
            _ if part.starts_with("step=") => rule.counter_step = number(&part[5..], "step")?,
            _ if part.starts_with("width=") => {
                rule.counter_width = number(&part[6..], "width")? as usize
            }
            _ if part.starts_with("s/") || part.starts_with("re/") => {
                let regex = part.starts_with("re/");
                let body = &part[if regex { 3 } else { 2 }..];
                let mut fields = body.splitn(2, '/');
                let find = fields.next().unwrap_or_default();
                let replace = fields.next().unwrap_or_default().trim_end_matches('/');
                if find.is_empty() {
                    return Err(format!("Nothing to find in: {part}"));
                }
                rule.find = find.to_owned();
                rule.replace = replace.to_owned();
                rule.regex = regex;
            }
            "" => {}
            _ => return Err(format!("Unknown option: {part} (F1 explains)")),
        }
    }
    Ok(rule)
}

/// The text of F1 in the question.
pub fn help_lines() -> Vec<String> {
    [
        "Rename many files at once. The new names are shown while you type.",
        "",
        "Template",
        "  [N]       name without extension      [N2-5]  characters 2 to 5",
        "  [E]       extension                   [X]     extension with the dot",
        "  [C]       counter (1, 2, 3...)        [P]     name of the folder",
        "  [[ ]]     a literal [ or ]",
        "",
        "Options, after the template, each one after ' | '",
        "  lower  upper  title      change the letter case",
        "  s/old/new/               replace text (literal)",
        "  re/old/new/              replace with a regular expression ($1 groups)",
        "  start=10  step=5  width=3  how [C] counts and how many digits it has",
        "",
        "Examples",
        "  holiday-[C] | width=3                   holiday-001.jpg, holiday-002.jpg",
        "  [N][X] | lower | s/ /_/                 My File.TXT -> my_file.txt",
        "  [P]-[N][X]                              adds the folder's name in front",
        "",
        "Keys:  Ctrl+O ready-made rules,  Enter shows the changes to confirm,  Esc closes.",
        "Nothing is renamed before you confirm, and Ctrl+Z takes it back.",
    ]
    .iter()
    .map(|l| (*l).to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_read() {
        let r = parse_rule("a-[C] | width=3 | start=5 | upper | s/x/y/").unwrap();
        assert_eq!(r.template, "a-[C]");
        assert_eq!((r.counter_width, r.counter_start), (3, 5));
        assert_eq!(r.case, CaseChange::Upper);
        assert_eq!((r.find.as_str(), r.replace.as_str(), r.regex), ("x", "y", false));
        let r = parse_rule("[N] | re/[0-9]+//").unwrap();
        assert_eq!((r.find.as_str(), r.replace.as_str(), r.regex), ("[0-9]+", "", true));
        assert!(parse_rule("[N] | bogus").is_err());
        assert!(parse_rule("[N] | width=x").is_err());
        assert!(parse_rule("[N] | s//y/").is_err());
    }

    #[test]
    fn every_preset_parses() {
        for (_, text) in PRESETS {
            parse_rule(text).unwrap();
        }
    }
}
