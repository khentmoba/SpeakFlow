//! Tidies spoken text without calling a model.
//!
//! This is the local half of "clean up what I said". It only performs edits that
//! cannot change meaning:
//!
//! - A repeated phrase of two or more words is collapsed. "we need to we need to
//!   ship" is always an artifact of speech, never intentional.
//! - A repeated single word is collapsed only for words that are almost never
//!   doubled on purpose. "had had", "that that" and "very very" are left alone.
//! - A parenthetical "you know" — surrounded by commas — is dropped in English.
//!
//! Rewriting a self-correction ("five, no, six") needs the model, because the
//! local rules cannot tell a correction from a list. That stays in AI polish.

/// Single words that are collapsed when spoken twice in a row.
///
/// Deliberately excluded: "had" ("had had"), "that" ("the fact that that
/// happened"), "very"/"really" (emphasis), "no"/"yes" (answers) and "ha".
const COLLAPSIBLE_WORDS: &[&str] = &[
    "the", "a", "an", "and", "to", "of", "in", "on", "at", "it", "is", "was", "were", "i", "we",
    "you", "they", "he", "she", "but", "so", "for", "with", "then", "there", "be", "been", "have",
    "has", "will", "would", "can", "could", "should", "do", "does", "did", "my", "our", "your",
];

const MAX_PHRASE_WORDS: usize = 6;
const MAX_PASSES: usize = 8;

struct Word {
    /// Lowercased alphanumeric core, used for comparison.
    core: String,
    start: usize,
    end: usize,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-'
}

fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (index, c) in text.char_indices() {
        if is_word_char(c) {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            out.push(Word {
                core: text[begin..index].to_lowercase(),
                start: begin,
                end: index,
            });
        }
    }
    if let Some(begin) = start {
        out.push(Word {
            core: text[begin..].to_lowercase(),
            start: begin,
            end: text.len(),
        });
    }
    out
}

/// Removes `[from, to)` along with the separator and whitespace that introduced
/// it.
///
/// A line break before the removed run is kept, so paragraph structure survives.
fn remove_range(text: &str, from: usize, to: usize) -> String {
    let head = &text[..from];
    let trimmed = head.trim_end_matches([' ', '\t']);
    let mut cut = if trimmed.ends_with('\n') {
        from
    } else {
        trimmed.len()
    };
    // "the plan, the plan is ready" must not leave a stray comma behind.
    if !trimmed.ends_with('\n') {
        let without_separator = trimmed.trim_end_matches([',', ';', ':']);
        if without_separator.len() != trimmed.len() {
            cut = without_separator.trim_end_matches([' ', '\t']).len();
        }
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..cut]);
    out.push_str(&text[to..]);
    out
}

/// Collapses the second half of every adjacent repeated run of words.
fn collapse_repeats_once(text: &str) -> Option<String> {
    let words = words(text);
    for size in (2..=MAX_PHRASE_WORDS).rev() {
        if words.len() < size * 2 {
            continue;
        }
        for start in 0..=words.len() - size * 2 {
            let first = &words[start..start + size];
            let second = &words[start + size..start + size * 2];
            if first.iter().zip(second).all(|(a, b)| a.core == b.core) {
                return Some(remove_range(text, second[0].start, second[size - 1].end));
            }
        }
    }
    for index in 0..words.len().saturating_sub(1) {
        let (first, second) = (&words[index], &words[index + 1]);
        if first.core == second.core && COLLAPSIBLE_WORDS.contains(&first.core.as_str()) {
            return Some(remove_range(text, second.start, second.end));
        }
    }
    None
}

/// Drops a comma-wrapped "you know".
fn remove_you_know(text: &str) -> Option<String> {
    let words = words(text);
    if words.len() < 2 {
        return None;
    }
    for index in 0..words.len() - 1 {
        let (first, second) = (&words[index], &words[index + 1]);
        if first.core != "you" || second.core != "know" {
            continue;
        }
        let before = text[..first.start].trim_end();
        let after = text[second.end..].trim_start();
        if !before.ends_with(',') || !after.starts_with(',') {
            continue;
        }
        // Keep the first comma, drop the phrase and the second one.
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..first.start]);
        let rest = text[second.end..].trim_start();
        let rest = rest.strip_prefix(',').map(str::trim_start).unwrap_or(rest);
        out.push_str(rest);
        return Some(out);
    }
    None
}

fn is_english(language: Option<&str>) -> bool {
    language.is_some_and(|value| {
        value
            .split(['-', '_'])
            .next()
            .is_some_and(|base| base.eq_ignore_ascii_case("en"))
    })
}

/// Collapses repeated words and phrases.
///
/// Safe for every language: a repeated two-word phrase is an artifact in any
/// script, and the single-word list is English-only in practice.
pub fn collapse_repetitions(text: &str) -> String {
    let mut current = text.to_string();
    for _ in 0..MAX_PASSES {
        match collapse_repeats_once(&current) {
            Some(next) => current = next,
            None => break,
        }
    }
    current
}

/// Applies every local cleanup rule that fits the transcript's language.
///
/// Text with nothing to tidy is returned unchanged.
pub fn tidy(text: &str, language: Option<&str>) -> String {
    let mut current = collapse_repetitions(text);
    if is_english(language) {
        for _ in 0..MAX_PASSES {
            match remove_you_know(&current) {
                Some(next) => current = next,
                None => break,
            }
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeated_phrase_is_collapsed() {
        assert_eq!(
            collapse_repetitions("we need to we need to ship this"),
            "we need to ship this"
        );
        assert_eq!(
            collapse_repetitions("I think I think it works"),
            "I think it works"
        );
    }

    #[test]
    fn a_repeated_single_word_is_collapsed_for_safe_words() {
        assert_eq!(collapse_repetitions("the the report"), "the report");
        assert_eq!(collapse_repetitions("I I think so"), "I think so");
    }

    #[test]
    fn meaningful_doubles_survive() {
        assert_eq!(collapse_repetitions("I had had enough"), "I had had enough");
        assert_eq!(
            collapse_repetitions("the fact that that happened"),
            "the fact that that happened"
        );
        assert_eq!(
            collapse_repetitions("it was very very good"),
            "it was very very good"
        );
        assert_eq!(
            collapse_repetitions("no no, that is wrong"),
            "no no, that is wrong"
        );
    }

    #[test]
    fn repetition_across_a_comma_is_collapsed() {
        assert_eq!(
            collapse_repetitions("the plan, the plan is ready"),
            "the plan is ready"
        );
    }

    #[test]
    fn line_breaks_before_a_repeat_survive() {
        assert_eq!(
            collapse_repetitions("first line\nsecond line second line"),
            "first line\nsecond line"
        );
    }

    #[test]
    fn you_know_is_dropped_only_between_commas() {
        assert_eq!(
            tidy("so, you know, we should go", Some("en-US")),
            "so, we should go"
        );
        assert_eq!(
            tidy("you know the answer already", Some("en")),
            "you know the answer already"
        );
    }

    #[test]
    fn you_know_is_kept_for_other_languages() {
        assert_eq!(
            tidy("so, you know, we should go", Some("de")),
            "so, you know, we should go"
        );
    }

    #[test]
    fn clean_text_is_returned_unchanged() {
        let text = "The quick brown fox jumps over the lazy dog.";
        assert_eq!(tidy(text, Some("en")), text);
        assert_eq!(tidy("", Some("en")), "");
    }

    #[test]
    fn collapse_is_idempotent() {
        let once = collapse_repetitions("we we should should go go now");
        assert_eq!(collapse_repetitions(&once), once);
    }
}
