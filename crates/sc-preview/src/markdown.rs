//! Markdown (CommonMark with tables and strikethrough) to [`Block`]s.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::rich::{Block, BlockKind, Span, Style};

/// The blocks of a Markdown document.
pub fn parse(text: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut builder = Builder::default();
    for event in Parser::new_ext(text, options) {
        builder.event(event);
    }
    builder.blocks
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    /// The block being written, if any.
    current: Option<Block>,
    style: Style,
    /// For each open list: the next number, or `None` for bullets.
    lists: Vec<Option<u64>>,
    quote: usize,
    /// The table being read: its rows and the cell being written.
    table: Option<(Vec<Vec<String>>, String)>,
    /// A link's target, shown after its text when they differ.
    link: Option<String>,
}

impl Builder {
    fn open(&mut self, kind: BlockKind) {
        self.close();
        self.current = Some(Block::new(kind, Vec::new()));
    }

    fn close(&mut self) {
        if let Some(block) = self.current.take() {
            let empty = block.spans.iter().all(|s| s.text.trim().is_empty());
            if !empty || block.kind == BlockKind::Rule {
                self.blocks.push(block);
            }
        }
    }

    fn text(&mut self, text: &str) {
        if let Some((_, cell)) = &mut self.table {
            cell.push_str(text);
            return;
        }
        if self.current.is_none() {
            let kind = if self.quote > 0 { BlockKind::Quote } else { BlockKind::Paragraph };
            self.current = Some(Block::new(kind, Vec::new()));
        }
        let style = self.style;
        if let Some(block) = &mut self.current {
            match block.spans.last_mut() {
                Some(last) if last.style == style => last.text.push_str(text),
                _ => block.spans.push(Span::styled(text, style)),
            }
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                let saved = self.style;
                self.style.code = true;
                self.text(&code);
                self.style = saved;
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.text("\n"),
            Event::Rule => {
                self.close();
                self.blocks.push(Block::new(BlockKind::Rule, Vec::new()));
            }
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::Html(html) | Event::InlineHtml(html) => self.text(html.trim_end_matches('\n')),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                let level = heading_level(level);
                self.open(BlockKind::Heading(level));
                self.style = Style { bold: true, heading: level, ..Style::default() };
            }
            Tag::Paragraph => {
                // A paragraph inside a list item continues the item.
                let in_item = matches!(&self.current, Some(b) if matches!(b.kind, BlockKind::Item { .. }) && b.spans.is_empty());
                if !in_item {
                    let kind = if self.quote > 0 { BlockKind::Quote } else { BlockKind::Paragraph };
                    self.open(kind);
                }
            }
            Tag::BlockQuote(_) => {
                self.close();
                self.quote += 1;
            }
            Tag::CodeBlock(kind) => {
                self.open(BlockKind::Code);
                if let CodeBlockKind::Fenced(language) = kind {
                    let _ = language;
                }
            }
            Tag::List(start) => {
                self.close();
                self.lists.push(start);
            }
            Tag::Item => {
                let depth = self.lists.len().saturating_sub(1);
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => ["•", "◦", "▪"][depth % 3].to_owned(),
                };
                self.open(BlockKind::Item { depth, marker });
            }
            Tag::Table(_) => {
                self.close();
                self.table = Some((Vec::new(), String::new()));
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some((rows, _)) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Tag::Emphasis => self.style.italic = true,
            Tag::Strong => self.style.bold = true,
            Tag::Link { dest_url, .. } => {
                self.style.link = true;
                self.link = Some(dest_url.to_string());
            }
            Tag::Image { dest_url, .. } => {
                self.text("[image: ");
                self.link = Some(dest_url.to_string());
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.close();
                self.style = Style::default();
            }
            TagEnd::Paragraph | TagEnd::CodeBlock | TagEnd::Item => self.close(),
            TagEnd::BlockQuote(_) => {
                self.close();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.close();
                self.lists.pop();
            }
            TagEnd::TableCell => {
                if let Some((rows, cell)) = &mut self.table
                    && let Some(row) = rows.last_mut()
                {
                    row.push(std::mem::take(cell).trim().to_owned());
                }
            }
            TagEnd::Table => {
                if let Some((rows, _)) = self.table.take() {
                    self.blocks
                        .push(Block::new(BlockKind::Table { rows, header: true }, Vec::new()));
                }
            }
            TagEnd::Emphasis => self.style.italic = false,
            TagEnd::Strong => self.style.bold = false,
            TagEnd::Link => {
                self.style.link = false;
                if let Some(target) = self.link.take() {
                    let shown =
                        self.current.as_ref().and_then(|b| b.spans.last()).map(|s| s.text.clone());
                    if shown.as_deref() != Some(target.as_str()) && !target.starts_with('#') {
                        self.text(&format!(" <{target}>"));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(target) = self.link.take() {
                    self.text(&format!(" {target}]"));
                }
            }
            _ => {}
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::{layout, line_text};

    #[test]
    fn a_document_becomes_blocks() {
        let blocks = parse(
            "# Notes\n\nSome **bold** and `code`, a [link](https://x.org).\n\n\
             - one\n- two\n  1. nested\n\n> quoted\n\n```sh\nls -l\n```\n\n\
             | A | B |\n|---|---|\n| 1 | 2 |\n\n---\n",
        );
        let kinds: Vec<&BlockKind> = blocks.iter().map(|b| &b.kind).collect();
        assert!(matches!(kinds[0], BlockKind::Heading(1)));
        assert!(matches!(kinds[1], BlockKind::Paragraph));
        assert!(matches!(kinds[2], BlockKind::Item { depth: 0, .. }));
        assert!(matches!(kinds[4], BlockKind::Item { depth: 1, marker } if marker == "1."));
        assert!(matches!(kinds[5], BlockKind::Quote));
        assert!(matches!(kinds[6], BlockKind::Code));
        assert!(matches!(kinds[7], BlockKind::Table { header: true, .. }));
        assert!(matches!(kinds[8], BlockKind::Rule));

        let paragraph = &blocks[1];
        assert_eq!(paragraph.text(), "Some bold and code, a link <https://x.org>.");
        assert!(paragraph.spans.iter().any(|s| s.text == "bold" && s.style.bold));
        assert!(paragraph.spans.iter().any(|s| s.text == "code" && s.style.code));
        assert!(paragraph.spans.iter().any(|s| s.text == "link" && s.style.link));

        let text: Vec<String> = layout(&blocks, 60).iter().map(line_text).collect();
        assert!(text.contains(&"  ls -l".to_owned()), "{text:?}");
        assert!(text.contains(&"A │ B".to_owned()), "{text:?}");
    }
}
