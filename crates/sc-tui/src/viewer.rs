//! The internal file viewer (F3) and the quick view panel: text with tabs
//! expanded, or a hex dump for binary files.

use sc_vfs::{ProviderRegistry, VfsPath};
use tokio::io::AsyncReadExt;

/// At most this much of a file is loaded into the viewer.
pub const VIEW_LIMIT: u64 = 16 * 1024 * 1024;
/// The quick view panel loads only the beginning of a file.
pub const QUICK_VIEW_LIMIT: u64 = 64 * 1024;
/// Bytes shown per hex dump row.
pub const HEX_ROW: usize = 16;

/// Reads up to `limit` bytes of `path`. Returns the data and whether the
/// file was longer.
pub async fn load(
    registry: &ProviderRegistry,
    path: &VfsPath,
    limit: u64,
) -> sc_vfs::Result<(Vec<u8>, bool)> {
    let provider = registry.provider_for(path).await?;
    let reader = provider.read(path, 0).await?;
    let mut data = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut data)
        .await
        .map_err(|e| sc_vfs::VfsError::from_io(path, e))?;
    let truncated = data.len() as u64 > limit;
    data.truncate(limit as usize);
    Ok((data, truncated))
}

/// `true` when `data` looks like text: no NUL bytes in the first 8 KiB and
/// valid UTF-8 apart from a possibly cut last character.
pub fn looks_like_text(data: &[u8]) -> bool {
    let head = &data[..data.len().min(8192)];
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // A multi-byte character cut at the end of the sample is fine.
        Err(e) => e.error_len().is_none(),
    }
}

/// Splits text into display lines, expanding tabs.
pub fn text_lines(data: &[u8], tab_width: usize) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let tab_width = tab_width.max(1);
    text.lines()
        .map(|line| {
            if !line.contains('\t') {
                return line.to_owned();
            }
            let mut out = String::with_capacity(line.len() + 8);
            let mut column = 0;
            for c in line.chars() {
                if c == '\t' {
                    let spaces = tab_width - column % tab_width;
                    out.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                } else {
                    out.push(c);
                    column += 1;
                }
            }
            out
        })
        .collect()
}

/// One hex dump row: offset, 16 bytes in hex, and their printable form.
pub fn hex_row(data: &[u8], row: usize) -> String {
    let start = row * HEX_ROW;
    let bytes = &data[start.min(data.len())..(start + HEX_ROW).min(data.len())];
    let mut line = format!("{start:08x}  ");
    for i in 0..HEX_ROW {
        match bytes.get(i) {
            Some(b) => line.push_str(&format!("{b:02x} ")),
            None => line.push_str("   "),
        }
        if i == 7 {
            line.push(' ');
        }
    }
    line.push(' ');
    line.extend(
        bytes.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }),
    );
    line.extend(std::iter::repeat_n(' ', HEX_ROW - bytes.len()));
    line
}

/// Number of hex rows for `len` bytes.
pub fn hex_rows(len: usize) -> usize {
    len.div_ceil(HEX_ROW).max(1)
}

/// State of the full-screen viewer.
#[derive(Debug, Clone)]
pub struct Viewer {
    /// The file shown.
    pub path: VfsPath,
    /// Loaded bytes (`None` while loading).
    pub data: Option<Vec<u8>>,
    /// The file is longer than what was loaded.
    pub truncated: bool,
    /// Text lines (computed once when loaded).
    pub lines: Vec<String>,
    /// Hex dump instead of text.
    pub hex: bool,
    /// Wrap long text lines.
    pub wrap: bool,
    /// First visible row.
    pub scroll: usize,
    /// Loading error.
    pub error: Option<String>,
}

impl Viewer {
    /// A viewer waiting for the contents of `path`.
    pub fn new(path: VfsPath, wrap: bool) -> Self {
        Viewer {
            path,
            data: None,
            truncated: false,
            lines: Vec::new(),
            hex: false,
            wrap,
            scroll: 0,
            error: None,
        }
    }

    /// Shows loaded data. Binary data starts in hex mode.
    pub fn set_data(&mut self, data: Vec<u8>, truncated: bool, tab_width: usize) {
        self.hex = !looks_like_text(&data);
        self.lines = text_lines(&data, tab_width);
        self.data = Some(data);
        self.truncated = truncated;
        self.scroll = 0;
    }

    /// Number of rows in the current mode.
    pub fn row_count(&self) -> usize {
        match &self.data {
            None => 0,
            Some(data) if self.hex => hex_rows(data.len()),
            Some(_) => self.lines.len(),
        }
    }

    /// Scrolls by `delta` rows, keeping a page of `page` rows visible.
    pub fn scroll_by(&mut self, delta: isize, page: usize) {
        let max = self.row_count().saturating_sub(page.max(1));
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    /// Switches between text and hex, keeping roughly the same position.
    pub fn toggle_hex(&mut self) {
        if self.data.is_none() {
            return;
        }
        let offset = if self.hex {
            self.scroll * HEX_ROW
        } else {
            // Byte offset of the first visible line.
            self.lines.iter().take(self.scroll).map(|l| l.len() + 1).sum::<usize>()
        };
        self.hex = !self.hex;
        self.scroll = if self.hex {
            offset / HEX_ROW
        } else {
            let mut seen = 0;
            self.lines
                .iter()
                .take_while(|l| {
                    seen += l.len() + 1;
                    seen <= offset
                })
                .count()
        };
        self.scroll = self.scroll.min(self.row_count().saturating_sub(1));
    }

    /// The rows to draw, starting at the scroll position.
    pub fn visible_rows(&self, count: usize) -> Vec<String> {
        match &self.data {
            None => Vec::new(),
            Some(data) if self.hex => (self.scroll
                ..(self.scroll + count).min(hex_rows(data.len())))
                .map(|row| hex_row(data, row))
                .collect(),
            Some(_) => self.lines.iter().skip(self.scroll).take(count).cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_detection() {
        assert!(looks_like_text(b"hello\nworld"));
        assert!(looks_like_text("città".as_bytes()));
        assert!(looks_like_text(&"è".as_bytes()[..1]), "cut character at the end");
        assert!(!looks_like_text(b"\x7fELF\x00\x01"));
        assert!(!looks_like_text(&[0xff, 0xfe, b'a', b'b']));
    }

    #[test]
    fn tabs_are_expanded() {
        assert_eq!(text_lines(b"a\tb\n\tc", 4), vec!["a   b", "    c"]);
    }

    #[test]
    fn hex_rows_have_a_fixed_layout() {
        let data: Vec<u8> = (0u8..20).collect();
        assert_eq!(hex_rows(data.len()), 2);
        let first = hex_row(&data, 0);
        assert!(first.starts_with("00000000  00 01 02 03 04 05 06 07  08 09"));
        let second = hex_row(&data, 1);
        assert!(second.starts_with("00000010  10 11 12 13"));
        assert_eq!(first.len(), second.len(), "short rows are padded");
    }

    #[test]
    fn viewer_scrolls_and_toggles() {
        let mut viewer = Viewer::new(VfsPath::parse("mem:///x").unwrap(), false);
        let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
        viewer.set_data(text.into_bytes(), false, 4);
        assert!(!viewer.hex);
        assert_eq!(viewer.row_count(), 100);
        viewer.scroll_by(1000, 10);
        assert_eq!(viewer.scroll, 90);
        assert_eq!(viewer.visible_rows(3)[0], "line 90");
        viewer.toggle_hex();
        assert!(viewer.hex);
        viewer.toggle_hex();
        // Hex rows are coarser than lines, so the round trip may land on the
        // line just before.
        assert!((89..=90).contains(&viewer.scroll), "position survives a round trip");

        viewer.set_data(vec![0, 1, 2], false, 4);
        assert!(viewer.hex, "binary data opens as hex");
    }

    #[tokio::test]
    async fn load_reports_truncation() {
        let registry = sc_providers::default_registry();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f"), b"0123456789").unwrap();
        let path = VfsPath::from_local(dir.path().join("f")).unwrap();
        assert_eq!(load(&registry, &path, 100).await.unwrap(), (b"0123456789".to_vec(), false));
        assert_eq!(load(&registry, &path, 4).await.unwrap(), (b"0123".to_vec(), true));
    }
}
