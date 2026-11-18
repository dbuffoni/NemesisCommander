//! The drive list (Alt+F1, Ctrl+N): this computer's drives and the saved
//! connections in one list, each with a category. Choosing a row opens it in
//! the active panel the same way, whatever its category.

use std::path::PathBuf;

use nmc_config::Config;
use nmc_vfs::ARCHIVE_SCHEMES;

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
        let row = format!("{:<8} {:<20} {}", self.category.label(), self.name, self.location);
        match connection_kind(&self.location) {
            Some(kind) => format!("{row}  [{kind}]"),
            None => row,
        }
    }
}

/// How a remote location is reached, in words (`None` for local places and
/// archives). The agent over SSH and the `nmc serve` daemon both look like
/// "Remote", so this is what tells them apart.
pub fn connection_kind(location: &str) -> Option<&'static str> {
    let (scheme, _) = location.split_once("://")?;
    Some(match scheme {
        "agent" => "agent over SSH",
        "scs" => "nmc serve, TLS",
        "sftp" => "SFTP over SSH",
        "s3" => "S3",
        "smb" => "SMB",
        "dav" | "davs" | "webdav" | "webdavs" => "WebDAV",
        _ => return None,
    })
}

/// What to add to the drive text of a remote panel whose last listing
/// failed with a lost connection (`lost`): the next listing, already
/// running (`loading`), opens the connection again.
pub fn connection_state(lost: bool, loading: bool) -> &'static str {
    match (lost, loading) {
        (true, true) => " · reconnecting…",
        (true, false) => " · connection lost",
        (false, _) => "",
    }
}

/// The short name shown above a panel for `location`: the saved connection
/// it is inside, else the kind of place (`Local`, `C:`, `sftp`, `Archive`).
pub fn drive_label(config: &Config, location: &nmc_vfs::VfsPath) -> String {
    let text = location.to_string();
    let scheme = location.scheme();
    if scheme == "file" {
        let local = location.to_local_path().map(|p| p.display().to_string()).unwrap_or_default();
        let bytes = local.as_bytes();
        return if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            local[..2].to_ascii_uppercase()
        } else {
            "Local".to_owned()
        };
    }
    if ARCHIVE_SCHEMES.contains(&scheme) {
        return "Archive".to_owned();
    }
    config
        .connections
        .iter()
        .filter(|c| text.starts_with(c.uri.trim_end_matches('/')))
        .max_by_key(|c| c.uri.len())
        .map_or_else(|| scheme.to_owned(), |c| c.name.clone())
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
pub fn suggested_name(location: &nmc_vfs::VfsPath) -> String {
    let authority = location.authority();
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() { location.scheme().to_owned() } else { host.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmc_config::Connection;

    #[test]
    fn new_connections_are_named_after_their_host() {
        let name = |uri: &str| suggested_name(&nmc_vfs::VfsPath::parse(uri).unwrap());
        assert_eq!(name("sftp://me@nas.lan:2222/srv"), "nas.lan");
        assert_eq!(name("s3://photos/2026"), "photos");
    }

    #[test]
    fn a_lost_connection_shows_until_a_listing_works() {
        assert_eq!(connection_state(false, true), "");
        assert_eq!(connection_state(true, false), " · connection lost");
        assert_eq!(connection_state(true, true), " · reconnecting…");
    }

    #[test]
    fn the_connection_kind_tells_agent_and_serve_apart() {
        assert_eq!(connection_kind("agent://nas/srv"), Some("agent over SSH"));
        assert_eq!(connection_kind("scs://nas:7443/srv"), Some("nmc serve, TLS"));
        assert_eq!(connection_kind("sftp://me@nas/srv"), Some("SFTP over SSH"));
        assert_eq!(connection_kind("/home/me"), None);
        assert_eq!(connection_kind("zip:///a.zip!/"), None);
        let drive = Drive::new("nas", "scs://nas:7443/srv");
        assert!(drive.row().ends_with("[nmc serve, TLS]"), "{}", drive.row());
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

    #[test]
    fn a_panel_names_its_drive() {
        let mut config = Config::default();
        config.connections.push(Connection {
            name: "nas".into(),
            uri: "sftp://me@nas/srv".into(),
            identity_file: None,
            options: Default::default(),
        });
        let label = |text: &str| drive_label(&config, &nmc_vfs::VfsPath::parse(text).unwrap());
        assert_eq!(label("/var/tmp"), "Local");
        assert_eq!(label("sftp://me@nas/srv/photos"), "nas");
        assert_eq!(label("sftp://me@other/home"), "sftp");
        assert_eq!(label("s3://bucket/key"), "s3");
    }
}
