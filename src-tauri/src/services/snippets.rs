//! Expands a spoken shortcut into saved text.
//!
//! Two shapes are supported, both case-insensitive:
//!
//! - A dictation that is *only* the trigger becomes the expansion. This is the
//!   common case: say "my address" and the saved address is inserted.
//! - A trigger of two or more words is also replaced inside a longer sentence,
//!   so "send them the support link" works mid-dictation. Single-word triggers
//!   are never replaced inline: "sig" is a plausible word, and silently
//!   rewriting it would corrupt ordinary dictation.
//!
//! Matching is whole-word and prefers the longest trigger, so "my work email"
//! wins over "my email" when both exist.

use serde::{Deserialize, Serialize};

/// A spoken shortcut and the text it inserts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub expansion: String,
}

/// Enough for a personal library without turning every dictation into a scan.
pub const MAX_SNIPPETS: usize = 200;
pub const MAX_TRIGGER_CHARS: usize = 60;
pub const MAX_EXPANSION_CHARS: usize = 4000;

/// Trims, drops empty entries and removes case-insensitive duplicate triggers.
///
/// The first entry wins for a duplicate trigger, and the stored order is kept so
/// the settings list does not jump while the user edits it.
pub fn normalize(snippets: impl IntoIterator<Item = Snippet>) -> Vec<Snippet> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for snippet in snippets {
        let trigger = snippet.trigger.trim();
        let expansion = snippet.expansion.trim();
        if trigger.is_empty() || expansion.is_empty() {
            continue;
        }
        let trigger: String = trigger.chars().take(MAX_TRIGGER_CHARS).collect();
        let expansion: String = expansion.chars().take(MAX_EXPANSION_CHARS).collect();
        let key = trigger.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(Snippet { trigger, expansion });
        if out.len() == MAX_SNIPPETS {
            break;
        }
    }
    out
}

/// Whether a character can be part of a word for boundary purposes.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-' || c == '_'
}

/// Lowercased words of a phrase, with punctuation and spacing discarded.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            current.extend(c.to_lowercase());
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Byte offsets of every word in `text`, in order.
fn word_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (index, c) in text.char_indices() {
        if is_word_char(c) {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            spans.push((begin, index));
        }
    }
    if let Some(begin) = start {
        spans.push((begin, text.len()));
    }
    spans
}

/// Replaces whole-word occurrences of multi-word triggers in one left-to-right
/// pass.
///
/// A single pass is what keeps an expansion from being expanded again: the text
/// that was just inserted is never rescanned.
fn replace_inline(text: &str, snippets: &[Snippet]) -> String {
    let mut ordered: Vec<(Vec<String>, &str)> = snippets
        .iter()
        .map(|snippet| (words(&snippet.trigger), snippet.expansion.as_str()))
        .filter(|(trigger, _)| trigger.len() >= 2)
        .collect();
    // Longest trigger first, so "my work email" is not shadowed by "my email".
    ordered.sort_by_key(|(trigger, _)| std::cmp::Reverse(trigger.len()));
    if ordered.is_empty() {
        return text.to_string();
    }

    let spans = word_spans(text);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut index = 0;
    while index < spans.len() {
        let mut hit: Option<(usize, &str)> = None;
        for (trigger, expansion) in &ordered {
            if index + trigger.len() > spans.len() {
                continue;
            }
            let matches = trigger.iter().enumerate().all(|(offset, want)| {
                let (from, to) = spans[index + offset];
                text[from..to].to_lowercase() == *want
            });
            if matches {
                hit = Some((trigger.len(), expansion));
                break;
            }
        }
        match hit {
            Some((length, expansion)) => {
                out.push_str(&text[cursor..spans[index].0]);
                out.push_str(expansion);
                cursor = spans[index + length - 1].1;
                index += length;
            }
            None => index += 1,
        }
    }
    out.push_str(&text[cursor..]);
    out
}

/// Expands a finished transcript.
///
/// Returns the input unchanged when no snippet applies, so the caller can
/// compare against the original to decide whether anything happened.
pub fn expand(text: &str, snippets: &[Snippet]) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() || snippets.is_empty() {
        return text.to_string();
    }

    let spoken = words(trimmed);
    // Longest trigger first, so "my work email" is not shadowed by "my email".
    let mut ordered: Vec<&Snippet> = snippets.iter().collect();
    ordered.sort_by_key(|snippet| std::cmp::Reverse(words(&snippet.trigger).len()));

    for snippet in &ordered {
        let trigger_words = words(&snippet.trigger);
        if !trigger_words.is_empty() && trigger_words == spoken {
            return snippet.expansion.clone();
        }
    }

    replace_inline(text, snippets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snippet(trigger: &str, expansion: &str) -> Snippet {
        Snippet {
            trigger: trigger.into(),
            expansion: expansion.into(),
        }
    }

    #[test]
    fn a_whole_dictation_that_is_the_trigger_expands() {
        let snippets = vec![snippet("my email", "khent@example.com")];
        assert_eq!(expand("My email.", &snippets), "khent@example.com");
        assert_eq!(expand("  my   EMAIL  ", &snippets), "khent@example.com");
    }

    #[test]
    fn a_single_word_trigger_is_never_replaced_inline() {
        let snippets = vec![snippet("sig", "Khent Felary G Sanco")];
        assert_eq!(
            expand("The sig on the contract looks fine", &snippets),
            "The sig on the contract looks fine"
        );
    }

    #[test]
    fn a_multi_word_trigger_is_replaced_inside_a_sentence() {
        let snippets = vec![snippet("support link", "https://example.com/help")];
        assert_eq!(
            expand("Please send them the support link today", &snippets),
            "Please send them the https://example.com/help today"
        );
    }

    #[test]
    fn the_longest_trigger_wins() {
        let snippets = vec![
            snippet("my email", "short@example.com"),
            snippet("my work email", "long@example.com"),
        ];
        assert_eq!(expand("my work email", &snippets), "long@example.com");
        assert_eq!(expand("my email", &snippets), "short@example.com");
    }

    #[test]
    fn matching_respects_word_boundaries() {
        let snippets = vec![snippet("cat photo", "KITTEN")];
        assert_eq!(
            expand("the cat photos are here", &snippets),
            "the cat photos are here"
        );
        assert_eq!(expand("a cat photo", &snippets), "a KITTEN");
    }

    #[test]
    fn expansion_keeps_surrounding_punctuation() {
        let snippets = vec![snippet("the docs", "https://example.com/docs")];
        assert_eq!(
            expand("Read the docs, then reply.", &snippets),
            "Read https://example.com/docs, then reply."
        );
    }

    #[test]
    fn normalize_trims_drops_empty_and_dedupes_case_insensitively() {
        let normalized = normalize(vec![
            snippet("  My Email ", " first@example.com "),
            snippet("my email", "second@example.com"),
            snippet("   ", "orphan"),
            snippet("blank", "   "),
        ]);
        assert_eq!(normalized, vec![snippet("My Email", "first@example.com")]);
    }

    #[test]
    fn normalize_caps_the_library_size() {
        let many = (0..MAX_SNIPPETS + 25)
            .map(|index| snippet(&format!("trigger {index}"), &format!("expansion {index}")));
        assert_eq!(normalize(many).len(), MAX_SNIPPETS);
    }

    #[test]
    fn an_expansion_is_not_expanded_again() {
        let snippets = vec![
            snippet("my email", "write to my address"),
            snippet("my address", "1 Example Street"),
        ];
        assert_eq!(
            expand("send my email today", &snippets),
            "send write to my address today"
        );
    }

    #[test]
    fn two_triggers_in_one_sentence_both_expand() {
        let snippets = vec![
            snippet("my email", "a@example.com"),
            snippet("my address", "1 Example Street"),
        ];
        assert_eq!(
            expand("send my email and my address", &snippets),
            "send a@example.com and 1 Example Street"
        );
    }

    #[test]
    fn text_without_a_match_is_returned_unchanged() {
        let snippets = vec![snippet("my email", "khent@example.com")];
        assert_eq!(
            expand("Nothing to see here.", &snippets),
            "Nothing to see here."
        );
        assert_eq!(expand("", &snippets), "");
        assert_eq!(expand("anything", &[]), "anything");
    }
}
