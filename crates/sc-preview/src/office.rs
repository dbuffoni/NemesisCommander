//! Office documents to [`Block`]s: the text, headings, lists and tables of
//! Word and OpenDocument texts, the slides of PowerPoint and OpenDocument
//! presentations, and the sheets of Excel and OpenDocument spreadsheets.
//! Layout, pictures and fonts are not kept: this is a preview to read.

use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::rich::{Block, BlockKind, Span, Style};

/// Rows and columns of a sheet shown at most.
const SHEET_ROWS: usize = 1000;
const SHEET_COLUMNS: usize = 40;

/// The Office formats this module reads, by file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Word (`.docx`).
    Docx,
    /// OpenDocument text (`.odt`).
    Odt,
    /// PowerPoint (`.pptx`).
    Pptx,
    /// OpenDocument presentation (`.odp`).
    Odp,
    /// Spreadsheets: `.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`.
    Sheet,
}

impl Format {
    /// The format of a file name, if this module reads it.
    pub fn of(name: &str) -> Option<Format> {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match extension.as_str() {
            "docx" | "docm" => Format::Docx,
            "odt" => Format::Odt,
            "pptx" | "pptm" => Format::Pptx,
            "odp" => Format::Odp,
            "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => Format::Sheet,
            _ => return None,
        })
    }
}

/// The contents of an Office file.
pub fn read(format: Format, data: &[u8]) -> Result<Vec<Block>, String> {
    match format {
        Format::Sheet => sheets(data),
        Format::Docx => word(&member(data, "word/document.xml")?),
        Format::Odt => open_document(&member(data, "content.xml")?),
        Format::Odp => open_document(&member(data, "content.xml")?),
        Format::Pptx => slides(data),
    }
}

/// A file inside the zip container of an Office document.
fn member(data: &[u8], name: &str) -> Result<String, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("not an Office file: {e}"))?;
    let mut file =
        archive.by_name(name).map_err(|_| format!("not an Office file: {name} is missing"))?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

/// The local name of an XML element (`w:p` → `p`).
fn local(element: &BytesStart<'_>) -> String {
    element.local_name().as_ref().to_owned()
}

/// The value of attribute `name` (local name) of `element`.
fn attribute(element: &BytesStart<'_>, name: &str) -> Option<String> {
    let found = element.attributes().flatten().find(|a| a.key.local_name().as_ref() == name)?;
    let raw = found.value.to_string();
    Some(quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or(raw))
}

/// Collects blocks: paragraphs with styled runs, and tables.
#[derive(Default)]
struct Collector {
    blocks: Vec<Block>,
    paragraph: Option<Block>,
    style: Style,
    /// Open tables (nested ones are flattened into cells): rows and the
    /// text of the current cell.
    tables: Vec<(Vec<Vec<String>>, String)>,
}

impl Collector {
    fn start_paragraph(&mut self, kind: BlockKind) {
        self.end_paragraph();
        self.paragraph = Some(Block::new(kind, Vec::new()));
    }

    fn end_paragraph(&mut self) {
        let Some(block) = self.paragraph.take() else { return };
        if let Some((_, cell)) = self.tables.last_mut() {
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(block.text().trim());
        } else if !block.text().trim().is_empty() {
            self.blocks.push(block);
        }
    }

    fn text(&mut self, text: &str) {
        if self.paragraph.is_none() {
            self.paragraph = Some(Block::new(BlockKind::Paragraph, Vec::new()));
        }
        let style = self.style;
        if let Some(block) = &mut self.paragraph {
            match block.spans.last_mut() {
                Some(last) if last.style == style => last.text.push_str(text),
                _ => block.spans.push(Span::styled(text, style)),
            }
        }
    }

    fn start_table(&mut self) {
        self.end_paragraph();
        self.tables.push((Vec::new(), String::new()));
    }

    fn start_row(&mut self) {
        if let Some((rows, _)) = self.tables.last_mut() {
            rows.push(Vec::new());
        }
    }

    fn end_cell(&mut self) {
        self.end_paragraph();
        if let Some((rows, cell)) = self.tables.last_mut() {
            let text = std::mem::take(cell);
            if let Some(row) = rows.last_mut() {
                row.push(text);
            }
        }
    }

    fn end_table(&mut self) {
        self.end_paragraph();
        if let Some((rows, _)) = self.tables.pop() {
            let rows: Vec<Vec<String>> = rows.into_iter().filter(|r| !r.is_empty()).collect();
            if rows.is_empty() {
                return;
            }
            if let Some((_, cell)) = self.tables.last_mut() {
                // A table in a table: its text goes into the outer cell.
                let text: Vec<String> = rows.iter().map(|r| r.join(" ")).collect();
                cell.push_str(&text.join(" "));
            } else {
                self.blocks.push(Block::new(BlockKind::Table { rows, header: false }, Vec::new()));
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.end_paragraph();
        while !self.tables.is_empty() {
            self.end_table();
        }
        self.blocks
    }
}

/// `word/document.xml` of a `.docx`.
fn word(xml: &str) -> Result<Vec<Block>, String> {
    let mut reader = Reader::from_str(xml);
    let mut out = Collector::default();
    let mut in_text = false;
    // Run properties seen in the current run.
    let mut in_run_properties = false;
    loop {
        let event = reader.read_event().map_err(|e| format!("damaged document: {e}"))?;
        match event {
            Event::Start(e) | Event::Empty(e) => match local(&e).as_str() {
                "p" => out.start_paragraph(BlockKind::Paragraph),
                "pStyle" => {
                    let style = attribute(&e, "val").unwrap_or_default().to_ascii_lowercase();
                    let level = if style == "title" {
                        Some(1)
                    } else {
                        style.strip_prefix("heading").and_then(|n| n.parse::<u8>().ok())
                    };
                    if let (Some(level), Some(block)) = (level, &mut out.paragraph) {
                        block.kind = BlockKind::Heading(level.clamp(1, 6));
                    }
                }
                "numPr" => {
                    if let Some(block) = &mut out.paragraph {
                        block.kind = BlockKind::Item { depth: 0, marker: "•".into() };
                    }
                }
                "ilvl" => {
                    let depth = attribute(&e, "val").and_then(|v| v.parse().ok()).unwrap_or(0);
                    if let Some(Block { kind: BlockKind::Item { depth: d, marker }, .. }) =
                        &mut out.paragraph
                    {
                        *d = depth;
                        *marker = ["•", "◦", "▪"][depth % 3].into();
                    }
                }
                "r" => out.style = heading_style(&out.paragraph),
                "rPr" => in_run_properties = true,
                "b" if in_run_properties => {
                    out.style.bold = attribute(&e, "val").as_deref() != Some("0")
                }
                "i" if in_run_properties => {
                    out.style.italic = attribute(&e, "val").as_deref() != Some("0")
                }
                "t" => in_text = true,
                "tab" if !in_run_properties => out.text("\t"),
                "br" | "cr" => out.text("\n"),
                "tbl" => out.start_table(),
                "tr" => out.start_row(),
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                "p" => out.end_paragraph(),
                "rPr" => in_run_properties = false,
                "t" => in_text = false,
                "tc" => out.end_cell(),
                "tbl" => out.end_table(),
                _ => {}
            },
            Event::Text(t) if in_text => out.text(&t.xml10_content()),
            Event::GeneralRef(r) if in_text => out.text(&entity(&r)),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out.finish())
}

/// The base style of runs in `paragraph`: headings are bold.
fn heading_style(paragraph: &Option<Block>) -> Style {
    match paragraph {
        Some(Block { kind: BlockKind::Heading(level), .. }) => {
            Style { bold: true, heading: *level, ..Style::default() }
        }
        _ => Style::default(),
    }
}

/// The text of an entity reference such as `&amp;`.
fn entity(reference: &quick_xml::events::BytesRef<'_>) -> String {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return c.to_string();
    }
    match reference.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
    .to_owned()
}

/// `content.xml` of an OpenDocument text or presentation.
fn open_document(xml: &str) -> Result<Vec<Block>, String> {
    let mut reader = Reader::from_str(xml);
    let mut out = Collector::default();
    let mut depth = 0usize;
    let mut text_depth = 0usize;
    let mut pages = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| format!("damaged document: {e}"))?;
        match event {
            Event::Start(e) => match local(&e).as_str() {
                "h" => {
                    let level =
                        attribute(&e, "outline-level").and_then(|l| l.parse().ok()).unwrap_or(1);
                    out.start_paragraph(BlockKind::Heading(level));
                    out.style = Style { bold: true, heading: level, ..Style::default() };
                    text_depth += 1;
                }
                "p" => {
                    let kind = if depth > 0 {
                        BlockKind::Item {
                            depth: depth - 1,
                            marker: ["•", "◦", "▪"][(depth - 1) % 3].into(),
                        }
                    } else {
                        BlockKind::Paragraph
                    };
                    out.start_paragraph(kind);
                    out.style = Style::default();
                    text_depth += 1;
                }
                "list" => depth += 1,
                "page" => {
                    pages += 1;
                    out.end_paragraph();
                    let name = attribute(&e, "name").unwrap_or_else(|| format!("Slide {pages}"));
                    out.blocks.push(Block::heading(2, format!("Slide {pages}: {name}")));
                }
                "table" => out.start_table(),
                "table-row" => out.start_row(),
                _ => {}
            },
            Event::Empty(e) => match local(&e).as_str() {
                "s" => {
                    let count = attribute(&e, "c").and_then(|c| c.parse().ok()).unwrap_or(1usize);
                    out.text(&" ".repeat(count));
                }
                "tab" => out.text("\t"),
                "line-break" => out.text("\n"),
                "table-cell" => {
                    out.end_cell();
                }
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                "h" | "p" => {
                    out.end_paragraph();
                    out.style = Style::default();
                    text_depth = text_depth.saturating_sub(1);
                }
                "list" => depth = depth.saturating_sub(1),
                "table-cell" => out.end_cell(),
                "table" => out.end_table(),
                _ => {}
            },
            Event::Text(t) if text_depth > 0 => out.text(&t.xml10_content()),
            Event::GeneralRef(r) if text_depth > 0 => out.text(&entity(&r)),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out.finish())
}

/// The slides of a `.pptx`, in order, each with its text.
fn slides(data: &[u8]) -> Result<Vec<Block>, String> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(|e| format!("not an Office file: {e}"))?;
    let mut names: Vec<(usize, String)> = archive
        .file_names()
        .filter_map(|name| {
            let name = name.ok()?;
            let number = name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?;
            Some((number.parse().ok()?, name.to_string()))
        })
        .collect();
    names.sort();
    if names.is_empty() {
        return Err("not a presentation: it has no slides".into());
    }
    let mut blocks = Vec::new();
    for (number, name) in names {
        let mut xml = String::new();
        archive
            .by_name(&name)
            .map_err(|e| e.to_string())?
            .read_to_string(&mut xml)
            .map_err(|e| e.to_string())?;
        blocks.push(Block::heading(2, format!("Slide {number}")));
        let mut reader = Reader::from_str(&xml);
        let mut out = Collector::default();
        let mut in_text = false;
        loop {
            match reader.read_event().map_err(|e| format!("damaged presentation: {e}"))? {
                Event::Start(e) => match local(&e).as_str() {
                    "p" => out.start_paragraph(BlockKind::Paragraph),
                    "t" => in_text = true,
                    "tbl" => out.start_table(),
                    "tr" => out.start_row(),
                    _ => {}
                },
                Event::Empty(e) if local(&e) == "br" => out.text("\n"),
                Event::End(e) => match e.local_name().as_ref() {
                    "p" => out.end_paragraph(),
                    "t" => in_text = false,
                    "tc" => out.end_cell(),
                    "tbl" => out.end_table(),
                    _ => {}
                },
                Event::Text(t) if in_text => out.text(&t.xml10_content()),
                Event::GeneralRef(r) if in_text => out.text(&entity(&r)),
                Event::Eof => break,
                _ => {}
            }
        }
        // The first text of a slide is usually its title.
        let mut texts = out.finish();
        if let Some(first) = texts.first_mut() {
            for span in &mut first.spans {
                span.style.bold = true;
            }
        }
        blocks.extend(texts);
    }
    Ok(blocks)
}

/// Every sheet of a workbook as a heading and a table.
fn sheets(data: &[u8]) -> Result<Vec<Block>, String> {
    use calamine::{Data, Reader as _};
    let mut workbook = calamine::open_workbook_auto_from_rs(Cursor::new(data.to_vec()))
        .map_err(|e| format!("not a spreadsheet: {e}"))?;
    let mut blocks = Vec::new();
    for name in workbook.sheet_names() {
        blocks.push(Block::heading(2, name.clone()));
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(e) => {
                blocks.push(Block::paragraph(format!("(cannot read this sheet: {e})")));
                continue;
            }
        };
        let (rows, columns) = range.get_size();
        let mut table: Vec<Vec<String>> = range
            .rows()
            .take(SHEET_ROWS)
            .map(|row| {
                row.iter()
                    .take(SHEET_COLUMNS)
                    .map(|cell| match cell {
                        Data::Empty => String::new(),
                        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => {
                            format!("{}", *f as i64)
                        }
                        Data::DateTime(d) => d.to_string(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<String>>()
            })
            .collect();
        // Trailing empty cells make the table wider than it looks.
        for row in &mut table {
            while row.last().is_some_and(String::is_empty) {
                row.pop();
            }
        }
        if table.iter().all(Vec::is_empty) {
            blocks.push(Block::paragraph("(empty)"));
            continue;
        }
        blocks.push(Block::new(BlockKind::Table { rows: table, header: false }, Vec::new()));
        if rows > SHEET_ROWS || columns > SHEET_COLUMNS {
            blocks.push(Block::paragraph(format!(
                "(showing {} of {rows} rows and {} of {columns} columns)",
                rows.min(SHEET_ROWS),
                columns.min(SHEET_COLUMNS)
            )));
        }
    }
    Ok(blocks)
}
