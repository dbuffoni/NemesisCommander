//! E-books as formatted text: EPUB (a zip of XHTML chapters in the order
//! of its package file) and FictionBook 2 (`.fb2`, one XML file). Pictures,
//! styles and fonts are not kept: this is a preview to read.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesRef, BytesStart, Event};

use crate::rich::{Block, BlockKind, Span, Style};

/// Chapters read at most (a preview is not a reader of 10 000 pages).
const CHAPTER_LIMIT: usize = 400;
/// Blocks kept at most.
const BLOCK_LIMIT: usize = 60_000;

/// The e-book formats this module reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.epub`.
    Epub,
    /// `.fb2` (FictionBook 2), also `.fb2.zip` is not read.
    Fb2,
}

impl Format {
    /// The format of a file name, if this module reads it.
    pub fn of(name: &str) -> Option<Format> {
        match name.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
            "epub" => Some(Format::Epub),
            "fb2" => Some(Format::Fb2),
            _ => None,
        }
    }
}

/// The text of an e-book.
pub fn read(format: Format, data: &[u8]) -> Result<Vec<Block>, String> {
    let blocks = match format {
        Format::Epub => epub(data)?,
        Format::Fb2 => fb2(data)?,
    };
    if blocks.is_empty() {
        return Err("the book has no text".into());
    }
    Ok(blocks)
}

fn local(element: &BytesStart<'_>) -> String {
    element.local_name().as_ref().to_ascii_lowercase()
}

fn attribute(element: &BytesStart<'_>, name: &str) -> Option<String> {
    let found = element.attributes().flatten().find(|a| a.key.local_name().as_ref() == name)?;
    Some(found.value.to_string())
}

/// The text of `&name;` and `&#N;`.
fn entity(reference: &BytesRef<'_>) -> String {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return c.to_string();
    }
    match reference.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "mdash" => "—",
        "ndash" => "–",
        "hellip" => "…",
        "ldquo" => "“",
        "rdquo" => "”",
        "lsquo" => "‘",
        "rsquo" => "’",
        "laquo" => "«",
        "raquo" => "»",
        "copy" => "©",
        "middot" => "·",
        "bull" => "•",
        _ => "",
    }
    .to_owned()
}

/// Collects blocks from a stream of text, with the current style.
#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    current: Option<Block>,
    style: Style,
    /// Open `<pre>`: line breaks are kept.
    pre: bool,
}

impl Builder {
    fn start(&mut self, kind: BlockKind) {
        self.end();
        self.current = Some(Block::new(kind, Vec::new()));
    }

    fn end(&mut self) {
        let Some(mut block) = self.current.take() else { return };
        if !matches!(block.kind, BlockKind::Code) {
            // Collapse spaces at the edges.
            if let Some(first) = block.spans.first_mut() {
                first.text = first.text.trim_start().to_owned();
            }
            if let Some(last) = block.spans.last_mut() {
                last.text = last.text.trim_end().to_owned();
            }
        }
        if !block.text().trim().is_empty() && self.blocks.len() < BLOCK_LIMIT {
            self.blocks.push(block);
        }
    }

    fn text(&mut self, text: &str) {
        let text = if self.pre {
            text.to_owned()
        } else {
            // Runs of white space (including the line breaks of the source)
            // are one space.
            let mut out = String::with_capacity(text.len());
            let mut space = self.current.as_ref().is_none_or(|b| b.text().ends_with(' '));
            for c in text.chars() {
                if c.is_whitespace() {
                    if !space {
                        out.push(' ');
                    }
                    space = true;
                } else {
                    out.push(c);
                    space = false;
                }
            }
            out
        };
        if text.is_empty() {
            return;
        }
        if self.current.is_none() {
            self.current = Some(Block::new(BlockKind::Paragraph, Vec::new()));
        }
        let style = self.style;
        if let Some(block) = &mut self.current {
            match block.spans.last_mut() {
                Some(last) if last.style == style => last.text.push_str(&text),
                _ => block.spans.push(Span::styled(text, style)),
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.end();
        self.blocks
    }
}

fn heading_style(level: u8) -> Style {
    Style { bold: true, heading: level, ..Style::default() }
}

// ----------------------------------------------------------------- EPUB

fn member(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = archive.by_name(name).map_err(|_| format!("{name} is missing"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// `href` made relative to the folder of the package file, without `#…`,
/// and with `%20`-style escapes undone.
fn resolve(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or("");
    let mut decoded = Vec::new();
    let bytes = href.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = href.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            decoded.push(v);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let href = String::from_utf8_lossy(&decoded).into_owned();
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The package file and what it lists: title, author, chapters in order.
struct Package {
    title: String,
    authors: Vec<String>,
    language: String,
    chapters: Vec<String>,
}

fn package(xml: &str, base: &str) -> Result<Package, String> {
    let mut reader = Reader::from_str(xml);
    let mut title = String::new();
    let mut authors = Vec::new();
    let mut language = String::new();
    let mut manifest: HashMap<String, (String, String)> = HashMap::new();
    let mut spine: Vec<String> = Vec::new();
    let mut field: Option<String> = None;
    loop {
        match reader.read_event().map_err(|e| format!("damaged package: {e}"))? {
            Event::Start(e) | Event::Empty(e) => {
                let name = local(&e);
                match name.as_str() {
                    "item" => {
                        if let (Some(id), Some(href)) = (attribute(&e, "id"), attribute(&e, "href"))
                        {
                            let kind = attribute(&e, "media-type").unwrap_or_default();
                            manifest.insert(id, (resolve(base, &href), kind));
                        }
                    }
                    "itemref" => {
                        if attribute(&e, "linear").as_deref() != Some("no")
                            && let Some(id) = attribute(&e, "idref")
                        {
                            spine.push(id);
                        }
                    }
                    "title" | "creator" | "language" => field = Some(name),
                    _ => {}
                }
            }
            Event::Text(t) => {
                let text = t.xml10_content().trim().to_owned();
                match field.as_deref() {
                    Some("title") if title.is_empty() => title = text,
                    Some("creator") if !text.is_empty() => authors.push(text),
                    Some("language") if language.is_empty() => language = text,
                    _ => {}
                }
            }
            Event::End(_) => field = None,
            Event::Eof => break,
            _ => {}
        }
    }
    let chapters = spine
        .iter()
        .filter_map(|id| manifest.get(id))
        .filter(|(_, kind)| kind.contains("html") || kind.contains("xml"))
        .map(|(path, _)| path.clone())
        .collect();
    Ok(Package { title, authors, language, chapters })
}

fn epub(data: &[u8]) -> Result<Vec<Block>, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("not an EPUB file: {e}"))?;
    let container = member(&mut archive, "META-INF/container.xml")
        .map_err(|e| format!("not an EPUB file: {e}"))?;
    let container = String::from_utf8_lossy(&container).into_owned();
    let mut reader = Reader::from_str(&container);
    let mut root = None;
    loop {
        match reader.read_event().map_err(|e| format!("damaged EPUB: {e}"))? {
            Event::Start(e) | Event::Empty(e) if local(&e) == "rootfile" => {
                root = attribute(&e, "full-path");
                break;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let root = root.ok_or("not an EPUB file: no package file")?;
    let base = root.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
    let opf = String::from_utf8_lossy(&member(&mut archive, &root)?).into_owned();
    let package = package(&opf, &base)?;

    let mut blocks = Vec::new();
    if !package.title.is_empty() {
        blocks.push(Block::heading(1, package.title.clone()));
    }
    let mut about = Vec::new();
    if !package.authors.is_empty() {
        about.push(format!("by {}", package.authors.join(", ")));
    }
    if !package.language.is_empty() {
        about.push(format!("language: {}", package.language));
    }
    about.push(format!("{} sections", package.chapters.len()));
    blocks.push(Block::paragraph(about.join(" · ")));
    blocks.push(Block::new(BlockKind::Rule, Vec::new()));
    for chapter in package.chapters.iter().take(CHAPTER_LIMIT) {
        let Ok(bytes) = member(&mut archive, chapter) else { continue };
        blocks.extend(xhtml(&String::from_utf8_lossy(&bytes)));
        if blocks.len() >= BLOCK_LIMIT {
            break;
        }
    }
    if package.chapters.len() > CHAPTER_LIMIT {
        blocks.push(Block::paragraph(format!(
            "… {} more sections are not shown",
            package.chapters.len() - CHAPTER_LIMIT
        )));
    }
    Ok(blocks)
}

/// One chapter (XHTML) as blocks.
pub fn xhtml(xml: &str) -> Vec<Block> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;
    let mut out = Builder::default();
    // Elements whose text is not read (inside `head`, scripts, styles).
    let mut skip = 0usize;
    let mut quote = 0usize;
    let mut list: Vec<(bool, usize)> = Vec::new();
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => {
                let name = local(&e);
                match name.as_str() {
                    "head" | "script" | "style" | "svg" => skip += 1,
                    _ if skip > 0 => {}
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = name[1..].parse().unwrap_or(2);
                        out.start(BlockKind::Heading(level));
                        out.style = heading_style(level);
                    }
                    "p" | "div" | "section" | "article" | "dd" | "dt" | "figcaption" => {
                        out.start(if quote > 0 { BlockKind::Quote } else { BlockKind::Paragraph })
                    }
                    "blockquote" => {
                        quote += 1;
                        out.start(BlockKind::Quote);
                    }
                    "pre" => {
                        out.start(BlockKind::Code);
                        out.pre = true;
                    }
                    "ul" | "ol" => list.push((name == "ol", 0)),
                    "li" => {
                        let marker = match list.last_mut() {
                            Some((true, n)) => {
                                *n += 1;
                                format!("{n}.")
                            }
                            _ => "•".to_owned(),
                        };
                        out.start(BlockKind::Item { depth: list.len().saturating_sub(1), marker });
                    }
                    "tr" => out.start(BlockKind::Paragraph),
                    "td" | "th" => out.text(" │ "),
                    "b" | "strong" => out.style.bold = true,
                    "i" | "em" | "cite" => out.style.italic = true,
                    "code" | "tt" => out.style.code = true,
                    "a" => out.style.link = true,
                    _ => {}
                }
            }
            Event::Empty(e) => match local(&e).as_str() {
                "br" if skip == 0 => out.text(if out.pre { "\n" } else { " " }),
                "hr" if skip == 0 => {
                    out.end();
                    out.blocks.push(Block::new(BlockKind::Rule, Vec::new()));
                }
                "img" | "image" if skip == 0 => {
                    let alt = attribute(&e, "alt").filter(|a| !a.trim().is_empty());
                    out.text(&format!("[{}]", alt.as_deref().unwrap_or("picture")));
                }
                _ => {}
            },
            Event::End(e) => {
                let name = local_end(&e);
                match name.as_str() {
                    "head" | "script" | "style" | "svg" => skip = skip.saturating_sub(1),
                    _ if skip > 0 => {}
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        out.end();
                        out.style = Style::default();
                    }
                    "p" | "div" | "section" | "article" | "li" | "dd" | "dt" | "figcaption"
                    | "tr" | "table" => out.end(),
                    "blockquote" => {
                        out.end();
                        quote = quote.saturating_sub(1);
                    }
                    "pre" => {
                        out.end();
                        out.pre = false;
                    }
                    "ul" | "ol" => {
                        list.pop();
                    }
                    "b" | "strong" => out.style.bold = false,
                    "i" | "em" | "cite" => out.style.italic = false,
                    "code" | "tt" => out.style.code = false,
                    "a" => out.style.link = false,
                    _ => {}
                }
            }
            Event::Text(t) if skip == 0 => out.text(&t.xml10_content()),
            Event::CData(t) if skip == 0 => out.text(&t.into_inner()),
            Event::GeneralRef(r) if skip == 0 => out.text(&entity(&r)),
            Event::Eof => break,
            _ => {}
        }
    }
    out.finish()
}

fn local_end(element: &quick_xml::events::BytesEnd<'_>) -> String {
    element.local_name().as_ref().to_ascii_lowercase()
}

// ------------------------------------------------------------------ FB2

/// Windows-1251 from 0x80 on.
const CP1251: &str = "ЂЃ‚ѓ„…†‡€‰Љ‹ЊЌЋЏђ‘’“”•–—\u{98}™љ›њќћџ\u{a0}ЎўЈ¤Ґ¦§Ё©Є«¬\u{ad}®Ї°±Ііґµ¶·ё№є»јЅѕїАБВГДЕЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯабвгдежзийклмнопрстуфхцчшщъыьэюя";

/// The text of an FB2 file: UTF-8, or Windows-1251 as declared.
fn fb2_text(data: &[u8]) -> String {
    let head = String::from_utf8_lossy(&data[..data.len().min(200)]).to_ascii_lowercase();
    if head.contains("windows-1251") || head.contains("cp1251") {
        let table: Vec<char> = CP1251.chars().collect();
        data.iter()
            .map(|&b| {
                if b < 0x80 {
                    b as char
                } else {
                    table.get(usize::from(b) - 0x80).copied().unwrap_or('?')
                }
            })
            .collect()
    } else {
        String::from_utf8_lossy(data).into_owned()
    }
}

fn fb2(data: &[u8]) -> Result<Vec<Block>, String> {
    let text = fb2_text(data);
    if !text.contains("<FictionBook") {
        return Err("not an FB2 file".into());
    }
    let mut reader = Reader::from_str(&text);
    reader.config_mut().check_end_names = false;
    let mut out = Builder::default();
    let mut path: Vec<String> = Vec::new();
    let mut section_depth = 0usize;
    let mut authors: Vec<String> = Vec::new();
    let mut author = String::new();
    let mut in_title_info = false;
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => {
                let name = local(&e);
                match name.as_str() {
                    "title-info" => in_title_info = true,
                    "binary" => path.push(name.clone()),
                    "section" => section_depth += 1,
                    "title" if !in_title_info => {
                        let level = (section_depth + 1).clamp(1, 4) as u8;
                        out.start(BlockKind::Heading(level));
                        out.style = heading_style(level);
                    }
                    "book-title" => out.start(BlockKind::Heading(1)),
                    "p" if !path.contains(&"binary".to_owned()) => {
                        let quote = path.iter().any(|p| p == "cite" || p == "epigraph");
                        if out
                            .current
                            .as_ref()
                            .is_none_or(|b| !matches!(b.kind, BlockKind::Heading(_)))
                        {
                            out.start(if quote { BlockKind::Quote } else { BlockKind::Paragraph });
                        }
                    }
                    "v" => out.start(BlockKind::Quote),
                    "emphasis" => out.style.italic = true,
                    "strong" => out.style.bold = true,
                    "code" => out.style.code = true,
                    "first-name" | "middle-name" | "last-name" | "nickname" => {}
                    "annotation" => out.start(BlockKind::Paragraph),
                    _ => {}
                }
                path.push(name);
            }
            Event::Empty(e) => {
                if local(&e) == "empty-line" {
                    out.end();
                }
            }
            Event::End(e) => {
                let name = local_end(&e);
                path.pop();
                match name.as_str() {
                    "title-info" => in_title_info = false,
                    "section" => {
                        out.end();
                        section_depth = section_depth.saturating_sub(1);
                    }
                    "title" if !in_title_info => {
                        out.end();
                        out.style = Style::default();
                    }
                    "book-title" => out.end(),
                    "p" | "v" | "annotation" | "subtitle" => out.end(),
                    "emphasis" => out.style.italic = false,
                    "strong" => out.style.bold = false,
                    "code" => out.style.code = false,
                    "author" if in_title_info => {
                        if !author.trim().is_empty() {
                            authors.push(author.trim().to_owned());
                        }
                        author.clear();
                    }
                    _ => {}
                }
            }
            Event::Text(t) => {
                let text = t.xml10_content();
                let leaf = path.last().map(String::as_str).unwrap_or("");
                if path.iter().any(|p| p == "binary") {
                    continue;
                }
                if in_title_info && path.iter().any(|p| p == "author") {
                    author.push_str(text.trim());
                    author.push(' ');
                } else if in_title_info
                    && !matches!(leaf, "book-title" | "p" | "annotation" | "emphasis" | "strong")
                {
                    continue;
                } else if !in_title_info
                    || matches!(leaf, "book-title" | "p" | "emphasis" | "strong")
                {
                    out.text(&text);
                }
            }
            Event::GeneralRef(r) => out.text(&entity(&r)),
            Event::Eof => break,
            _ => {}
        }
    }
    let mut blocks = out.finish();
    if !authors.is_empty() {
        let at = usize::from(
            matches!(blocks.first(), Some(b) if matches!(b.kind, BlockKind::Heading(1))),
        );
        blocks.insert(at.min(blocks.len()), Block::paragraph(format!("by {}", authors.join(", "))));
    }
    Ok(blocks)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn epub_file() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        let mut add = |name: &str, text: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        };
        add(
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
        );
        add(
            "OEBPS/content.opf",
            r#"<package xmlns:dc="x"><metadata><dc:title>Il libro</dc:title><dc:creator>A. Autore</dc:creator><dc:language>it</dc:language></metadata>
<manifest><item id="c1" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="two.xhtml" media-type="application/xhtml+xml"/><item id="img" href="a.png" media-type="image/png"/></manifest>
<spine><itemref idref="c2"/><itemref idref="c1"/></spine></package>"#,
        );
        add(
            "OEBPS/text/one.xhtml",
            "<html><head><title>x</title><style>p{}</style></head><body><h1>Uno</h1><p>Ciao&nbsp;<b>mondo</b> &mdash; ok.</p><ul><li>a</li><li>b</li></ul></body></html>",
        );
        add(
            "OEBPS/two.xhtml",
            "<html><body><h2>Due</h2><p>Prima\n   riga <i>corsiva</i></p></body></html>",
        );
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn an_epub_is_read_in_spine_order() {
        let blocks = read(Format::Epub, &epub_file()).unwrap();
        let text: Vec<String> = blocks.iter().map(Block::text).collect();
        assert_eq!(text[0], "Il libro");
        assert!(text[1].contains("by A. Autore") && text[1].contains("language: it"), "{text:?}");
        let due = text.iter().position(|t| t == "Due").unwrap();
        let uno = text.iter().position(|t| t == "Uno").unwrap();
        assert!(due < uno, "the spine lists two.xhtml first: {text:?}");
        assert!(text.contains(&"Prima riga corsiva".to_owned()), "{text:?}");
        assert!(
            text.iter().any(|t| t.starts_with("Ciao") && t.contains("mondo — ok.")),
            "{text:?}"
        );
        assert!(!text.iter().any(|t| t.contains("p{}")), "styles are not text");
        assert_eq!(blocks.iter().filter(|b| matches!(b.kind, BlockKind::Item { .. })).count(), 2);
    }

    #[test]
    fn a_file_that_is_not_an_epub_says_so() {
        assert!(read(Format::Epub, b"hello").unwrap_err().contains("not an EPUB"));
    }

    #[test]
    fn an_fb2_book_gives_title_author_and_text() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><FictionBook><description><title-info>
<author><first-name>Anna</first-name><last-name>Rossi</last-name></author><book-title>Titolo</book-title></title-info></description>
<body><section><title><p>Capitolo 1</p></title><p>Testo <emphasis>forte</emphasis>.</p><empty-line/><cite><p>Citazione</p></cite></section></body></FictionBook>"#;
        let blocks = read(Format::Fb2, xml.as_bytes()).unwrap();
        let text: Vec<String> = blocks.iter().map(Block::text).collect();
        assert_eq!(text[0], "Titolo");
        assert_eq!(text[1], "by Anna Rossi");
        assert!(text.contains(&"Capitolo 1".to_owned()), "{text:?}");
        assert!(text.contains(&"Testo forte.".to_owned()), "{text:?}");
        assert!(blocks.iter().any(|b| b.kind == BlockKind::Quote && b.text() == "Citazione"));
    }

    #[test]
    fn windows_1251_fb2_is_decoded() {
        let mut bytes =
            br#"<?xml version="1.0" encoding="windows-1251"?><FictionBook><body><section><p>"#
                .to_vec();
        bytes.extend_from_slice(&[0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2]); // Привет
        bytes.extend_from_slice(b"</p></section></body></FictionBook>");
        let blocks = read(Format::Fb2, &bytes).unwrap();
        assert_eq!(blocks[0].text(), "Привет");
    }

    #[test]
    fn relative_paths_are_resolved() {
        assert_eq!(resolve("OEBPS", "text/one.xhtml#a"), "OEBPS/text/one.xhtml");
        assert_eq!(resolve("OEBPS/text", "../two%20x.xhtml"), "OEBPS/two x.xhtml");
        assert_eq!(resolve("", "a.xhtml"), "a.xhtml");
    }
}
