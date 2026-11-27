//! Small outline icons for the known kinds of files.
//!
//! Every icon is one colour, drawn with a bold line and no fill, so it
//! stays quiet next to the name. The window draws the strokes of
//! [`Shape::strokes`] on a 16 × 16 grid; the terminal, which cannot draw
//! shapes, shows [`Shape::glyph`] in the same colour.

use pnxr_vfs::{Entry, EntryKind};

/// What an icon looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shape {
    /// A folder.
    Folder,
    /// A plain file.
    File,
    /// A text page.
    Text,
    /// A document with paragraphs.
    Document,
    /// A table of cells.
    Sheet,
    /// A slide.
    Slides,
    /// A PDF.
    Pdf,
    /// Source code.
    Code,
    /// A shell script or batch file.
    Script,
    /// Structured data or a database.
    Data,
    /// Settings.
    Config,
    /// A picture.
    Picture,
    /// Sound.
    Audio,
    /// A film.
    Video,
    /// A packed archive.
    Archive,
    /// A 3D model or drawing.
    Model,
    /// A font.
    Font,
    /// A program or installer.
    Program,
    /// A disc or disk image.
    Disc,
    /// A key or certificate.
    Key,
    /// An e-book.
    Book,
}

impl Shape {
    /// Every shape.
    pub const ALL: [Shape; 21] = [
        Shape::Folder,
        Shape::File,
        Shape::Text,
        Shape::Document,
        Shape::Sheet,
        Shape::Slides,
        Shape::Pdf,
        Shape::Code,
        Shape::Script,
        Shape::Data,
        Shape::Config,
        Shape::Picture,
        Shape::Audio,
        Shape::Video,
        Shape::Archive,
        Shape::Model,
        Shape::Font,
        Shape::Program,
        Shape::Disc,
        Shape::Key,
        Shape::Book,
    ];

    /// The name the window knows the shape by.
    pub fn id(self) -> &'static str {
        match self {
            Shape::Folder => "folder",
            Shape::File => "file",
            Shape::Text => "text",
            Shape::Document => "document",
            Shape::Sheet => "sheet",
            Shape::Slides => "slides",
            Shape::Pdf => "pdf",
            Shape::Code => "code",
            Shape::Script => "script",
            Shape::Data => "data",
            Shape::Config => "config",
            Shape::Picture => "picture",
            Shape::Audio => "audio",
            Shape::Video => "video",
            Shape::Archive => "archive",
            Shape::Model => "model",
            Shape::Font => "font",
            Shape::Program => "program",
            Shape::Disc => "disc",
            Shape::Key => "key",
            Shape::Book => "book",
        }
    }

    /// The one character the terminal shows.
    pub fn glyph(self) -> char {
        match self {
            Shape::Folder => '▸',
            Shape::File => '·',
            Shape::Text => '≡',
            Shape::Document => '▤',
            Shape::Sheet => '▦',
            Shape::Slides => '▭',
            Shape::Pdf => '▨',
            Shape::Code => '‹',
            Shape::Script => '$',
            Shape::Data => '◈',
            Shape::Config => '≣',
            Shape::Picture => '▲',
            Shape::Audio => '♪',
            Shape::Video => '▶',
            Shape::Archive => '▥',
            Shape::Model => '◆',
            Shape::Font => 'A',
            Shape::Program => '■',
            Shape::Disc => '◎',
            Shape::Key => '¤',
            Shape::Book => '▮',
        }
    }

    /// The lines of the outline on a 16 × 16 grid, as `x, y, x, y, …`
    /// per line; a line that ends where it began is closed.
    pub fn strokes(self) -> Vec<Vec<u8>> {
        // A page with a folded corner.
        let page =
            || vec![vec![3, 2, 10, 2, 13, 5, 13, 14, 3, 14, 3, 2], vec![10, 2, 10, 5, 13, 5]];
        let with = |extra: Vec<Vec<u8>>| {
            let mut lines = page();
            lines.extend(extra);
            lines
        };
        let frame = || vec![2, 3, 14, 3, 14, 13, 2, 13, 2, 3];
        match self {
            Shape::Folder => vec![vec![2, 4, 7, 4, 8, 6, 14, 6, 14, 13, 2, 13, 2, 4]],
            Shape::File => page(),
            Shape::Text => with(vec![vec![5, 8, 11, 8], vec![5, 11, 11, 11]]),
            Shape::Document => with(vec![vec![5, 7, 8, 7], vec![5, 9, 11, 9], vec![5, 11, 11, 11]]),
            Shape::Sheet => with(vec![vec![3, 8, 13, 8], vec![3, 11, 13, 11], vec![8, 8, 8, 14]]),
            Shape::Slides => with(vec![vec![5, 7, 11, 7, 11, 12, 5, 12, 5, 7]]),
            Shape::Pdf => with(vec![vec![5, 12, 5, 7, 9, 7, 9, 10, 5, 10]]),
            Shape::Code => with(vec![vec![7, 7, 5, 10, 7, 13], vec![9, 7, 11, 10, 9, 13]]),
            Shape::Script => {
                vec![frame(), vec![2, 6, 14, 6], vec![4, 8, 6, 10, 4, 12], vec![8, 12, 11, 12]]
            }
            Shape::Data => vec![
                vec![3, 4, 8, 2, 13, 4, 13, 12, 8, 14, 3, 12, 3, 4],
                vec![3, 4, 8, 6, 13, 4],
                vec![3, 8, 8, 10, 13, 8],
            ],
            Shape::Config => vec![
                vec![2, 4, 14, 4],
                vec![2, 8, 14, 8],
                vec![2, 12, 14, 12],
                vec![10, 3, 10, 5],
                vec![5, 7, 5, 9],
                vec![11, 11, 11, 13],
            ],
            Shape::Picture => {
                vec![frame(), vec![2, 11, 6, 7, 9, 10, 11, 8, 14, 11], vec![11, 5, 12, 5]]
            }
            Shape::Audio => vec![
                vec![6, 12, 6, 4, 12, 3, 12, 11],
                vec![4, 12, 6, 14, 8, 12, 6, 10, 4, 12],
                vec![10, 11, 12, 13, 14, 11, 12, 9, 10, 11],
            ],
            Shape::Video => vec![frame(), vec![6, 5, 6, 11, 11, 8, 6, 5]],
            Shape::Archive => vec![
                vec![3, 2, 13, 2, 13, 14, 3, 14, 3, 2],
                vec![7, 4, 9, 4],
                vec![7, 6, 9, 6],
                vec![7, 8, 9, 8],
                vec![6, 10, 10, 10, 10, 13, 6, 13, 6, 10],
            ],
            Shape::Model => vec![
                vec![8, 2, 14, 5, 14, 11, 8, 14, 2, 11, 2, 5, 8, 2],
                vec![2, 5, 8, 8, 14, 5],
                vec![8, 8, 8, 14],
            ],
            Shape::Font => vec![vec![3, 13, 8, 3, 13, 13], vec![5, 10, 11, 10]],
            Shape::Program => vec![frame(), vec![2, 6, 14, 6], vec![4, 4, 5, 4]],
            Shape::Disc => vec![
                vec![8, 2, 12, 4, 14, 8, 12, 12, 8, 14, 4, 12, 2, 8, 4, 4, 8, 2],
                vec![8, 7, 9, 8, 8, 9, 7, 8, 8, 7],
            ],
            Shape::Key => vec![
                vec![5, 5, 7, 6, 8, 8, 7, 10, 5, 11, 3, 10, 2, 8, 3, 6, 5, 5],
                vec![8, 8, 14, 8],
                vec![12, 8, 12, 11],
            ],
            Shape::Book => {
                vec![vec![3, 2, 13, 2, 13, 14, 3, 14, 3, 2], vec![5, 2, 5, 14], vec![7, 5, 11, 5]]
            }
        }
    }
}

/// A shape with its colour (`#rrggbb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Icon {
    /// What it looks like.
    pub shape: Shape,
    /// Its colour.
    pub color: &'static str,
}

const FOLDER: &str = "#4f9de0";
const PLAIN: &str = "#8a8f98";

/// The kinds of files by extension: shape and colour.
const KINDS: &[(&[&str], Shape, &str)] = &[
    (&["pdf"], Shape::Pdf, "#e04b4b"),
    (&["doc", "docx", "odt", "rtf", "pages"], Shape::Document, "#3b78d8"),
    (&["xls", "xlsx", "ods", "csv", "tsv", "numbers"], Shape::Sheet, "#3aa55d"),
    (&["ppt", "pptx", "odp", "key"], Shape::Slides, "#e8833a"),
    (&["txt", "log", "nfo"], Shape::Text, PLAIN),
    (&["md", "markdown", "rst", "tex", "adoc"], Shape::Text, "#2aa198"),
    (
        &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "ico", "heic", "svg", "psd",
            "raw",
        ],
        Shape::Picture,
        "#c25bd6",
    ),
    (&["mp3", "flac", "wav", "ogg", "m4a", "aac", "opus", "wma", "mid"], Shape::Audio, "#d9a621"),
    (&["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v"], Shape::Video, "#e05a8a"),
    (
        &["zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst", "tgz", "jar", "cab"],
        Shape::Archive,
        "#a8743a",
    ),
    (
        &[
            "rs", "py", "js", "mjs", "ts", "tsx", "jsx", "go", "c", "cc", "cpp", "h", "hpp",
            "java", "rb", "php", "cs", "swift", "kt", "lua", "qml", "dart", "scala", "pl", "r",
        ],
        Shape::Code,
        "#8e6fd8",
    ),
    (&["html", "htm", "css", "scss", "vue"], Shape::Code, "#e0703a"),
    (&["sh", "bash", "zsh", "fish", "bat", "cmd", "ps1"], Shape::Script, "#5aa89b"),
    (
        &["json", "xml", "yaml", "yml", "sql", "db", "sqlite", "sqlite3", "parquet"],
        Shape::Data,
        "#26a6c9",
    ),
    (&["toml", "ini", "conf", "cfg", "env", "properties", "desktop"], Shape::Config, "#7e8fa6"),
    (
        &["stl", "obj", "fbx", "3mf", "step", "stp", "dwg", "dxf", "blend", "gltf", "glb", "scad"],
        Shape::Model,
        "#17b890",
    ),
    (&["ttf", "otf", "woff", "woff2"], Shape::Font, "#9aa5b1"),
    (
        &["exe", "msi", "app", "deb", "rpm", "appimage", "dmg", "apk", "com"],
        Shape::Program,
        "#d35f5f",
    ),
    (&["iso", "img", "vhd", "vhdx", "vmdk", "qcow2"], Shape::Disc, "#8f9aa8"),
    (&["pem", "crt", "cer", "gpg", "pub", "asc", "p12", "pfx"], Shape::Key, "#c9a227"),
    (&["epub", "mobi", "azw3", "djvu", "cbz", "cbr"], Shape::Book, "#b5654a"),
];

/// Files known by their whole name.
const NAMES: &[(&str, Shape, &str)] = &[
    ("makefile", Shape::Code, "#8e6fd8"),
    ("dockerfile", Shape::Code, "#8e6fd8"),
    ("cmakelists.txt", Shape::Code, "#8e6fd8"),
    ("readme", Shape::Text, "#2aa198"),
    ("license", Shape::Text, PLAIN),
    ("copying", Shape::Text, PLAIN),
    (".gitignore", Shape::Config, "#7e8fa6"),
    (".env", Shape::Config, "#7e8fa6"),
];

/// The icon of an entry, from its kind and its name's extension.
pub fn icon_for(entry: &Entry) -> Icon {
    if entry.is_dir() {
        return Icon { shape: Shape::Folder, color: FOLDER };
    }
    let name = entry.name.to_lowercase();
    if let Some((_, shape, color)) = NAMES.iter().find(|(n, ..)| *n == name) {
        return Icon { shape: *shape, color };
    }
    if entry.kind != EntryKind::File && entry.kind != EntryKind::Symlink {
        return Icon { shape: Shape::File, color: PLAIN };
    }
    let extension = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    match KINDS.iter().find(|(extensions, ..)| extensions.contains(&extension)) {
        Some((_, shape, color)) => Icon { shape: *shape, color },
        None => Icon { shape: Shape::File, color: PLAIN },
    }
}

/// How many kinds of files have their own icon (not counting the plain one
/// and folders).
pub fn known_kinds() -> usize {
    KINDS.iter().map(|(extensions, ..)| extensions.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(name: &str, kind: EntryKind) -> Icon {
        icon_for(&Entry::new(name, kind))
    }

    #[test]
    fn kinds_have_their_own_shape_and_colour() {
        let file = |name: &str| icon(name, EntryKind::File);
        assert_eq!(file("report.PDF").shape, Shape::Pdf);
        assert_eq!(file("report.pdf").color, "#e04b4b");
        assert_eq!(file("a.xlsx").shape, Shape::Sheet);
        assert_eq!(file("main.rs").shape, Shape::Code);
        assert_eq!(file("model.stl").shape, Shape::Model);
        assert_eq!(file("paper.tex").shape, Shape::Text);
        assert_eq!(file("backup.tar").shape, Shape::Archive);
        assert_eq!(file("Makefile").shape, Shape::Code);
        assert_eq!(file("unknown.zzz").shape, Shape::File);
        assert_eq!(file("noextension").shape, Shape::File);
        assert_eq!(icon("src", EntryKind::Directory).shape, Shape::Folder);
        assert_eq!(
            icon("link.pdf", EntryKind::Symlink).shape,
            Shape::Pdf,
            "links look like their target's kind"
        );
        assert_eq!(icon("pipe.pdf", EntryKind::Other).shape, Shape::File);
    }

    #[test]
    fn more_than_thirty_kinds_are_known() {
        assert!(known_kinds() >= 30, "{}", known_kinds());
    }

    #[test]
    fn every_shape_is_drawn_on_the_grid_with_a_glyph_of_its_own() {
        let mut glyphs = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for shape in Shape::ALL {
            assert!(glyphs.insert(shape.glyph()), "{shape:?} shares its glyph");
            assert!(ids.insert(shape.id()), "{shape:?} shares its id");
            let strokes = shape.strokes();
            assert!(!strokes.is_empty());
            for line in strokes {
                assert!(line.len() >= 4 && line.len() % 2 == 0, "{shape:?}: {line:?}");
                assert!(line.iter().all(|&v| v <= 16), "{shape:?}: {line:?}");
            }
        }
    }

    #[test]
    fn every_colour_is_a_hex_triplet() {
        let colours =
            KINDS.iter().map(|k| k.2).chain(NAMES.iter().map(|n| n.2)).chain([FOLDER, PLAIN]);
        for colour in colours {
            assert!(colour.len() == 7 && colour.starts_with('#'), "{colour}");
            assert!(colour[1..].chars().all(|c| c.is_ascii_hexdigit()), "{colour}");
        }
    }
}
