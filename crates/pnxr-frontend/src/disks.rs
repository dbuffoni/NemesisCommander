//! The disks and partitions of this computer: size, partition table,
//! file system, free space and where they are mounted.
//!
//! The operating system's own tools do the listing (`lsblk` on Linux,
//! `df` on macOS, PowerShell on Windows), so nothing here needs special
//! privileges and what is not known is shown as `-`.

use pnxr_panel::format::human_size;

use crate::usage::bar;

/// A disk, a partition or another block device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskRow {
    /// 0 for a disk, 1 for its partitions, and so on.
    pub depth: usize,
    /// Device name (`sda`, `nvme0n1p2`, `C:`).
    pub name: String,
    /// `disk`, `part`, `lvm`, `crypt`, `rom`, `volume`…
    pub kind: String,
    /// Model of a disk, label of a partition.
    pub label: String,
    /// File system (`ext4`, `ntfs`…).
    pub fs: String,
    /// Partition table of a disk (`gpt`, `dos`).
    pub table: String,
    /// Size in bytes.
    pub size: u64,
    /// Bytes used (when it is mounted).
    pub used: Option<u64>,
    /// Bytes free (when it is mounted).
    pub free: Option<u64>,
    /// Where it is mounted.
    pub mount: Option<String>,
    /// Removable (USB sticks, cards).
    pub removable: bool,
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_owned()
}

/// A number that `lsblk` writes as a number, a string or `null`.
fn number(value: &serde_json::Value, key: &str) -> Option<u64> {
    match value.get(key)? {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A flag that `lsblk` writes as a boolean, `"1"` or a number.
fn flag(value: &serde_json::Value, key: &str) -> bool {
    match value.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => s == "1" || s == "true",
        Some(serde_json::Value::Number(n)) => n.as_u64() == Some(1),
        _ => false,
    }
}

/// Reads `lsblk -b -J -o NAME,TYPE,SIZE,FSTYPE,LABEL,MODEL,PTTYPE,MOUNTPOINT,RM,FSUSED,FSAVAIL`.
pub fn parse_lsblk(json: &str) -> Result<Vec<DiskRow>, String> {
    let root: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let devices = root
        .get("blockdevices")
        .and_then(|d| d.as_array())
        .ok_or("no block devices in the answer of lsblk")?;
    let mut rows = Vec::new();
    for device in devices {
        add_device(device, 0, &mut rows);
    }
    Ok(rows)
}

fn add_device(device: &serde_json::Value, depth: usize, rows: &mut Vec<DiskRow>) {
    let kind = text(device, "type");
    // RAM disks and loop devices (snaps) are not disks anybody manages.
    if kind == "loop" || text(device, "name").starts_with("zram") {
        return;
    }
    let mount = {
        let single = text(device, "mountpoint");
        let several = device
            .get("mountpoints")
            .and_then(|m| m.as_array())
            .and_then(|m| m.iter().find_map(|p| p.as_str()))
            .unwrap_or("")
            .to_owned();
        [single, several].into_iter().find(|m| !m.is_empty())
    };
    let label = if depth == 0 { text(device, "model") } else { text(device, "label") };
    rows.push(DiskRow {
        depth,
        name: text(device, "name"),
        kind,
        label,
        fs: text(device, "fstype"),
        table: text(device, "pttype"),
        size: number(device, "size").unwrap_or(0),
        used: number(device, "fsused"),
        free: number(device, "fsavail"),
        mount,
        removable: flag(device, "rm"),
    });
    if let Some(children) = device.get("children").and_then(|c| c.as_array()) {
        for child in children {
            add_device(child, depth + 1, rows);
        }
    }
}

/// Reads the output of `df -Pk` (1 KiB blocks).
pub fn parse_df(output: &str) -> Vec<DiskRow> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 || !fields[0].starts_with("/dev/") {
                return None;
            }
            let kib = |s: &str| s.parse::<u64>().ok().map(|n| n * 1024);
            Some(DiskRow {
                name: fields[0].trim_start_matches("/dev/").to_owned(),
                kind: "volume".into(),
                size: kib(fields[1]).unwrap_or(0),
                used: kib(fields[2]),
                free: kib(fields[3]),
                mount: Some(fields[5..].join(" ")),
                ..DiskRow::default()
            })
        })
        .collect()
}

/// Reads the JSON of `Get-Volume | ConvertTo-Json` (an object when there
/// is one volume, else an array).
pub fn parse_volumes(json: &str) -> Result<Vec<DiskRow>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        single @ serde_json::Value::Object(_) => vec![single],
        _ => return Err("no volumes in the answer of PowerShell".into()),
    };
    Ok(items
        .iter()
        .filter_map(|v| {
            let letter = text(v, "DriveLetter");
            let size = number(v, "Size").unwrap_or(0);
            let free = number(v, "SizeRemaining");
            let kind = text(v, "DriveType");
            Some(DiskRow {
                name: if letter.is_empty() { text(v, "UniqueId") } else { format!("{letter}:") },
                kind: if kind.is_empty() { "volume".into() } else { kind.to_lowercase() },
                label: text(v, "FileSystemLabel"),
                fs: text(v, "FileSystem"),
                size,
                used: free.map(|f| size.saturating_sub(f)),
                free,
                mount: (!letter.is_empty()).then(|| format!("{letter}:\\")),
                removable: kind == "Removable",
                ..DiskRow::default()
            })
            .filter(|row| row.size > 0)
        })
        .collect())
}

/// The disks of this computer (runs the system's tools: call it from a
/// thread that may block).
pub fn list() -> Result<Vec<DiskRow>, String> {
    #[cfg(target_os = "linux")]
    {
        let output = run(
            "lsblk",
            &[
                "-b",
                "-J",
                "-o",
                "NAME,TYPE,SIZE,FSTYPE,LABEL,MODEL,PTTYPE,MOUNTPOINT,RM,FSUSED,FSAVAIL",
            ],
        )?;
        parse_lsblk(&output)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(parse_df(&run("df", &["-Pk"])?))
    }
    #[cfg(windows)]
    {
        let output = run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-Volume | Select-Object DriveLetter,FileSystemLabel,FileSystem,DriveType,Size,SizeRemaining | ConvertTo-Json",
            ],
        )?;
        parse_volumes(&output)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Ok(parse_df(&run("df", &["-Pk"])?))
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new(program);
    let output = pnxr_vfs::proc::hide_console(&mut command)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() { "-" } else { text }
}

/// The table's rows as text, with a usage bar for what is mounted.
pub fn lines(rows: &[DiskRow]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            let name = format!("{}{}", "  ".repeat(r.depth), r.name);
            let usage = match (r.used, r.free) {
                (Some(used), Some(free)) => {
                    let total = used + free;
                    format!(
                        "{} {:>3}%",
                        bar(used, total, 8),
                        (used * 100).checked_div(total).unwrap_or(0)
                    )
                }
                _ => format!("{:<12}", "-"),
            };
            let free = r.free.map_or_else(|| "-".to_owned(), human_size);
            format!(
                "{name:<14} {:<6} {:>10}  {:<5} {:<8} {usage} {free:>10} free  {}  {}{}",
                r.kind,
                human_size(r.size),
                or_dash(&r.table),
                or_dash(&r.fs),
                r.mount.as_deref().unwrap_or("-"),
                or_dash(&r.label),
                if r.removable { "  (removable)" } else { "" },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK: &str = r#"{
      "blockdevices": [
        {"name":"loop0","type":"loop","size":"4096","fstype":"squashfs","mountpoint":"/snap/x"},
        {"name":"sda","type":"disk","size":512110190592,"model":"Samsung SSD 860","pttype":"gpt","rm":false,
         "children":[
           {"name":"sda1","type":"part","size":536870912,"fstype":"vfat","label":"EFI","mountpoint":"/boot/efi","fsused":"6291456","fsavail":"530579456"},
           {"name":"sda2","type":"part","size":511573319680,"fstype":"ext4","label":null,"mountpoint":"/","fsused":200000000000,"fsavail":300000000000}
         ]},
        {"name":"sdb","type":"disk","size":"16000000000","model":"USB Stick","pttype":"dos","rm":"1"}
      ]
    }"#;

    #[test]
    fn lsblk_gives_disks_with_their_partitions() {
        let rows = parse_lsblk(LSBLK).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["sda", "sda1", "sda2", "sdb"], "loop devices are left out");
        assert_eq!(rows[0].table, "gpt");
        assert_eq!(rows[0].label, "Samsung SSD 860");
        assert_eq!(
            (rows[1].depth, rows[1].fs.as_str(), rows[1].label.as_str()),
            (1, "vfat", "EFI")
        );
        assert_eq!(rows[1].used, Some(6_291_456));
        assert_eq!(rows[2].free, Some(300_000_000_000));
        assert_eq!(rows[2].mount.as_deref(), Some("/"));
        assert!(rows[3].removable && !rows[0].removable);
        assert_eq!(rows[3].size, 16_000_000_000, "sizes written as text are read too");
        assert!(parse_lsblk("{}").is_err());
    }

    #[test]
    fn df_lists_the_mounted_devices() {
        let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                  /dev/disk3s1s1 488245288 15000000 400000000 4% /\n\
                  devfs 200 200 0 100% /dev\n\
                  /dev/disk4s1 1000 400 600 40% /Volumes/My Disk\n";
        let rows = parse_df(df);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].mount.as_deref(), Some("/Volumes/My Disk"));
        assert_eq!(
            (rows[1].size, rows[1].used, rows[1].free),
            (1_024_000, Some(409_600), Some(614_400))
        );
    }

    #[test]
    fn powershell_volumes_are_an_object_or_an_array() {
        let one = r#"{"DriveLetter":"C","FileSystemLabel":"OS","FileSystem":"NTFS","DriveType":"Fixed","Size":1000,"SizeRemaining":250}"#;
        let rows = parse_volumes(one).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].name.as_str(), rows[0].used, rows[0].mount.as_deref()),
            ("C:", Some(750), Some("C:\\"))
        );
        let many = format!("[{one},{{\"DriveLetter\":null,\"Size\":0}}]");
        assert_eq!(parse_volumes(&many).unwrap().len(), 1, "volumes without size are skipped");
    }

    #[test]
    fn lines_show_size_table_and_free_space() {
        let rows = parse_lsblk(LSBLK).unwrap();
        let text = lines(&rows);
        assert!(text[0].starts_with("sda "), "{}", text[0]);
        assert!(text[0].contains("gpt") && text[0].contains("Samsung SSD 860"), "{}", text[0]);
        assert!(text[2].starts_with("  sda2"), "{}", text[2]);
        assert!(
            text[2].contains("ext4") && text[2].contains("40%") && text[2].contains("/"),
            "{}",
            text[2]
        );
        assert!(text[3].contains("(removable)"), "{}", text[3]);
    }
}
