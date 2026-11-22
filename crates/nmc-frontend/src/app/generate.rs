//! "Generate random text files…": makes files of random words, for tests of
//! transfers, speed and sizes. Asks the name, how many and how big.

use nmc_panel::format::{human_size, parse_size};
use nmc_vfs::{VfsPath, WriteOptions};
use tokio::io::AsyncWriteExt;

use super::{App, AppEvent};
use crate::dialogs::InputPurpose;

/// The most files one request makes.
pub const MAX_FILES: u64 = 10_000;
/// The biggest file one request makes.
pub const MAX_FILE_SIZE: u64 = 4 << 30;

impl App {
    /// First question: the name.
    pub(super) fn generate_files(&mut self) {
        self.ask(
            "Generate random text files",
            "File name (a counter is added when there are several):",
            "random.txt",
            InputPurpose::GenerateName,
        );
    }

    /// An answer of the generator dialogs; asks the next question or starts.
    pub(super) fn generate_step(&mut self, purpose: InputPurpose, text: &str) {
        let text = text.trim();
        match purpose {
            InputPurpose::GenerateName => {
                if text.is_empty() || text.contains('/') {
                    return self.set_error(String::from("The name cannot be empty or contain /"));
                }
                self.ask(
                    "Generate random text files",
                    &format!("How many files named {text}:"),
                    "1",
                    InputPurpose::GenerateCount(text.to_owned()),
                );
            }
            InputPurpose::GenerateCount(name) => match text.parse::<u64>() {
                Ok(count) if (1..=MAX_FILES).contains(&count) => self.ask(
                    "Generate random text files",
                    "Size of each file (for example 10k, 2.5M, 1G):",
                    "1M",
                    InputPurpose::GenerateSize(name, count),
                ),
                _ => self.set_error(format!("How many: a whole number from 1 to {MAX_FILES}")),
            },
            InputPurpose::GenerateSize(name, count) => match parse_size(text) {
                Some(size) if (1..=MAX_FILE_SIZE).contains(&size) => {
                    self.start_generating(name, count, size)
                }
                _ => self.set_error(format!(
                    "Size: a number with an optional unit (k, M, G), at most {}",
                    human_size(MAX_FILE_SIZE)
                )),
            },
            _ => {}
        }
    }

    fn start_generating(&mut self, name: String, count: u64, size: u64) {
        let here = self.workspace.active().location().clone();
        let registry = self.context.registry.clone();
        self.set_status(format!("Writing {count} file(s) of {}…", human_size(size)));
        self.spawn_task(async move {
            let result = write_files(&registry, &here, &name, count, size).await;
            let first = file_name(&name, 1, count);
            match result {
                Ok(()) => AppEvent::TaskDone {
                    message: format!(
                        "Generated {count} file{} of {} in {}",
                        if count == 1 { "" } else { "s" },
                        human_size(size),
                        super::display_location(&here)
                    ),
                    error: false,
                    reload: true,
                    focus: Some(first),
                },
                Err(message) => {
                    AppEvent::TaskDone { message, error: true, reload: true, focus: None }
                }
            }
        });
    }
}

/// The name of file `index` (from 1) of `count`: as typed for one file,
/// else with a counter before the extension (`random-007.txt`).
pub fn file_name(name: &str, index: u64, count: u64) -> String {
    if count <= 1 {
        return name.to_owned();
    }
    let width = count.to_string().len();
    match name.rfind('.').filter(|&dot| dot > 0) {
        Some(dot) => format!("{}-{index:0width$}{}", &name[..dot], &name[dot..]),
        None => format!("{name}-{index:0width$}"),
    }
}

async fn write_files(
    registry: &nmc_vfs::ProviderRegistry,
    folder: &VfsPath,
    name: &str,
    count: u64,
    size: u64,
) -> Result<(), String> {
    let provider = registry.provider_for(folder).await.map_err(|e| e.to_string())?;
    let mut seed = seed();
    for index in 1..=count {
        let file = file_name(name, index, count);
        let path = folder.join(&file);
        let options = WriteOptions { size_hint: Some(size), ..WriteOptions::create_new() };
        let mut sink = provider.write(&path, options).await.map_err(|e| format!("{file}: {e}"))?;
        let mut text = RandomText::new(next(&mut seed));
        let mut left = size;
        while left > 0 {
            let chunk = left.min(1 << 20) as usize;
            let bytes = text.take(chunk);
            sink.write_all(&bytes).await.map_err(|e| format!("{file}: {e}"))?;
            left -= chunk as u64;
        }
        sink.shutdown().await.map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}

fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    nanos | 1
}

fn next(state: &mut u64) -> u64 {
    // xorshift64*
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

const WORDS: [&str; 64] = [
    "alpha", "river", "stone", "light", "north", "quiet", "amber", "delta", "forest", "signal",
    "paper", "orbit", "violet", "market", "harbor", "copper", "silent", "ember", "lantern",
    "meadow", "winter", "bridge", "falcon", "garden", "island", "jungle", "kernel", "marble",
    "nectar", "ocean", "pepper", "quartz", "rocket", "silver", "thunder", "umber", "velvet",
    "willow", "yellow", "zephyr", "anchor", "breeze", "candle", "dragon", "engine", "feather",
    "glacier", "hollow", "ivory", "jasmine", "kettle", "ladder", "mirror", "needle", "orchard",
    "pillar", "ribbon", "saddle", "timber", "update", "voyage", "wander", "yonder", "zenith",
];

/// An endless text of random words in lines of about 72 characters.
struct RandomText {
    state: u64,
    column: usize,
}

impl RandomText {
    fn new(seed: u64) -> Self {
        RandomText { state: seed | 1, column: 0 }
    }

    /// The next `n` bytes (the last byte of a file ends the line).
    fn take(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 16);
        while out.len() < n {
            let word = WORDS[(next(&mut self.state) % WORDS.len() as u64) as usize];
            if self.column + word.len() + 1 > 72 {
                out.push(b'\n');
                self.column = 0;
            } else if self.column > 0 {
                out.push(b' ');
                self.column += 1;
            }
            out.extend_from_slice(word.as_bytes());
            self.column += word.len();
        }
        out.truncate(n);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_file_keeps_its_name_and_several_get_a_counter() {
        assert_eq!(file_name("data.txt", 1, 1), "data.txt");
        assert_eq!(file_name("data.txt", 7, 12), "data-07.txt");
        assert_eq!(file_name("data.txt", 3, 1000), "data-0003.txt");
        assert_eq!(file_name("notes", 2, 5), "notes-2");
        assert_eq!(file_name(".hidden", 2, 5), ".hidden-2");
        assert_eq!(file_name("a.tar.gz", 1, 3), "a.tar-1.gz");
    }

    #[test]
    fn the_text_has_exactly_the_size_and_is_text() {
        let mut text = RandomText::new(42);
        let a = text.take(5000);
        assert_eq!(a.len(), 5000);
        assert!(std::str::from_utf8(&a).is_ok());
        assert!(a.iter().all(|b| b.is_ascii_lowercase() || *b == b' ' || *b == b'\n'));
        let lines = a.split(|b| *b == b'\n');
        assert!(lines.clone().all(|l| l.len() <= 72));
        assert_ne!(a, RandomText::new(44).take(5000), "other seed, other text");
    }
}
