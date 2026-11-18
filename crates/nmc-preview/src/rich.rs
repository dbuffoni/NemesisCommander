//! Formatted text: what Markdown and Office documents become.
//!
//! A document is a list of [`Block`]s (headings, paragraphs, list items,
//! code, table rows...), each made of styled [`Span`]s. [`layout`] wraps
//! the blocks to a width in columns and returns [`Line`]s that a terminal
//! or a window draws with a fixed-width font.

/// How a piece of text looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    /// Bold (also used for headings and table headers).
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Inline or block code.
    pub code: bool,
    /// A link's text.
    pub link: bool,
    /// Heading level 1-6, 0 for normal text.
    pub heading: u8,
    /// Decoration drawn by the layout (bullets, borders, quote bars).
    pub mark: bool,
    /// Something taken away (red): removed lines, files to delete.
    pub removed: bool,
}

/// Text with one style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The text, without line breaks.
    pub text: String,
    /// How it looks.
    pub style: Style,
}

impl Span {
    /// Plain text.
    pub fn plain(text: impl Into<String>) -> Self {
        Span { text: text.into(), style: Style::default() }
    }

    /// Text with `style`.
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Span { text: text.into(), style }
    }
}

/// What a block is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A heading of level 1-6.
    Heading(u8),
    /// Running text.
    Paragraph,
    /// A list item at `depth` (0 for the outer list) with its marker
    /// (`•`, `1.`).
    Item {
        /// Nesting, from 0.
        depth: usize,
        /// The bullet or number.
        marker: String,
    },
    /// Preformatted lines, not wrapped.
    Code,
    /// A quotation.
    Quote,
    /// A table: rows of cells, the first row a header when `header`.
    Table {
        /// The cells, row by row.
        rows: Vec<Vec<String>>,
        /// The first row names the columns.
        header: bool,
    },
    /// A horizontal line.
    Rule,
}

/// A paragraph-like unit of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// What it is.
    pub kind: BlockKind,
    /// Its text (empty for tables and rules).
    pub spans: Vec<Span>,
}

impl Block {
    /// A block of `kind` with `spans`.
    pub fn new(kind: BlockKind, spans: Vec<Span>) -> Self {
        Block { kind, spans }
    }

    /// A paragraph of plain text.
    pub fn paragraph(text: impl Into<String>) -> Self {
        Block::new(BlockKind::Paragraph, vec![Span::plain(text)])
    }

    /// A heading of plain text.
    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        let style = Style { bold: true, heading: level, ..Style::default() };
        Block::new(BlockKind::Heading(level), vec![Span::styled(text, style)])
    }

    /// The block's text without styles.
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// One row on screen.
pub type Line = Vec<Span>;

/// The plain text of a line.
pub fn line_text(line: &Line) -> String {
    line.iter().map(|span| span.text.as_str()).collect()
}

/// Lays `blocks` out in `width` columns (at least 20): paragraphs wrap at
/// spaces, code keeps its lines, tables get aligned columns, and blocks
/// are separated by an empty line.
pub fn layout(blocks: &[Block], width: usize) -> Vec<Line> {
    let width = width.max(20);
    let mut lines: Vec<Line> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let tight_list = matches!(block.kind, BlockKind::Item { .. })
            && index > 0
            && matches!(blocks[index - 1].kind, BlockKind::Item { .. });
        if index > 0 && !tight_list {
            lines.push(Vec::new());
        }
        let mark = Style { mark: true, ..Style::default() };
        match &block.kind {
            BlockKind::Heading(level) => {
                let wrapped = wrap(&block.spans, width);
                let longest = wrapped.iter().map(text_width).max().unwrap_or(0);
                lines.extend(wrapped);
                if *level <= 2 {
                    let underline = if *level == 1 { "═" } else { "─" };
                    lines.push(vec![Span::styled(underline.repeat(longest.max(1)), mark)]);
                }
            }
            BlockKind::Paragraph => lines.extend(wrap(&block.spans, width)),
            BlockKind::Item { depth, marker } => {
                let indent = "  ".repeat(*depth);
                let first = format!("{indent}{marker} ");
                let rest = " ".repeat(text_width_str(&first));
                let inner = width.saturating_sub(first.chars().count()).max(10);
                for (n, mut line) in wrap(&block.spans, inner).into_iter().enumerate() {
                    let prefix = if n == 0 { first.clone() } else { rest.clone() };
                    line.insert(0, Span::styled(prefix, mark));
                    lines.push(line);
                }
            }
            BlockKind::Quote => {
                for mut line in wrap(&block.spans, width.saturating_sub(2).max(10)) {
                    line.insert(0, Span::styled("│ ", mark));
                    for span in line.iter_mut().skip(1) {
                        span.style.italic = true;
                    }
                    lines.push(line);
                }
            }
            BlockKind::Code => {
                let code = Style { code: true, ..Style::default() };
                for text in block.text().lines() {
                    lines.push(vec![Span::styled(format!("  {text}"), code)]);
                }
            }
            BlockKind::Table { rows, header } => lines.extend(table(rows, *header, width)),
            BlockKind::Rule => lines.push(vec![Span::styled("─".repeat(width.min(60)), mark)]),
        }
    }
    lines
}

fn text_width_str(text: &str) -> usize {
    text.chars().count()
}

fn text_width(line: &Line) -> usize {
    line.iter().map(|span| text_width_str(&span.text)).sum()
}

/// Wraps styled text at spaces into lines of at most `width` characters;
/// words longer than a line are cut.
fn wrap(spans: &[Span], width: usize) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line: Line = Vec::new();
    let mut used = 0;
    let push = |line: &mut Line, text: &str, style: Style| match line.last_mut() {
        Some(last) if last.style == style => last.text.push_str(text),
        _ => line.push(Span::styled(text, style)),
    };
    for span in spans {
        for (n, part) in span.text.split('\n').enumerate() {
            if n > 0 {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            for word in part.split_inclusive(' ') {
                let mut word = word;
                let length = text_width_str(word.trim_end());
                if used > 0 && used + length > width {
                    trim_end(&mut line);
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                    word = word.trim_start();
                }
                // A word longer than a whole line is cut.
                let mut rest: String = word.to_owned();
                while text_width_str(rest.trim_end()) > width {
                    let head: String = rest.chars().take(width - used).collect();
                    rest = rest.chars().skip(width - used).collect();
                    push(&mut line, &head, span.style);
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                if !rest.is_empty() {
                    used += text_width_str(&rest);
                    push(&mut line, &rest, span.style);
                }
            }
        }
    }
    trim_end(&mut line);
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

fn trim_end(line: &mut Line) {
    while let Some(last) = line.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            line.pop();
        } else {
            break;
        }
    }
}

/// A table with aligned columns, shrunk to `width` by cutting long cells.
fn table(rows: &[Vec<String>], header: bool, width: usize) -> Vec<Line> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Vec::new();
    }
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(text_width_str(cell));
        }
    }
    // Separators take 3 columns each; shrink the widest columns first.
    let budget = width.saturating_sub(3 * (columns - 1)).max(columns * 3);
    while widths.iter().sum::<usize>() > budget {
        let widest = (0..columns).max_by_key(|&c| widths[c]).unwrap_or(0);
        if widths[widest] <= 3 {
            break;
        }
        widths[widest] -= 1;
    }
    let mark = Style { mark: true, ..Style::default() };
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let bold = header && index == 0;
        let mut line = Vec::new();
        for (column, column_width) in widths.iter().enumerate() {
            if column > 0 {
                line.push(Span::styled(" │ ", mark));
            }
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            let mut text: String = cell.chars().take(*column_width).collect();
            if text_width_str(cell) > *column_width && *column_width > 1 {
                text = cell.chars().take(column_width - 1).collect::<String>() + "…";
            }
            let padding = column_width - text_width_str(&text);
            text.push_str(&" ".repeat(padding));
            line.push(Span::styled(text, Style { bold, ..Style::default() }));
        }
        trim_end(&mut line);
        lines.push(line);
        if bold {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            lines.push(vec![Span::styled(rule.join("─┼─"), mark)]);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    #[test]
    fn paragraphs_wrap_at_spaces() {
        let blocks = [Block::paragraph("the quick brown fox jumps over the lazy dog again")];
        let lines = texts(&layout(&blocks, 20));
        assert_eq!(lines, ["the quick brown fox", "jumps over the lazy", "dog again"]);
        assert!(lines.iter().all(|l| l.chars().count() <= 20));
    }

    #[test]
    fn long_words_are_cut() {
        let blocks = [Block::paragraph("a".repeat(45))];
        let lines = texts(&layout(&blocks, 20));
        assert_eq!(lines.iter().map(|l| l.len()).collect::<Vec<_>>(), [20, 20, 5]);
    }

    #[test]
    fn headings_lists_quotes_and_code() {
        let blocks = [
            Block::heading(1, "Title"),
            Block::new(BlockKind::Item { depth: 0, marker: "•".into() }, vec![Span::plain("one")]),
            Block::new(BlockKind::Item { depth: 1, marker: "1.".into() }, vec![Span::plain("two")]),
            Block::new(BlockKind::Quote, vec![Span::plain("wise words")]),
            Block::new(BlockKind::Code, vec![Span::plain("let x = 1;\nlet y = 2;")]),
        ];
        let lines = layout(&blocks, 40);
        assert_eq!(
            texts(&lines),
            [
                "Title",
                "═════",
                "",
                "• one",
                "  1. two",
                "",
                "│ wise words",
                "",
                "  let x = 1;",
                "  let y = 2;"
            ]
        );
        assert!(lines[0][0].style.bold && lines[0][0].style.heading == 1);
        assert!(lines[6][1].style.italic);
        assert!(lines[8][0].style.code);
    }

    #[test]
    fn tables_align_and_shrink() {
        let rows = vec![
            vec!["Room".to_owned(), "Litres".to_owned()],
            vec!["Kitchen".to_owned(), "5".to_owned()],
        ];
        let lines = texts(&layout(
            &[Block::new(BlockKind::Table { rows: rows.clone(), header: true }, Vec::new())],
            40,
        ));
        assert_eq!(lines, ["Room    │ Litres", "────────┼───────", "Kitchen │ 5"]);
        let narrow = texts(&table(&[vec!["a".repeat(30), "b".repeat(30)]], false, 23));
        assert!(narrow[0].chars().count() <= 23, "{narrow:?}");
        assert!(narrow[0].contains('…'));
    }
}
