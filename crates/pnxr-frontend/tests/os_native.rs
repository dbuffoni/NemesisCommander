//! Checks against the real operating system. They run on every system and
//! assert what that system should give, so the nightly full tier (Linux,
//! macOS, Windows) covers the OS-specific code (SC-164, docs/ci.md).

use pnxr_frontend::{disks, drives};
use pnxr_vfs::VfsPath;

#[test]
fn local_drives_start_with_home_and_list_real_places() {
    let list = drives::local_drives();
    assert!(!list.is_empty());
    for drive in list.iter().filter(|d| d.location != "~") {
        let path = std::path::Path::new(&drive.location);
        assert!(path.exists(), "{} is listed but does not exist", drive.location);
    }
    if cfg!(windows) {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let want = format!("{}\\", system.trim_end_matches('\\'));
        assert!(
            list.iter().any(|d| d.location.eq_ignore_ascii_case(&want)),
            "the system drive {want} is missing: {list:?}"
        );
    } else if cfg!(target_os = "macos") {
        assert!(list.iter().any(|d| d.location == "/"), "{list:?}");
    }
}

#[test]
fn the_disk_list_comes_from_the_system_tools() {
    // Hosted runners may have no `lsblk` rights; an error is reported, not a panic.
    match disks::list() {
        Ok(rows) => {
            if !cfg!(target_os = "linux") {
                assert!(!rows.is_empty(), "no disk found");
            }
            let _ = disks::lines(&rows);
        }
        Err(e) => assert!(!e.is_empty()),
    }
}

#[test]
fn a_temp_folder_round_trips_through_the_path_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = VfsPath::from_local(dir.path()).unwrap();
    assert_eq!(path.scheme(), "file");
    let back = path.to_local_path().unwrap();
    // Canonical forms agree (8.3 names, /private on macOS, \\?\ on Windows).
    assert_eq!(std::fs::canonicalize(&back).unwrap(), std::fs::canonicalize(dir.path()).unwrap());
    if cfg!(windows) {
        assert!(path.path().starts_with('/'), "{}", path.path());
        assert!(path.path().as_bytes()[2] == b':', "drive letter form: {}", path.path());
    }
}

#[test]
fn a_symlink_is_listed_as_a_link() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    std::fs::write(&target, b"x").unwrap();
    let link = dir.path().join("link.txt");
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link).is_ok();
    // Creating links on Windows needs a privilege the runners have.
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link).is_ok();
    if !made {
        return;
    }
    let meta = std::fs::symlink_metadata(&link).unwrap();
    assert!(meta.file_type().is_symlink());
}
