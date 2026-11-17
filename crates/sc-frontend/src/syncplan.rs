//! The plan of a folder synchronization as the viewer shows it, and what it
//! takes to run it.

use sc_ops::{SyncAction, SyncMode, SyncPlan};
use sc_panel::format::human_size;
use sc_preview::rich::{Line, Span, Style};
use sc_vfs::VfsPath;

use crate::app::display_location;

/// A plan waiting for the user's go-ahead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRun {
    /// The left folder (the active panel).
    pub left: VfsPath,
    /// The right folder (the other panel).
    pub right: VfsPath,
    /// How they are made to agree.
    pub mode: SyncMode,
    /// What would happen.
    pub plan: SyncPlan,
}

impl SyncRun {
    /// The name of the mode for titles.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            SyncMode::Mirror => "mirror",
            SyncMode::Update => "update",
            SyncMode::TwoWay => "two-way",
        }
    }

    /// The window title.
    pub fn title(&self) -> String {
        format!(
            "Synchronize ({}): {} → {}",
            self.mode_name(),
            display_location(&self.left),
            display_location(&self.right)
        )
    }

    /// The plan as colored lines: green for new files, blue for replaced
    /// ones, red for deletions.
    pub fn lines(&self) -> Vec<Line> {
        let plain = Style::default();
        let new = Style { code: true, ..plain };
        let replace = Style { link: true, ..plain };
        let delete = Style { removed: true, ..plain };
        let bold = Style { bold: true, ..plain };
        let plan = &self.plan;
        let mut lines: Vec<Line> = Vec::new();
        let copies = plan
            .items
            .iter()
            .filter(|i| matches!(i.action, SyncAction::CopyToRight | SyncAction::CopyToLeft));
        let summary = format!(
            "{} to copy ({} replace an older file), {} to delete, {} not decided, {} already the same",
            copies.clone().count(),
            copies.filter(|i| i.replaces).count(),
            plan.count(SyncAction::DeleteRight),
            plan.count(SyncAction::Conflict),
            plan.unchanged,
        );
        lines.push(vec![Span::styled(summary, bold)]);
        lines.push(vec![Span::plain(if plan.has_work() {
            "Enter runs it, Esc cancels. Nothing has been changed yet."
        } else {
            "Nothing to do. Esc closes."
        })]);
        lines.push(Vec::new());
        for item in &plan.items {
            let (word, style) = match (item.action, item.replaces) {
                (SyncAction::CopyToRight, false) => ("→ new     ", new),
                (SyncAction::CopyToRight, true) => ("→ replace ", replace),
                (SyncAction::CopyToLeft, false) => ("← new     ", new),
                (SyncAction::CopyToLeft, true) => ("← replace ", replace),
                (SyncAction::DeleteRight, _) => ("✗ delete  ", delete),
                (SyncAction::Conflict, _) => ("? differs  ", bold),
            };
            let what = if item.is_dir {
                format!("{}/", item.path)
            } else if item.size > 0 {
                format!("{}  ({})", item.path, human_size(item.size))
            } else {
                item.path.clone()
            };
            lines.push(vec![Span::styled(word, style), Span::styled(what, style)]);
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use sc_ops::SyncItem;

    use super::*;

    #[test]
    fn the_plan_reads_line_by_line() {
        let item = |action, path: &str, is_dir, replaces| SyncItem {
            action,
            path: path.into(),
            is_dir,
            size: 2048,
            replaces,
        };
        let run = SyncRun {
            left: VfsPath::parse("file:///a").unwrap(),
            right: VfsPath::parse("file:///b").unwrap(),
            mode: SyncMode::Mirror,
            plan: SyncPlan {
                items: vec![
                    item(SyncAction::CopyToRight, "new.txt", false, false),
                    item(SyncAction::CopyToRight, "old.txt", false, true),
                    item(SyncAction::DeleteRight, "gone", true, false),
                ],
                unchanged: 4,
            },
        };
        let text: Vec<String> = run
            .lines()
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        assert_eq!(
            text[0],
            "2 to copy (1 replace an older file), 1 to delete, 0 not decided, 4 already the same"
        );
        assert_eq!(text[3], "→ new     new.txt  (2.0 KiB)");
        assert_eq!(text[4], "→ replace old.txt  (2.0 KiB)");
        assert_eq!(text[5], "✗ delete  gone/");
        let styles: Vec<Style> = run.lines()[3..].iter().map(|l| l[0].style).collect();
        assert!(styles[0].code && styles[1].link && styles[2].removed);
        assert!(run.title().starts_with("Synchronize (mirror): "));
    }
}
