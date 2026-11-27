//! Listings as text: the "copy path" variants and "export folder listing"
//! in CSV, JSON, Markdown and HTML.

use pnxr_panel::format::local_time;
use pnxr_vfs::{Entry, EntryKind, VfsPath};

use crate::app::display_location;

/// What "copy path to clipboard" can put in the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathVariant {
    /// File names, one per line.
    Name,
    /// Full paths, one per line (as the panel shows them).
    FullPath,
    /// Locations with their scheme (`file:///home/ada/a.txt`).
    Uri,
    /// Folder path of the first item.
    Folder,
}

impl PathVariant {
    /// The variants in menu order.
    pub const ALL: [PathVariant; 4] =
        [PathVariant::FullPath, PathVariant::Name, PathVariant::Uri, PathVariant::Folder];

    /// The row of the variants menu.
    pub fn label(self) -> &'static str {
        match self {
            PathVariant::Name => "Name",
            PathVariant::FullPath => "Full path",
            PathVariant::Uri => "URI",
            PathVariant::Folder => "Folder path",
        }
    }

    /// The text for `paths`, one item per line.
    pub fn text(self, paths: &[VfsPath]) -> String {
        let lines: Vec<String> = match self {
            PathVariant::Name => {
                paths.iter().map(|p| p.file_name().unwrap_or("/").to_owned()).collect()
            }
            PathVariant::FullPath => paths.iter().map(display_location).collect(),
            PathVariant::Uri => paths.iter().map(ToString::to_string).collect(),
            PathVariant::Folder => paths
                .iter()
                .take(1)
                .filter_map(VfsPath::parent)
                .map(|p| display_location(&p))
                .collect(),
        };
        lines.join("\n")
    }
}

/// A listing format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Comma separated values.
    Csv,
    /// A JSON array of objects.
    Json,
    /// A Markdown table.
    Markdown,
    /// An HTML table.
    Html,
}

impl Format {
    /// The formats in menu order.
    pub const ALL: [Format; 4] = [Format::Csv, Format::Json, Format::Markdown, Format::Html];

    /// The row of the formats menu.
    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Json => "JSON",
            Format::Markdown => "Markdown",
            Format::Html => "HTML",
        }
    }

    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
            Format::Markdown => "md",
            Format::Html => "html",
        }
    }

    /// The format a file name asks for (`list.md`), if its extension says.
    pub fn from_extension(name: &str) -> Option<Format> {
        let ext = name.rsplit_once('.')?.1.to_lowercase();
        Format::ALL
            .into_iter()
            .find(|f| f.extension() == ext || (ext == "htm" && *f == Format::Html))
    }
}

const COLUMNS: [&str; 5] = ["name", "path", "type", "size", "modified"];

fn kind_name(entry: &Entry) -> &'static str {
    match entry.kind {
        EntryKind::File => "file",
        EntryKind::Directory => "folder",
        EntryKind::Symlink => "link",
        EntryKind::Other => "other",
    }
}

fn cells(folder: &VfsPath, entry: &Entry) -> [String; 5] {
    [
        entry.name.clone(),
        display_location(&folder.join(&entry.name)),
        kind_name(entry).to_owned(),
        if entry.is_dir() { String::new() } else { entry.size.to_string() },
        entry.modified.map(local_time).unwrap_or_default(),
    ]
}

/// `entries` of `folder` as text in `format`.
pub fn render(folder: &VfsPath, entries: &[Entry], format: Format) -> String {
    let rows: Vec<[String; 5]> = entries.iter().map(|e| cells(folder, e)).collect();
    match format {
        Format::Csv => {
            let quote = |cell: &str| {
                if cell.contains([',', '"', '\n']) {
                    format!("\"{}\"", cell.replace('"', "\"\""))
                } else {
                    cell.to_owned()
                }
            };
            let mut out = COLUMNS.join(",");
            out.push('\n');
            for row in &rows {
                out.push_str(&row.iter().map(|c| quote(c)).collect::<Vec<_>>().join(","));
                out.push('\n');
            }
            out
        }
        Format::Json => {
            let items: Vec<serde_json::Value> = entries
                .iter()
                .zip(&rows)
                .map(|(entry, row)| {
                    serde_json::json!({
                        "name": row[0],
                        "path": row[1],
                        "type": row[2],
                        "size": (!entry.is_dir()).then_some(entry.size),
                        "modified": row[4],
                    })
                })
                .collect();
            let mut out = serde_json::to_string_pretty(&items).unwrap_or_default();
            out.push('\n');
            out
        }
        Format::Markdown => {
            let escape = |cell: &str| cell.replace('|', "\\|");
            let mut out = format!("| {} |\n", COLUMNS.join(" | "));
            out.push_str(&format!("|{}\n", " --- |".repeat(COLUMNS.len())));
            for row in &rows {
                let cells: Vec<String> = row.iter().map(|c| escape(c)).collect();
                out.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            out
        }
        Format::Html => {
            let escape =
                |cell: &str| cell.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let mut out = String::from("<table>\n<thead><tr>");
            for column in COLUMNS {
                out.push_str(&format!("<th>{column}</th>"));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            for row in &rows {
                out.push_str("<tr>");
                for cell in row {
                    out.push_str(&format!("<td>{}</td>", escape(cell)));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</tbody>\n</table>\n");
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> VfsPath {
        VfsPath::parse("file:///data").unwrap()
    }

    fn entries() -> Vec<Entry> {
        vec![
            Entry::new("a, \"b\".txt", EntryKind::File).with_size(12),
            Entry::new("docs", EntryKind::Directory),
        ]
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        let text = render(&folder(), &entries(), Format::Csv);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("name,path,type,size,modified"));
        assert!(lines.next().unwrap().starts_with("\"a, \"\"b\"\".txt\","));
        assert!(lines.next().unwrap().starts_with("docs,"));
    }

    #[test]
    fn json_has_one_object_per_entry() {
        let text = render(&folder(), &entries(), Format::Json);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value[0]["size"], 12);
        assert_eq!(value[1]["type"], "folder");
        assert!(value[1]["size"].is_null());
    }

    #[test]
    fn markdown_and_html_escape_cells() {
        let entries = vec![Entry::new("a|b<c>&.txt", EntryKind::File)];
        let markdown = render(&folder(), &entries, Format::Markdown);
        assert!(markdown.contains("a\\|b<c>&.txt"));
        assert_eq!(markdown.lines().count(), 3);
        let html = render(&folder(), &entries, Format::Html);
        assert!(html.contains("<td>a|b&lt;c&gt;&amp;.txt</td>"));
    }

    #[test]
    fn formats_are_found_by_file_name() {
        assert_eq!(Format::from_extension("list.MD"), Some(Format::Markdown));
        assert_eq!(Format::from_extension("x.htm"), Some(Format::Html));
        assert_eq!(Format::from_extension("x.txt"), None);
        assert_eq!(Format::from_extension("csv"), None);
    }

    #[test]
    fn path_variants() {
        let paths = vec![folder().join("a.txt"), folder().join("b.txt")];
        assert_eq!(PathVariant::Name.text(&paths), "a.txt\nb.txt");
        assert_eq!(PathVariant::Uri.text(&paths), "file:///data/a.txt\nfile:///data/b.txt");
        assert_eq!(PathVariant::FullPath.text(&paths[..1]), display_location(&paths[0]));
        assert_eq!(PathVariant::Folder.text(&paths), display_location(&folder()));
    }
}
