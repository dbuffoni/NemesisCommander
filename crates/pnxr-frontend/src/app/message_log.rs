//! The message log: every message the status line showed, kept so that it can
//! be read again and copied (a plugin error disappears from the status line
//! at the next key).

use std::time::SystemTime;

use pnxr_panel::format::local_time;

use super::*;

/// The most messages kept.
const KEEP: usize = 500;

/// One message of the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogLine {
    /// When it was shown.
    pub(crate) when: SystemTime,
    /// Shown as an error.
    pub(crate) error: bool,
    /// The text.
    pub(crate) text: String,
}

impl LogLine {
    /// `2026-11-27 20:50  error  text`, the form of the list and of what is
    /// copied.
    fn line(&self) -> String {
        let kind = if self.error { "error" } else { "info " };
        format!("{}  {kind}  {}", local_time(self.when), self.text)
    }
}

impl App {
    /// Keeps `text` for the message log. Progress texts (ending in `…`) are
    /// left out, and so is a repeat of the last message.
    pub(super) fn log_message(&mut self, text: &str, error: bool) {
        if text.trim().is_empty() || (!error && text.ends_with('…')) {
            return;
        }
        if self.messages.last().is_some_and(|last| last.text == text && last.error == error) {
            return;
        }
        self.messages.push(LogLine { when: SystemTime::now(), error, text: text.to_owned() });
        if self.messages.len() > KEEP {
            self.messages.remove(0);
        }
    }

    /// The window with the messages, newest first.
    pub(super) fn open_message_log(&mut self) {
        let lines: Vec<String> = self.messages.iter().rev().map(LogLine::line).collect();
        if lines.is_empty() {
            return self.set_status("No messages yet");
        }
        let mut dialog = ChooseDialog::new(
            &format!("Messages ({})", lines.len()),
            lines.clone(),
            ChoosePurpose::MessageLog(lines),
        );
        dialog.footer = crate::hints::list("copy this one", &["a: copy all"], "close");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// Keys of the message log: `c` and Enter copy the highlighted message,
    /// `a` copies all of them (oldest first).
    pub(super) fn message_log_key(
        &mut self,
        dialog: &mut ChooseDialog,
        press: &KeyPress,
    ) -> Option<bool> {
        let ChoosePurpose::MessageLog(lines) = &dialog.purpose else { return None };
        let plain = press.plain();
        match plain {
            Some(Key::Char('c')) => {
                if let Some(line) = lines.get(dialog.cursor).cloned() {
                    self.copy_text(line, "the message");
                }
                Some(true)
            }
            Some(Key::Char('a')) => {
                let all = lines.iter().rev().cloned().collect::<Vec<_>>().join("\n");
                let count = lines.len();
                self.copy_text(all, &format!("{count} messages"));
                Some(true)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_shows_when_what_and_the_text() {
        let line = LogLine { when: SystemTime::UNIX_EPOCH, error: true, text: "boom".into() };
        assert!(line.line().ends_with("error  boom"), "{}", line.line());
    }
}
