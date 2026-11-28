//! The program's launcher on a Linux desktop (`pnxr desktop install`).
//!
//! GNOME and other desktops show the icon of the *launcher* (the
//! `.desktop` file) found by the window's application id, not the one the
//! window sets, and they remember icons by name. After an update, an old
//! launcher or a cached icon keeps showing the old picture. The launcher
//! written here names its icon by an absolute path that holds a hash of the
//! picture, so a new picture can never be served from an old cache, and the
//! window brings it up to date when it starts ([`refresh`]).

use std::path::{Path, PathBuf};

/// The file name of the launcher (the application id of the window).
pub const DESKTOP_FILE: &str = "pairnox.desktop";

/// Marks a launcher as written by this program, so only those are touched.
const MARKER: &str = "X-Pairnox-Launcher=1";

/// The icon, 256 pixels.
const ICON: &[u8] = include_bytes!("../../../assets/branding/pairnox-256.png");

/// A short hash of `bytes` (FNV-1a), for the icon's file name.
fn hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", h >> 32)
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
fn data_home() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(path) => Some(PathBuf::from(path)),
        None => std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(|home| PathBuf::from(home).join(".local/share")),
    }
}

/// Where the launcher goes.
fn launcher_path(data: &Path) -> PathBuf {
    data.join("applications").join(DESKTOP_FILE)
}

/// The folder of the icons written here.
fn icon_dir(data: &Path) -> PathBuf {
    data.join("pairnox").join("icons")
}

/// The text of the launcher.
pub fn entry(exec: &str, icon: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Pairnox\nGenericName=File Manager\n\
         Comment=Fast dual-pane file manager for local and remote files\n\
         Exec={} %U\nIcon={}\nTerminal=false\nStartupWMClass=pairnox\n\
         Categories=System;FileTools;FileManager;Utility;\n\
         Keywords=files;file manager;commander;sftp;archive;\n{MARKER}\n",
        quote_exec(exec),
        icon.display()
    )
}

/// An `Exec` program name, quoted when it holds spaces.
fn quote_exec(program: &str) -> String {
    if program.contains([' ', '"', '\\', '$', '`']) {
        format!("\"{}\"", program.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$"))
    } else {
        program.to_owned()
    }
}

/// Writes the icon (named by its hash, older ones removed) and returns its
/// path.
fn write_icon(data: &Path) -> std::io::Result<PathBuf> {
    let dir = icon_dir(data);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("pairnox-{}.png", hash(ICON)));
    if !file.exists() {
        std::fs::write(&file, ICON)?;
    }
    for old in std::fs::read_dir(&dir)?.flatten() {
        if old.path() != file {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(file)
}

/// The program the launcher starts: the AppImage when running from one,
/// else the window program next to this one.
fn program() -> Result<String, String> {
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(image).display().to_string());
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let gui = me.with_file_name("pnxr-gui");
    if gui.exists() {
        return Ok(gui.display().to_string());
    }
    Err("cannot find the window program (pnxr-gui next to pnxr, or an AppImage)".into())
}

/// The launcher in the way: one that exists and was not written by this
/// program (another tool's `pairnox.desktop`), which is left alone.
fn foreign_launcher(data: &Path) -> Option<PathBuf> {
    let file = launcher_path(data);
    let text = std::fs::read_to_string(&file).ok()?;
    (!text.contains(MARKER)).then_some(file)
}

fn install_in(data: &Path, exec: &str) -> Result<PathBuf, String> {
    let icon = write_icon(data).map_err(|e| format!("cannot write the icon: {e}"))?;
    let file = launcher_path(data);
    std::fs::create_dir_all(file.parent().unwrap_or(data)).map_err(|e| e.to_string())?;
    std::fs::write(&file, entry(exec, &icon)).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file)
}

/// Writes the launcher and its icon for this user; returns what was done,
/// as lines to print. A `pairnox.desktop` that another tool wrote is not
/// replaced unless `force` is set.
pub fn install(force: bool) -> Result<Vec<String>, String> {
    let data = data_home().ok_or("cannot find the home folder")?;
    if !force && let Some(other) = foreign_launcher(&data) {
        return Err(format!(
            "{} was not written by Pairnox (it has no Pairnox marker), so it is left alone. \
             Rename or remove it and run this again, or use --force to replace it",
            other.display()
        ));
    }
    let exec = program()?;
    let file = install_in(&data, &exec)?;
    refresh_caches(&data);
    Ok(vec![
        format!("Launcher written: {}", file.display()),
        format!("It starts: {exec}"),
        "The icon is named by its content, so the desktop cannot show an old one.".to_owned(),
        "If an older launcher of Pairnox is still in the applications \
         list (for example one made by AppImageLauncher), remove that one."
            .to_owned(),
    ])
}

/// Removes the launcher and the icons written by [`install`].
pub fn remove() -> Result<Vec<String>, String> {
    let data = data_home().ok_or("cannot find the home folder")?;
    let file = launcher_path(&data);
    let mut lines = Vec::new();
    if std::fs::read_to_string(&file).is_ok_and(|t| t.contains(MARKER)) {
        std::fs::remove_file(&file).map_err(|e| e.to_string())?;
        lines.push(format!("Removed {}", file.display()));
    } else {
        lines.push("No launcher of this program was installed here".to_owned());
    }
    let _ = std::fs::remove_dir_all(data.join("pairnox").join("icons"));
    refresh_caches(&data);
    Ok(lines)
}

/// Brings an existing launcher written by [`install`] up to date (a new
/// icon after an update, the AppImage moved). Never creates one. Quiet:
/// the window must start whatever happens here.
pub fn refresh() {
    let Some(data) = data_home() else { return };
    let file = launcher_path(&data);
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    if !text.contains(MARKER) {
        return;
    }
    let Ok(exec) = program() else { return };
    let icon = icon_dir(&data).join(format!("pairnox-{}.png", hash(ICON)));
    if text == entry(&exec, &icon) && icon.exists() {
        return;
    }
    if install_in(&data, &exec).is_ok() {
        refresh_caches(&data);
    }
}

/// Asks the desktop to read the launchers again (best effort).
fn refresh_caches(data: &Path) {
    let mut command = std::process::Command::new("update-desktop-database");
    pnxr_vfs::proc::hide_console(&mut command)
        .arg(data.join("applications"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_names_the_icon_by_its_content() {
        let data = tempfile::tempdir().unwrap();
        let file = install_in(data.path(), "/opt/Pairnox.AppImage").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("Exec=/opt/Pairnox.AppImage %U"), "{text}");
        let icon_line = text.lines().find(|l| l.starts_with("Icon=")).unwrap();
        let icon = Path::new(&icon_line[5..]);
        assert!(icon.is_absolute() && icon.exists());
        assert!(icon.display().to_string().contains(&hash(ICON)));
        assert!(text.contains(MARKER));
    }

    #[test]
    fn a_launcher_of_another_tool_is_not_overwritten_without_force() {
        let data = tempfile::tempdir().unwrap();
        let file = launcher_path(data.path());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "[Desktop Entry]\nName=Other\nExec=other\n").unwrap();
        assert_eq!(foreign_launcher(data.path()), Some(file.clone()));
        install_in(data.path(), "pnxr-gui").unwrap();
        assert_eq!(foreign_launcher(data.path()), None, "ours now, with the marker");
        assert!(std::fs::read_to_string(&file).unwrap().contains(MARKER));
    }

    #[test]
    fn an_older_icon_is_replaced() {
        let data = tempfile::tempdir().unwrap();
        let dir = icon_dir(data.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pairnox-00000000.png"), b"old").unwrap();
        install_in(data.path(), "pnxr-gui").unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [format!("pairnox-{}.png", hash(ICON))]);
    }

    #[test]
    fn paths_with_spaces_are_quoted() {
        assert_eq!(quote_exec("/a b/c"), "\"/a b/c\"");
        assert_eq!(quote_exec("/a/c"), "/a/c");
    }
}
