//! Filters that forgive spelling mistakes (the command list and the
//! settings screen).
//!
//! Every word typed must be found somewhere in the text, exactly or with a
//! few mistakes: a wrong, missing or extra letter, or two letters swapped.
//! [`score`] says how well the text fits (0 is a perfect fit), so the
//! best matches can be listed first.

/// How many mistakes a word of `length` letters may have.
fn allowed_errors(length: usize) -> usize {
    match length {
        0..=2 => 0,
        3..=6 => 1,
        _ => 2,
    }
}

/// The fewest edits (insertions, deletions, substitutions, swaps of two
/// neighbours) that turn `word` into some part of `text`, and whether that
/// part starts a word of the text. Stops looking beyond `limit` edits.
fn best_fit(word: &[char], text: &[char], limit: usize) -> Option<(usize, bool)> {
    // Sellers' algorithm: a row of costs per position of the text, with a
    // free start anywhere in the text.
    let n = word.len();
    let mut previous2: Vec<usize> = vec![0; n + 1];
    let mut previous: Vec<usize> = (0..=n).collect();
    let mut best: Option<usize> = (n <= limit).then_some(n);
    let mut best_ends: Vec<usize> = Vec::new();
    if best.is_some() {
        best_ends.push(0);
    }
    for (j, &c) in text.iter().enumerate() {
        let mut row = vec![0usize; n + 1];
        for i in 1..=n {
            let same = usize::from(word[i - 1] != c);
            let mut cost = (previous[i - 1] + same).min(previous[i] + 1).min(row[i - 1] + 1);
            if i >= 2 && j >= 1 && word[i - 1] == text[j - 1] && word[i - 2] == c {
                cost = cost.min(previous2[i - 2] + 1);
            }
            row[i] = cost;
        }
        if row[n] <= limit {
            match best {
                Some(b) if row[n] > b => {}
                Some(b) if row[n] == b => best_ends.push(j + 1),
                _ => {
                    best = Some(row[n]);
                    best_ends = vec![j + 1];
                }
            }
        }
        previous2 = previous;
        previous = row;
    }
    let errors = best?;
    // Whether a best fit begins a word (its start is about `end - n`).
    let at_word_start = best_ends.iter().any(|&end| {
        let start = end.saturating_sub(n);
        start == 0 || !text[start - 1].is_alphanumeric()
    });
    Some((errors, at_word_start))
}

/// How well `text` fits what was typed in `filter`: `None` when some word
/// is not found (even allowing mistakes), else a score where 0 is best.
/// An empty filter fits everything with 0.
pub fn score(filter: &str, text: &str) -> Option<usize> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut total = 0;
    for word in filter.to_lowercase().split_whitespace() {
        let word: Vec<char> = word.chars().collect();
        let exact = text.windows(word.len()).position(|w| w == word.as_slice());
        total += match exact {
            Some(start) if start == 0 || !text[start - 1].is_alphanumeric() => 0,
            Some(_) => 1,
            None => {
                let (errors, at_start) = best_fit(&word, &text, allowed_errors(word.len()))?;
                // Exact fits come first, then the fewer mistakes the better.
                2 + errors * 2 + usize::from(!at_start)
            }
        };
    }
    Some(total)
}

/// Whether `score` (of [`score`]) means every word of `filter` was found
/// as typed. When anything fits like that, lists keep only those rows;
/// mistakes are forgiven only when nothing else fits.
pub fn is_exact(filter: &str, score: usize) -> bool {
    score <= filter.split_whitespace().count()
}

/// Whether every word of `filter` is found in `text`, mistakes allowed.
pub fn matches(filter: &str, text: &str) -> bool {
    score(filter, text).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_words_fit_best() {
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("copy", "Files Copy files.copy"), Some(0));
        assert_eq!(score("opy", "files copy"), Some(1), "inside a word");
        assert_eq!(score("move", "Files Copy"), None);
    }

    #[test]
    fn spelling_mistakes_are_forgiven() {
        for (typed, text) in [
            ("settngs", "Settings"),
            ("cpoy", "Files Copy"),
            ("renmae", "Files Rename"),
            ("copi", "copy"),
            ("coppy", "copy"),
            ("setings", "settings"),
            ("bokmarks", "bookmarks"),
        ] {
            assert!(matches(typed, text), "{typed} should find {text}");
            assert!(score(typed, text).unwrap() > 0);
        }
    }

    #[test]
    fn short_words_and_far_words_do_not_fit() {
        assert!(!matches("xy", "copy"), "two letters must be exact");
        assert!(!matches("cqz", "copy"));
        assert!(!matches("sttngz", "settings"));
    }

    #[test]
    fn every_word_must_fit_and_fewer_mistakes_rank_first() {
        assert!(matches("fils renmae", "files rename"));
        assert!(!matches("files zzzzzz", "files rename"));
        assert!(score("renam", "rename").unwrap() < score("renmae", "rename").unwrap());
    }
}
