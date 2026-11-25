//! The command list (F9): every command of the application in one place,
//! found by typing or by category.
//!
//! The list holds the built-in commands, the commands of the installed
//! plugins and the user's scripts. Each row reads `Category/Command` and
//! shows its shortcuts. Plugin commands go in the category their manifest
//! names, or in `Plugins`; scripts are in `Scripts`. Every command except
//! "run the script under the cursor" can have shortcuts of its own.

use nmc_config::{Action, Category};

use crate::app::PluginChoice;

/// The category of plugin commands that name none of the built-in ones.
pub const PLUGINS: &str = "Plugins";
/// The category of the user's scripts.
pub const SCRIPTS: &str = "Scripts";
/// The first category button: no category filter.
pub const ALL: &str = "All";

/// What a command does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A built-in command.
    Action(Action),
    /// A plugin command or a script.
    Plugin(PluginChoice),
    /// A saved search, by name.
    Search(String),
}

/// One row of the command list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    /// The id shortcuts are stored under (`files.copy`, `plugin.tidy.run`,
    /// `script.backup.ts`); empty when it cannot have shortcuts.
    pub id: String,
    /// Category name (`Files`, `Plugins`, ...).
    pub category: String,
    /// Command name (`Copy`).
    pub label: String,
    /// What it does.
    pub target: Target,
}

impl CommandEntry {
    /// A built-in command.
    pub fn action(action: Action) -> Self {
        CommandEntry {
            id: action.id().to_owned(),
            category: action.category().title().to_owned(),
            label: action.label().to_owned(),
            target: Target::Action(action),
        }
    }

    /// `Category/Command`, as the list shows it.
    pub fn name(&self) -> String {
        format!("{}/{}", self.category, self.label)
    }

    /// What it does, in a sentence or two (F1 in the command list).
    pub fn help(&self) -> String {
        match &self.target {
            Target::Action(action) => action.help().to_owned(),
            Target::Search(name) => format!("Runs the saved search \"{name}\" again."),
            Target::Plugin(PluginChoice::Command { dir, command }) => format!(
                "Runs the command \"{command}\" of the plugin in {}. Its plugin.json describes it.",
                dir.display()
            ),
            Target::Plugin(PluginChoice::UserScript(path)) => format!(
                "Runs your script {} on the folders and the selection of the panels.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Target::Plugin(PluginChoice::Store) => {
                "Lists the plugins of the plugin repositories, to install, update or uninstall."
                    .to_owned()
            }
            Target::Plugin(PluginChoice::Script(_)) => {
                "Runs the script under the cursor on the folders and the selection of the panels."
                    .to_owned()
            }
        }
    }

    /// Whether shortcuts can be given to it.
    pub fn can_have_keys(&self) -> bool {
        !self.id.is_empty()
    }

    /// Whether every word of `filter` is in its name or id, ignoring case
    /// and small spelling mistakes (`cop` finds Files/Copy, `files ren`
    /// Files/Rename, `settngs` Settings).
    pub fn matches(&self, filter: &str) -> bool {
        self.score(filter).is_some()
    }

    /// How well it fits `filter` (0 is best), see [`crate::fuzzy::score`].
    pub fn score(&self, filter: &str) -> Option<usize> {
        crate::fuzzy::score(filter, &format!("{} {}", self.name(), self.id))
    }
}

/// The built-in category called `name` (ignoring case), else [`PLUGINS`].
pub fn plugin_category(name: Option<&str>) -> String {
    name.and_then(|name| {
        Category::ALL
            .iter()
            .map(|c| c.title())
            .find(|title| title.eq_ignore_ascii_case(name.trim()))
    })
    .unwrap_or(PLUGINS)
    .to_owned()
}

/// The command list window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPalette {
    /// Every command.
    pub entries: Vec<CommandEntry>,
    /// The category buttons: [`ALL`], then each category that has commands.
    pub categories: Vec<String>,
    /// The chosen button (0 is [`ALL`]).
    pub category: usize,
    /// What was typed.
    pub filter: String,
    /// Highlighted row (among [`CommandPalette::shown`]).
    pub item: usize,
    /// Waiting for the key to give the highlighted command.
    pub recording: bool,
    /// The help of the highlighted command is shown (F1).
    pub help: bool,
}

impl CommandPalette {
    /// A list of `entries`, showing all of them.
    pub fn new(entries: Vec<CommandEntry>) -> Self {
        let mut categories = vec![ALL.to_owned()];
        let mut known: Vec<String> = Category::ALL.iter().map(|c| c.title().to_owned()).collect();
        known.extend([PLUGINS.to_owned(), SCRIPTS.to_owned()]);
        for category in known {
            if entries.iter().any(|e| e.category == category) {
                categories.push(category);
            }
        }
        CommandPalette {
            entries,
            categories,
            category: 0,
            filter: String::new(),
            item: 0,
            recording: false,
            help: false,
        }
    }

    /// The rows listed: those of the chosen category that match the
    /// filter (spelling mistakes allowed), names starting with the filter
    /// first, then the closest fits.
    pub fn shown(&self) -> Vec<&CommandEntry> {
        let category = self.categories.get(self.category).filter(|c| *c != ALL);
        let mut rows: Vec<&CommandEntry> = self
            .entries
            .iter()
            .filter(|e| category.is_none_or(|c| e.category == *c))
            .filter(|e| e.matches(&self.filter))
            .collect();
        let query = self.filter.trim().to_lowercase();
        if rows
            .iter()
            .any(|e| e.score(&self.filter).is_some_and(|s| crate::fuzzy::is_exact(&self.filter, s)))
        {
            rows.retain(|e| {
                e.score(&self.filter).is_some_and(|s| crate::fuzzy::is_exact(&self.filter, s))
            });
        }
        if !query.is_empty() {
            // Names starting with the filter first, then the best fits.
            rows.sort_by_key(|e| {
                (!e.label.to_lowercase().starts_with(&query), e.score(&self.filter))
            });
        }
        rows
    }

    /// The highlighted command, if any row is listed.
    pub fn selected(&self) -> Option<&CommandEntry> {
        self.shown().get(self.item).copied()
    }

    /// Chooses the category button `delta` steps away, wrapping around.
    pub fn move_category(&mut self, delta: isize) {
        let count = self.categories.len() as isize;
        self.set_category((self.category as isize + delta).rem_euclid(count) as usize);
    }

    /// Chooses category button `index` (clamped).
    pub fn set_category(&mut self, index: usize) {
        self.category = index.min(self.categories.len() - 1);
        self.item = 0;
    }

    /// Moves the highlight; one step wraps around, a page stops at the ends.
    pub fn move_item(&mut self, delta: isize) {
        let count = self.shown().len() as isize;
        if count == 0 {
            return;
        }
        let target = self.item as isize + delta;
        self.item =
            if delta.abs() == 1 { target.rem_euclid(count) } else { target.clamp(0, count - 1) }
                as usize;
    }

    /// Highlights row `index` (clamped).
    pub fn set_item(&mut self, index: usize) {
        self.item = index.min(self.shown().len().saturating_sub(1));
    }

    /// Adds a typed character to the filter.
    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.item = 0;
    }

    /// Removes the last character of the filter.
    pub fn erase(&mut self) {
        self.filter.pop();
        self.item = 0;
    }

    /// Empties the filter.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.item = 0;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn palette() -> CommandPalette {
        let mut entries: Vec<CommandEntry> =
            Action::ALL.iter().map(|&a| CommandEntry::action(a)).collect();
        entries.push(CommandEntry {
            id: "plugin.tidy.run".into(),
            category: plugin_category(None),
            label: "Tidy this folder".into(),
            target: Target::Plugin(PluginChoice::Command {
                dir: PathBuf::from("/p/tidy"),
                command: "run".into(),
            }),
        });
        CommandPalette::new(entries)
    }

    #[test]
    fn categories_come_from_the_commands() {
        let palette = palette();
        assert_eq!(palette.categories[0], ALL);
        assert_eq!(palette.categories[1], "Files");
        assert_eq!(palette.categories.last().unwrap(), PLUGINS, "no scripts: no button");
        assert_eq!(palette.shown().len(), Action::ALL.len() + 1);
        assert_eq!(plugin_category(Some("files")), "Files");
        assert_eq!(plugin_category(Some("Images")), PLUGINS);
    }

    #[test]
    fn a_misspelled_filter_still_finds_the_command() {
        let mut palette = palette();
        for c in "settngs".chars() {
            palette.type_char(c);
        }
        assert_eq!(palette.selected().unwrap().target, Target::Action(Action::Settings));
    }

    #[test]
    fn typing_and_categories_narrow_the_list() {
        let mut palette = palette();
        for c in "settings".chars() {
            palette.type_char(c);
        }
        assert_eq!(palette.selected().unwrap().target, Target::Action(Action::Settings));
        assert!(palette.shown().iter().all(|e| e.matches("settings")));
        palette.clear_filter();
        palette.move_category(-1);
        assert_eq!(palette.shown().len(), 1, "Plugins");
        assert_eq!(palette.selected().unwrap().name(), "Plugins/Tidy this folder");
        palette.move_category(1);
        assert_eq!(palette.category, 0, "wraps to All");
        palette.set_category(1);
        assert!(palette.shown().iter().all(|e| e.category == "Files"));
        for c in "zzz".chars() {
            palette.type_char(c);
        }
        assert!(palette.selected().is_none());
        palette.move_item(1);
        assert_eq!(palette.item, 0);
    }

    #[test]
    fn words_match_category_and_command() {
        let copy = CommandEntry::action(Action::Copy);
        assert_eq!(copy.name(), "Files/Copy");
        assert!(copy.matches("FILES cop"));
        assert!(copy.matches("files/copy"));
        assert!(!copy.matches("view cop"));
    }

    #[test]
    fn every_command_explains_itself() {
        for entry in palette().entries {
            let help = entry.help();
            assert!(help.len() > 20 && help.ends_with('.'), "{}: {help}", entry.name());
        }
    }

    #[test]
    fn pages_stop_at_the_ends() {
        let mut palette = palette();
        palette.move_item(-1);
        assert_eq!(palette.item, palette.shown().len() - 1, "one step wraps");
        palette.move_item(10);
        assert_eq!(palette.item, palette.shown().len() - 1);
        palette.move_item(-10_000);
        assert_eq!(palette.item, 0);
    }
}
