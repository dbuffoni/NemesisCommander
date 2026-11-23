//! The key rows under every dialog, in one place so the terminal and the
//! window show the same text, in the same order.
//!
//! The convention: one row, parts apart by four spaces, each `Key: verb`
//! (verbs in lower case; `OK` is the exception), in this order:
//!
//! 1. the main action (`Enter: OK`, `Enter: open`, `Enter: run`…),
//! 2. the keys this dialog adds,
//! 3. `Shift+Tab: back`, only when there is a previous question,
//! 4. `F1: help`, when the dialog has help,
//! 5. `Esc: cancel` (questions, confirms) or `Esc: close` (lists, messages,
//!    viewers), always last.
//!
//! A part may lead with a status (`11 found`, `Working…`), without a key.

/// Between two parts.
pub const SEP: &str = "    ";

/// F1: the main shortcuts.
pub const HELP: &str = "F9: all commands    Esc: close";

/// Ctrl+J: the transfer queue.
pub const JOBS: &str = "p: pause    r: resume    c: cancel    +/-: priority    \
    [/]: files at once    a: auto    x: remove finished    Esc: close";

/// A copy conflict.
pub const CONFLICT: &str = "o: overwrite    s: skip    r: keep both    \
    Shift+O / S / R: same for all    a: abort    Esc: cancel";

/// The keys of a question (a text field): `extra` are the keys it adds.
pub fn question(extra: &[&str]) -> String {
    let mut parts = vec!["Enter: OK", "Ctrl+U: clear"];
    parts.extend_from_slice(extra);
    parts.push("Esc: cancel");
    parts.join(SEP)
}

/// A yes/no question. `default_no`: Enter says no (what cannot be undone).
pub fn confirm(default_no: bool) -> String {
    if default_no {
        ["Enter: no", "y: yes", "Esc: cancel"].join(SEP)
    } else {
        ["Enter: yes", "n: no", "Esc: cancel"].join(SEP)
    }
}

/// A list: `verb` is what Enter does, `extra` the keys it adds, `esc` the
/// last part (`cancel` for a step of a question, `close` otherwise).
pub fn list(verb: &str, extra: &[&str], esc: &str) -> String {
    let mut parts = vec![format!("Enter: {verb}")];
    parts.extend(extra.iter().map(|part| (*part).to_owned()));
    parts.push(format!("Esc: {esc}"));
    parts.join(SEP)
}

/// `hint` with `Shift+Tab: back` placed before help and Esc.
pub fn with_back(hint: &str) -> String {
    let mut parts: Vec<&str> = hint.split(SEP).filter(|p| !p.is_empty()).collect();
    let at = parts.iter().position(|p| p.starts_with("F1: ") || p.starts_with("Esc: "));
    parts.insert(at.unwrap_or(parts.len()), "Shift+Tab: back");
    parts.join(SEP)
}

/// What is wrong with `hint` as the key row of a dialog, if anything.
pub fn check(hint: &str) -> Result<(), String> {
    let parts: Vec<&str> = hint.split(SEP).collect();
    let Some(last) = parts.last() else { return Err("empty".into()) };
    if !(last.starts_with("Esc: cancel")
        || last.starts_with("Esc: close")
        || last.starts_with("Esc: clear"))
    {
        return Err(format!("{hint:?} does not end with Esc: cancel/close"));
    }
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || part.trim() != *part || part.contains("  ") {
            return Err(format!("{hint:?}: bad separator around {part:?}"));
        }
        // A status may lead (`11 found`, `Working…`, `Find: x_`).
        if i == 0 && !part.contains(": ") && parts.len() > 1 {
            continue;
        }
        let Some((key, verb)) = part.split_once(": ") else {
            return Err(format!("{hint:?}: {part:?} is not `Key: verb`"));
        };
        if key.is_empty() || verb.is_empty() {
            return Err(format!("{hint:?}: {part:?} is not `Key: verb`"));
        }
        for bad in ["Del", "Backsp", "Escape", "Return", "PageUp", "PgDown", "PgUp "] {
            if key == bad {
                return Err(format!("{hint:?}: key name {key:?}"));
            }
        }
        let first = verb.chars().next().unwrap_or(' ');
        if first.is_uppercase() && verb != "OK" && !verb.starts_with("OK") {
            return Err(format!("{hint:?}: verb {verb:?} is not lower case"));
        }
    }
    // Back, help and Esc keep their places.
    let at = |prefix: &str| parts.iter().position(|p| p.starts_with(prefix));
    if let (Some(back), Some(help)) = (at("Shift+Tab: "), at("F1: "))
        && back > help
    {
        return Err(format!("{hint:?}: back must come before help"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_rows_follow_the_convention() {
        for hint in [HELP, JOBS, CONFLICT, &question(&[]), &confirm(true), &confirm(false)] {
            check(hint).unwrap();
        }
        check(&list("open", &["Delete: remove"], "close")).unwrap();
    }

    #[test]
    fn back_goes_before_help_and_esc() {
        let hint = with_back(&question(&["F1: help"]));
        check(&hint).unwrap();
        assert!(hint.ends_with("Shift+Tab: back    F1: help    Esc: cancel"), "{hint}");
        assert_eq!(with_back("Esc: close"), "Shift+Tab: back    Esc: close");
    }

    #[test]
    fn odd_rows_are_refused() {
        assert!(check("Enter: OK    Del: remove    Esc: close").is_err());
        assert!(check("Enter: OK  Esc: close").is_err());
        assert!(check("Enter: OK    Esc: cancel    Ctrl+U: clear").is_err());
        assert!(check("Esc: close    Enter: Open").is_err());
    }
}
