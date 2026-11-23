//! Side-by-side comparison of two text files (Shift+F3 on two files). The result
//! is shown in the viewer as formatted lines: line numbers, the two texts,
//! and a mark for what changed.

use nmc_preview::rich::{Line, Span, Style};
use similar::{ChangeTag, TextDiff};

/// One row of the comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    /// Left line and its number (from 1).
    pub left: Option<(usize, String)>,
    /// Right line and its number.
    pub right: Option<(usize, String)>,
    /// What the row is.
    pub kind: RowKind,
}

/// What a row of the comparison says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Same on both sides.
    Same,
    /// Only on the left.
    Removed,
    /// Only on the right.
    Added,
    /// Different on the two sides.
    Changed,
}

/// The comparison of two texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    /// Name of the left file.
    pub left_name: String,
    /// Name of the right file.
    pub right_name: String,
    /// The rows.
    pub rows: Vec<DiffRow>,
    /// Index of the first row of each group of changes.
    pub changes: Vec<usize>,
}

impl Diff {
    /// Compares `left` and `right`; `None` when they are the same text.
    pub fn new(left_name: &str, left: &str, right_name: &str, right: &str) -> Option<Diff> {
        if left == right {
            return None;
        }
        let diff = TextDiff::from_lines(left, right);
        let mut rows = Vec::new();
        let mut changes = Vec::new();
        for op in diff.ops() {
            let changes_in_op: Vec<_> = diff.iter_changes(op).collect();
            let mut removed = Vec::new();
            let mut added = Vec::new();
            let flush = |removed: &mut Vec<(usize, String)>,
                         added: &mut Vec<(usize, String)>,
                         rows: &mut Vec<DiffRow>,
                         changes: &mut Vec<usize>| {
                if removed.is_empty() && added.is_empty() {
                    return;
                }
                changes.push(rows.len());
                let pairs = removed.len().max(added.len());
                for i in 0..pairs {
                    let (l, r) = (removed.get(i).cloned(), added.get(i).cloned());
                    let kind = match (&l, &r) {
                        (Some(_), Some(_)) => RowKind::Changed,
                        (Some(_), None) => RowKind::Removed,
                        _ => RowKind::Added,
                    };
                    rows.push(DiffRow { left: l, right: r, kind });
                }
                removed.clear();
                added.clear();
            };
            for change in changes_in_op {
                let text = change.value().trim_end_matches(['\n', '\r']).to_owned();
                match change.tag() {
                    ChangeTag::Equal => {
                        flush(&mut removed, &mut added, &mut rows, &mut changes);
                        rows.push(DiffRow {
                            left: Some((change.old_index().unwrap_or(0) + 1, text.clone())),
                            right: Some((change.new_index().unwrap_or(0) + 1, text)),
                            kind: RowKind::Same,
                        });
                    }
                    ChangeTag::Delete => {
                        removed.push((change.old_index().unwrap_or(0) + 1, text));
                    }
                    ChangeTag::Insert => {
                        added.push((change.new_index().unwrap_or(0) + 1, text));
                    }
                }
            }
            flush(&mut removed, &mut added, &mut rows, &mut changes);
        }
        Some(Diff { left_name: left_name.into(), right_name: right_name.into(), rows, changes })
    }

    /// The rows as lines `columns` wide: `-` removed, `+` added, `~` changed,
    /// then the two numbered texts around a `│`.
    pub fn render(&self, columns: usize) -> Vec<Line> {
        const NUMBER: usize = 5;
        let half = columns.saturating_sub(2 * NUMBER + 5).max(10) / 2;
        let plain = Style::default();
        let mark = Style { mark: true, ..Style::default() };
        let removed = Style { removed: true, ..Style::default() };
        let added = Style { code: true, ..Style::default() };
        // Unchanged lines are coloured as the source they are; changed ones
        // keep the red and green that show the change.
        let language = nmc_preview::syntax::Language::of(&self.left_name, "")
            .or_else(|| nmc_preview::syntax::Language::of(&self.right_name, ""));
        let cell = |side: &Option<(usize, String)>, style: Style| -> Vec<Span> {
            match side {
                Some((n, text)) => {
                    let mut spans = vec![Span::styled(format!("{n:>4} "), mark)];
                    match language.filter(|_| style == plain) {
                        Some(language) => spans.extend(nmc_preview::syntax::highlight_line(
                            language,
                            &fit(text, half),
                        )),
                        None => spans.push(Span::styled(fit(text, half), style)),
                    }
                    spans
                }
                None => vec![Span::plain(" ".repeat(NUMBER + half))],
            }
        };
        let mut lines = Vec::with_capacity(self.rows.len() + 1);
        let header = fit(&self.left_name, half + NUMBER);
        lines.push(vec![Span::styled(
            format!("  {header} │ {}", fit(&self.right_name, half + NUMBER)),
            Style { bold: true, ..Style::default() },
        )]);
        for row in &self.rows {
            let (symbol, left_style, right_style) = match row.kind {
                RowKind::Same => (' ', plain, plain),
                RowKind::Removed => ('-', removed, plain),
                RowKind::Added => ('+', plain, added),
                RowKind::Changed => ('~', removed, added),
            };
            let mut line = vec![Span::styled(format!("{symbol} "), mark)];
            line.extend(cell(&row.left, left_style));
            line.push(Span::styled(" │ ", mark));
            line.extend(cell(&row.right, right_style));
            lines.push(line);
        }
        lines
    }

    /// The line of `render` where change number `index` starts (the header
    /// is line 0).
    pub fn line_of_change(&self, index: usize) -> Option<usize> {
        self.changes.get(index).map(|row| row + 1)
    }
}

/// `text` cut or padded to `width` characters; tabs become four spaces.
fn fit(text: &str, width: usize) -> String {
    let text = text.replace('\t', "    ");
    let count = text.chars().count();
    if count <= width {
        format!("{text:<width$}")
    } else {
        let mut cut: String = text.chars().take(width.saturating_sub(1)).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_has_no_diff() {
        assert!(Diff::new("a", "x\ny\n", "b", "x\ny\n").is_none());
    }

    #[test]
    fn unchanged_source_lines_are_coloured_in_a_comparison() {
        use nmc_preview::rich::Tone;
        let diff =
            Diff::new("a.rs", "fn one() {}\nlet x = 1;\n", "b.rs", "fn one() {}\nlet x = 2;\n")
                .unwrap();
        let lines = diff.render(80);
        let keyword = |line: &Line| line.iter().any(|s| s.style.tone == Tone::Keyword);
        assert!(keyword(&lines[1]), "{:?}", lines[1]);
        // The changed pair keeps its red and green.
        assert!(!keyword(&lines[2]));
        let text: String = lines[1].iter().map(|s| s.text.as_str()).collect();
        assert!(text.contains("fn one() {}"));
    }

    #[test]
    fn changes_are_paired_and_found() {
        let diff =
            Diff::new("a", "one\ntwo\nthree\nfour\n", "b", "one\n2\nthree\nfour\nfive\n").unwrap();
        let kinds: Vec<RowKind> = diff.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [RowKind::Same, RowKind::Changed, RowKind::Same, RowKind::Same, RowKind::Added]
        );
        assert_eq!(diff.changes, [1, 4]);
        assert_eq!(diff.rows[1].left, Some((2, "two".into())));
        assert_eq!(diff.rows[1].right, Some((2, "2".into())));
        assert_eq!(diff.rows[4].right, Some((5, "five".into())));
        let lines = diff.render(60);
        assert_eq!(lines.len(), 6, "header and rows");
        let text: String = lines[2].iter().map(|s| s.text.as_str()).collect();
        assert!(text.starts_with("~    2 two"), "{text}");
        assert!(text.contains("│    2 2"), "{text}");
        assert_eq!(diff.line_of_change(1), Some(5));
    }

    #[test]
    fn removed_lines_have_nothing_on_the_right() {
        let diff = Diff::new("a", "x\ngone\ny\n", "b", "x\ny\n").unwrap();
        assert_eq!(diff.rows[1].kind, RowKind::Removed);
        assert_eq!(diff.rows[1].right, None);
    }
}
