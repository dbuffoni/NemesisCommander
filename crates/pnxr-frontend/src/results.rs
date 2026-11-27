//! A list of results a plugin hands over (`pnxr.showResults`): rows of files
//! with columns, marks, a sort and the actions of the plugin on the marked
//! files. The image-duplicates plugin shows its groups of similar pictures
//! this way.
//!
//! The plugin prints one line, [`MARKER`] followed by JSON, which the front
//! end turns into a list it can sort and mark. Shown as a list, it works the
//! same in the window and in the terminal.

use pnxr_vfs::VfsPath;
use serde::Deserialize;

/// Starts the line of a script's output that carries results.
pub const MARKER: &str = "\u{1e}pnxr-results ";

/// What the plugin printed: the results and what can be done with them.
#[derive(Debug, Clone, PartialEq, Deserialize)]
struct Wire {
    title: String,
    columns: Vec<String>,
    rows: Vec<WireRow>,
    #[serde(default)]
    actions: Vec<Action>,
    /// The column the list starts sorted by (0 is the first).
    #[serde(default)]
    sort: usize,
    #[serde(default)]
    descending: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct WireRow {
    path: String,
    cells: Vec<String>,
    /// What each column sorts by (numbers); missing: by its text.
    #[serde(default)]
    values: Vec<f64>,
    /// The copy to keep: "all but the best" marks every other row.
    #[serde(default)]
    keep: bool,
}

/// A command of the plugin that works on the marked files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Action {
    /// The key that runs it (one character).
    pub key: char,
    /// What it does, for the hint line.
    pub title: String,
    /// The plugin command run with the marked files as the selection.
    pub command: String,
}

/// One row of results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The file.
    pub path: VfsPath,
    /// The text of each column.
    pub cells: Vec<String>,
    /// The numbers the columns sort by.
    pub values: Vec<f64>,
    /// The copy to keep.
    pub keep: bool,
    /// Chosen to be acted on.
    pub marked: bool,
}

/// The results of a plugin command, with their marks and sort.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// Window title.
    pub title: String,
    /// The column names.
    pub columns: Vec<String>,
    /// The rows, in the order shown.
    pub rows: Vec<Row>,
    /// The plugin's commands for the marked files.
    pub actions: Vec<Action>,
    /// The column the rows are sorted by.
    pub sort: usize,
    /// Largest first.
    pub descending: bool,
}

/// The results in a script's output, if it printed some.
pub fn find(lines: &[String]) -> Option<Result<Results, String>> {
    let json = lines.iter().find_map(|line| line.strip_prefix(MARKER))?;
    Some(Results::parse(json))
}

fn location(text: &str) -> Result<VfsPath, String> {
    if text.contains("://") { VfsPath::parse(text) } else { VfsPath::from_local(text) }
        .map_err(|e| format!("{text}: {e}"))
}

impl Results {
    /// Reads what the plugin printed.
    pub fn parse(json: &str) -> Result<Results, String> {
        let wire: Wire =
            serde_json::from_str(json).map_err(|e| format!("unexpected results: {e}"))?;
        if wire.columns.is_empty() {
            return Err("the results have no columns".into());
        }
        let mut rows = Vec::with_capacity(wire.rows.len());
        for row in wire.rows {
            if row.cells.len() != wire.columns.len() {
                return Err(format!("{}: a row needs one text for each column", row.path));
            }
            rows.push(Row {
                path: location(&row.path)?,
                cells: row.cells,
                values: row.values,
                keep: row.keep,
                marked: false,
            });
        }
        let mut results = Results {
            title: wire.title,
            sort: wire.sort.min(wire.columns.len() - 1),
            columns: wire.columns,
            rows,
            actions: wire.actions,
            descending: wire.descending,
        };
        results.sort_rows();
        Ok(results)
    }

    /// Puts the rows in the order of the sort column. Rows that are equal
    /// stay in the order the plugin gave (the sort is stable).
    pub fn sort_rows(&mut self) {
        let column = self.sort;
        let descending = self.descending;
        self.rows.sort_by(|a, b| {
            let order = match (a.values.get(column), b.values.get(column)) {
                (Some(x), Some(y)) => x.total_cmp(y),
                _ => a.cells[column].to_lowercase().cmp(&b.cells[column].to_lowercase()),
            };
            if descending { order.reverse() } else { order }
        });
    }

    /// Sorts by the next column (the same direction).
    pub fn next_sort(&mut self) {
        self.sort = (self.sort + 1) % self.columns.len();
        self.sort_rows();
    }

    /// Reverses the order.
    pub fn reverse(&mut self) {
        self.descending = !self.descending;
        self.sort_rows();
    }

    /// How many rows are marked.
    pub fn marked_count(&self) -> usize {
        self.rows.iter().filter(|r| r.marked).count()
    }

    /// The marked files, in the order shown.
    pub fn marked(&self) -> Vec<VfsPath> {
        self.rows.iter().filter(|r| r.marked).map(|r| r.path.clone()).collect()
    }

    /// Marks every row but the ones to keep; when they are all marked
    /// already, takes the marks away instead.
    pub fn toggle_all_but_best(&mut self) {
        let all = self.rows.iter().filter(|r| !r.keep).all(|r| r.marked)
            && self.rows.iter().any(|r| !r.keep);
        for row in &mut self.rows {
            row.marked = !row.keep && !all;
        }
    }

    /// Takes every mark away.
    pub fn unmark_all(&mut self) {
        for row in &mut self.rows {
            row.marked = false;
        }
    }

    /// The lines of the list: a mark, then the columns lined up.
    pub fn lines(&self) -> Vec<String> {
        let widths: Vec<usize> = (0..self.columns.len())
            .map(|c| {
                self.rows
                    .iter()
                    .map(|r| r.cells[c].chars().count())
                    .chain(std::iter::once(self.columns[c].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let last = self.columns.len() - 1;
        self.rows
            .iter()
            .map(|row| {
                let mut line = String::from(if row.marked { "[x] " } else { "[ ] " });
                for (c, cell) in row.cells.iter().enumerate() {
                    if c == last {
                        line.push_str(cell);
                    } else {
                        line.push_str(cell);
                        let pad = widths[c] - cell.chars().count() + 2;
                        line.extend(std::iter::repeat_n(' ', pad));
                    }
                }
                line
            })
            .collect()
    }

    /// The window title: what was found, and how much is marked.
    pub fn heading(&self) -> String {
        let marked = self.marked_count();
        let arrow = if self.descending { "↓" } else { "↑" };
        let sorted = format!("sorted by {} {arrow}", self.columns[self.sort]);
        if marked == 0 {
            format!("{} ({} files, {sorted})", self.title, self.rows.len())
        } else {
            format!("{} ({} files, {marked} marked, {sorted})", self.title, self.rows.len())
        }
    }

    /// The hint line under the list.
    pub fn hint(&self) -> String {
        let mut parts = vec![
            "Space: mark".to_owned(),
            "a: extras".to_owned(),
            "s/r: sort".to_owned(),
            "Delete: trash".to_owned(),
        ];
        parts.extend(self.actions.iter().map(|a| format!("{}: {}", a.key, a.title)));
        parts.push("Esc: close".to_owned());
        parts.join(crate::hints::SEP)
    }

    /// The action of the plugin bound to `key`.
    pub fn action_for(&self, key: char) -> Option<&Action> {
        self.actions.iter().find(|a| a.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Results {
        let json = r##"{
            "title": "Similar pictures",
            "columns": ["Group", "Match", "Pixels", "File"],
            "actions": [{"key": "m", "title": "move aside", "command": "move-marked"}],
            "rows": [
                {"path": "/p/a.jpg", "cells": ["#1", "best", "12 MP", "a.jpg"], "values": [1, 100, 12], "keep": true},
                {"path": "/p/b.jpg", "cells": ["#1", "97%", "2 MP", "b.jpg"], "values": [1, 97, 2]},
                {"path": "/p/c.png", "cells": ["#2", "best", "4 MP", "c.png"], "values": [2, 100, 4], "keep": true},
                {"path": "/p/d.png", "cells": ["#2", "92%", "1 MP", "d.png"], "values": [2, 92, 1]}
            ]
        }"##;
        Results::parse(json).unwrap()
    }

    fn names(results: &Results) -> Vec<&str> {
        results.rows.iter().map(|r| r.path.file_name().unwrap()).collect()
    }

    #[test]
    fn results_are_read_from_what_the_plugin_printed() {
        let lines = vec!["the report".to_owned(), format!("{MARKER}{}", "{}")];
        assert!(matches!(find(&lines), Some(Err(_))), "an empty object is not results");
        assert!(find(&["only text".to_owned()]).is_none());
        let results = sample();
        assert_eq!(results.rows.len(), 4);
        assert_eq!(names(&results), ["a.jpg", "b.jpg", "c.png", "d.png"]);
        assert_eq!(results.action_for('m').unwrap().command, "move-marked");
        assert!(Results::parse(r#"{"title":"x","columns":["A"],"rows":[{"path":"/a","cells":[]}]}"#).is_err());
    }

    #[test]
    fn the_list_sorts_by_each_column_and_both_ways() {
        let mut results = sample();
        results.next_sort(); // Match, ascending
        assert_eq!(names(&results), ["d.png", "b.jpg", "a.jpg", "c.png"]);
        results.reverse();
        assert_eq!(names(&results), ["a.jpg", "c.png", "b.jpg", "d.png"]);
        results.next_sort(); // Pixels, descending
        assert_eq!(names(&results), ["a.jpg", "c.png", "b.jpg", "d.png"]);
        results.next_sort(); // File: sorts by its text, no numbers
        assert_eq!(names(&results), ["d.png", "c.png", "b.jpg", "a.jpg"]);
        results.next_sort(); // back to Group
        assert_eq!(results.sort, 0);
        assert!(results.heading().contains("sorted by Group ↓"), "{}", results.heading());
    }

    #[test]
    fn all_but_the_best_marks_the_extra_copies_and_undoes_itself() {
        let mut results = sample();
        results.toggle_all_but_best();
        assert_eq!(results.marked().iter().map(|p| p.file_name().unwrap().to_owned()).collect::<Vec<_>>(), ["b.jpg", "d.png"]);
        assert!(results.heading().contains("2 marked"), "{}", results.heading());
        results.toggle_all_but_best();
        assert_eq!(results.marked_count(), 0);
        results.toggle_all_but_best();
        results.unmark_all();
        assert_eq!(results.marked_count(), 0);
        // The marks stay with their files when the order changes.
        results.rows[1].marked = true;
        results.next_sort();
        assert_eq!(results.marked().len(), 1);
        assert_eq!(results.marked()[0].file_name(), Some("b.jpg"));
    }

    #[test]
    fn the_lines_line_up_the_columns() {
        let mut results = sample();
        results.rows[1].marked = true;
        let lines = results.lines();
        assert_eq!(lines[0], "[ ] #1     best   12 MP   a.jpg");
        assert_eq!(lines[1], "[x] #1     97%    2 MP    b.jpg");
    }
}
