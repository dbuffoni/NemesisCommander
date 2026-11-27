//! Windows local paths. The pure rules live here so they are tested on every
//! system; only the Windows build uses them on real paths (ADR 0052).
//!
//! In a [`crate::VfsPath`] a drive path `C:\dir` is `/C:/dir` and a UNC path
//! `\\server\share\dir` is `/UNC/server/share/dir`.

/// Names Windows keeps for devices, with or without an extension.
const RESERVED: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
];

/// Past this many characters a native path needs the `\\?\` prefix (the
/// classic limit is 260 including the end; folders need room for `\*`).
const LONG_PATH: usize = 240;

/// Why `name` (one file or folder name) cannot be created on Windows, or
/// `Ok` when it can. Applied on Windows when creating or renaming, and
/// meant for names that come from another system.
pub fn check_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("the name is empty");
    }
    if name
        .chars()
        .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err("a Windows name cannot contain < > : \" / \\ | ? * or control characters");
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err("a Windows name cannot end with a dot or a space");
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    if RESERVED.iter().any(|reserved| stem.eq_ignore_ascii_case(reserved)) {
        return Err("this name is reserved by Windows for a device (CON, NUL, COM1 ...)");
    }
    Ok(())
}

/// The VFS inner form of a Windows path typed or returned by Windows:
/// `\\server\share\dir` and `\\?\UNC\server\share\dir` become
/// `/UNC/server/share/dir`. `None` when `input` is not a UNC path.
pub fn unc_to_inner(input: &str) -> Option<String> {
    // Only the backslash form: `//dir/x` is an ordinary path elsewhere.
    if !input.starts_with("\\\\") {
        return None;
    }
    let unified = input.replace('\\', "/");
    let rest = if let Some(rest) = unified.strip_prefix("//?/UNC/") {
        rest
    } else if unified.starts_with("//?/") || unified.starts_with("//./") {
        return None;
    } else {
        unified.strip_prefix("//")?
    };
    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let (server, share) = (parts.next()?, parts.next()?);
    let mut inner = format!("/UNC/{server}/{share}");
    for part in parts {
        inner.push('/');
        inner.push_str(part);
    }
    Some(inner)
}

/// The native Windows path for the inner path of a local VFS path
/// (`/C:/dir`, `/UNC/server/share/dir`); `None` for a path that is neither.
/// A path over 240 characters gets the `\\?\` prefix, which lifts the 260
/// limit.
pub fn native_from_inner(inner: &str) -> Option<String> {
    let trimmed = inner.strip_prefix('/')?;
    let (plain, verbatim) = if let Some(unc) = trimmed.strip_prefix("UNC/") {
        let mut parts = unc.splitn(3, '/');
        let (server, share) = (parts.next()?, parts.next()?);
        let rest = parts.next().unwrap_or("").replace('/', "\\");
        let tail = if rest.is_empty() { String::new() } else { format!("\\{rest}") };
        (format!("\\\\{server}\\{share}{tail}"), format!("\\\\?\\UNC\\{server}\\{share}{tail}"))
    } else {
        let b = trimmed.as_bytes();
        let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
        if !drive || (b.len() > 2 && b[2] != b'/') {
            return None;
        }
        let native = trimmed.replace('/', "\\");
        let native = if native.len() == 2 { format!("{native}\\") } else { native };
        (native.clone(), format!("\\\\?\\{native}"))
    };
    Some(if plain.chars().count() >= LONG_PATH { verbatim } else { plain })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_names_pass() {
        for name in ["a.txt", "report (final).docx", "conx.txt", "COM10", "LPT0", ".hidden", "é.md"]
        {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn reserved_names_are_refused_with_or_without_an_extension() {
        for name in [
            "CON",
            "con",
            "Nul.txt",
            "aux.tar.gz",
            "COM1",
            "lpt9.log",
            "COM\u{b9}",
            "CONIN$",
            "NUL .",
        ] {
            assert!(check_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn trailing_dots_and_spaces_and_bad_characters_are_refused() {
        for name in
            ["", "a.", "a ", "a:b", "a?b", "a*b", "a|b", "a<b", "a\"b", "a\tb", "a/b", "a\\b"]
        {
            assert!(check_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn unc_paths_become_inner_paths() {
        assert_eq!(
            unc_to_inner(r"\\srv\share\dir\f.txt").as_deref(),
            Some("/UNC/srv/share/dir/f.txt")
        );
        assert_eq!(unc_to_inner(r"\\srv\share").as_deref(), Some("/UNC/srv/share"));
        assert_eq!(unc_to_inner(r"\\?\UNC\srv\share\x").as_deref(), Some("/UNC/srv/share/x"));
        assert_eq!(unc_to_inner("//srv/share/x"), None, "forward slashes are an ordinary path");
        // No share, a verbatim drive path, a device path, a plain path.
        assert_eq!(unc_to_inner(r"\\srv"), None);
        assert_eq!(unc_to_inner(r"\\?\C:\dir"), None);
        assert_eq!(unc_to_inner(r"\\.\COM1"), None);
        assert_eq!(unc_to_inner(r"C:\dir"), None);
    }

    #[test]
    fn native_paths_and_the_long_prefix() {
        assert_eq!(native_from_inner("/C:/Users/me").as_deref(), Some(r"C:\Users\me"));
        assert_eq!(native_from_inner("/C:").as_deref(), Some(r"C:\"));
        assert_eq!(native_from_inner("/UNC/srv/share/d/f").as_deref(), Some(r"\\srv\share\d\f"));
        assert_eq!(native_from_inner("/UNC/srv/share").as_deref(), Some(r"\\srv\share"));
        assert_eq!(native_from_inner("/home/me"), None);
        let long = format!("/C:/{}", ["abcdefghij"; 30].join("/"));
        let native = native_from_inner(&long).unwrap();
        assert!(native.starts_with(r"\\?\C:\abcdefghij\"), "{native}");
        let long_unc = format!("/UNC/srv/share/{}", ["abcdefghij"; 30].join("/"));
        assert!(native_from_inner(&long_unc).unwrap().starts_with(r"\\?\UNC\srv\share\"));
    }
}
