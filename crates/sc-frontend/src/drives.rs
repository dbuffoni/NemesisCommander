//! The drive list (Alt+F1, Ctrl+N): this computer's drives and the saved
//! connections in one list, each with a category. Choosing a row opens it in
//! the active panel the same way, whatever its category.

use std::path::PathBuf;

use sc_config::Config;
use sc_vfs::ARCHIVE_SCHEMES;

/// What kind of place a drive row opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// A drive or mount of this computer.
    Local,
    /// A server or cloud storage.
    Remote,
    /// The inside of an archive file.
    Archive,
}

impl Category {
    /// The name shown in the list.
    pub fn label(self) -> &'static str {
        match self {
            Category::Local => "Local",
            Category::Remote => "Remote",
            Category::Archive => "Archive",
        }
    }

    /// The category of a location written as a path or URI.
    pub fn of(location: &str) -> Category {
        match location.split_once("://") {
            Some((scheme, _)) if ARCHIVE_SCHEMES.contains(&scheme) => Category::Archive,
            Some((scheme, _)) if scheme != "file" => Category::Remote,
            _ => Category::Local,
        }
    }
}

/// One row of the drive list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    /// Local, Remote or Archive.
    pub category: Category,
    /// Short name ("Home", "USB", a connection's name).
    pub name: String,
    /// Where it opens: a local path or a URI.
    pub location: String,
    /// A saved connection (which Delete removes), not a drive of this computer.
    pub saved: bool,
}

impl Drive {
    fn new(name: impl Into<String>, location: impl Into<String>) -> Self {
        let location = location.into();
        Drive { category: Category::of(&location), name: name.into(), location, saved: false }
    }

    /// The row as the list shows it.
    pub fn row(&self) -> String {
        format!("{:<8} {:<20} {}", self.category.label(), self.name, self.location)
    }
}

/// Every drive of the list, grouped by category (Local, Remote, Archive):
/// this computer's drives first, then the saved connections in their order.
pub fn drives(config: &Config) -> Vec<Drive> {
    let mut list = local_drives();
    list.extend(
        config
            .connections
            .iter()
            .map(|c| Drive { saved: true, ..Drive::new(c.name.clone(), c.uri.clone()) }),
    );
    // A stable sort keeps the order inside each category.
    list.sort_by_key(|d| d.category);
    list
}

/// This computer's drives: the home folder and the file systems mounted on it.
pub fn local_drives() -> Vec<Drive> {
    let mut list = Vec::new();
    let home = directories::UserDirs::new().map(|d| d.home_dir().to_path_buf());
    if home.is_some() {
        // "~", as the panels show it; Go to resolves it.
        list.push(Drive::new("Home", "~"));
    }
    for mount in mount_points() {
        if home.as_deref() == Some(mount.as_path()) {
            continue;
        }
        let name = match mount.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => mount.display().to_string(),
        };
        list.push(Drive::new(name, mount.display().to_string()));
    }
    list
}

#[cfg(windows)]
fn mount_points() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|l| PathBuf::from(format!("{}:\\", l as char)))
        .filter(|p| p.exists())
        .collect()
}

#[cfg(target_os = "macos")]
fn mount_points() -> Vec<PathBuf> {
    let mut list = vec![PathBuf::from("/")];
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        let mut volumes: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        volumes.sort();
        list.extend(volumes);
    }
    list
}

#[cfg(not(any(windows, target_os = "macos")))]
fn mount_points() -> Vec<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let mut list = vec![PathBuf::from("/")];
    for mount in user_mounts(&text) {
        if !list.contains(&mount) {
            list.push(mount);
        }
    }
    list
}

/// The mount points of `/proc/self/mounts` a user would open: disks and
/// network shares, not the system's own file systems (`/proc`, `/boot`,
/// snaps, containers).
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
fn user_mounts(mounts: &str) -> Vec<PathBuf> {
    const NETWORK: &[&str] = &["nfs", "nfs4", "cifs", "smb3", "fuse.sshfs", "fuse.rclone"];
    const SYSTEM: &[&str] = &["/boot", "/snap", "/proc", "/sys", "/dev", "/run", "/var", "/tmp"];
    let mut list = Vec::new();
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(point), Some(kind)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // Spaces in mount points are written as \040.
        let point = point.replace("\\040", " ");
        let is_disk = device.starts_with("/dev/") && !kind.starts_with("squashfs");
        if !(is_disk || NETWORK.contains(&kind)) || point == "/" {
            continue;
        }
        let user_media = ["/run/media/", "/media/", "/mnt/"].iter().any(|p| point.starts_with(p));
        let system = SYSTEM.iter().any(|s| point == *s || point.starts_with(&format!("{s}/")));
        if system && !user_media {
            continue;
        }
        let point = PathBuf::from(point);
        if !list.contains(&point) {
            list.push(point);
        }
    }
    list
}

/// A name for a new connection: its host or bucket, else its scheme.
pub fn suggested_name(location: &sc_vfs::VfsPath) -> String {
    let authority = location.authority();
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() { location.scheme().to_owned() } else { host.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_config::Connection;

    #[test]
    fn new_connections_are_named_after_their_host() {
        let name = |uri: &str| suggested_name(&sc_vfs::VfsPath::parse(uri).unwrap());
        assert_eq!(name("sftp://me@nas.lan:2222/srv"), "nas.lan");
        assert_eq!(name("s3://photos/2026"), "photos");
    }

    #[test]
    fn categories_come_from_the_scheme() {
        assert_eq!(Category::of("/home/me"), Category::Local);
        assert_eq!(Category::of("C:\\"), Category::Local);
        assert_eq!(Category::of("file:///srv"), Category::Local);
        assert_eq!(Category::of("sftp://me@host/srv"), Category::Remote);
        assert_eq!(Category::of("s3://bucket/"), Category::Remote);
        assert_eq!(Category::of("smb://nas/share"), Category::Remote);
        assert_eq!(Category::of("zip:///a/site.zip!/"), Category::Archive);
    }

    #[test]
    fn user_mounts_keep_disks_and_shares() {
        let mounts = "\
proc /proc proc rw 0 0
/dev/nvme0n1p2 / ext4 rw 0 0
/dev/nvme0n1p1 /boot/efi vfat rw 0 0
/dev/sda1 /run/media/me/USB\\040Stick vfat rw 0 0
/dev/loop3 /snap/core/1 squashfs ro 0 0
nas:/export /mnt/nas nfs4 rw 0 0
/dev/sdb1 /data ext4 rw 0 0
tmpfs /tmp tmpfs rw 0 0
";
        assert_eq!(
            user_mounts(mounts),
            [
                PathBuf::from("/run/media/me/USB Stick"),
                PathBuf::from("/mnt/nas"),
                PathBuf::from("/data")
            ]
        );
    }

    #[test]
    fn connections_join_the_drives_grouped_by_category() {
        let mut config = Config::default();
        for (name, uri) in [
            ("site", "zip:///srv/site.zip!/"),
            ("nas", "sftp://me@nas/srv"),
            ("scratch", "/var/tmp"),
            ("photos", "s3://photos/"),
        ] {
            config.connections.push(Connection {
                name: name.into(),
                uri: uri.into(),
                identity_file: None,
                options: Default::default(),
            });
        }
        let list = drives(&config);
        let categories: Vec<Category> = list.iter().map(|d| d.category).collect();
        let mut sorted = categories.clone();
        sorted.sort();
        assert_eq!(categories, sorted, "grouped");
        let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
        let tail = &names[names.len() - 3..];
        assert_eq!(tail, ["nas", "photos", "site"], "connections keep their order");
        assert!(names.contains(&"scratch"));
        assert!(list.iter().filter(|d| d.saved).count() == 4);
        assert_eq!(
            list.last().unwrap().row(),
            format!("{:<8} {:<20} {}", "Archive", "site", "zip:///srv/site.zip!/")
        );
    }
}
