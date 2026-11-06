//! Removes spoken filler words from a finished English transcript.
//!
//! Two rules keep this safe on real text:
//!
//! - It only runs for English transcripts. `um` is a filler in English and the
//!   word "a" in Portuguese, and `er` means "he" in German.
//! - Any word that appears inside a dictionary entry is never removed, so a
//!   person or company whose name contains a filler keeps it.

/// Single-token fillers. Matching is case-insensitive and whole-word only.
///
/// `er`, `mm` and `mmm` are deliberately absent: they are ordinary words or
/// units ("er" is "he" in German, "5 mm" is a measurement).
const FILLERS: &[&str] = &[
    "ah", "ahh", "eh", "erm", "hm", "hmm", "hmmm", "uh", "uhh", "uhm", "um", "umm", "ummm",
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-'
}

/// Punctuation that only existed because of the filler that followed it.
fn is_sentence_punctuation(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|c| {
            matches!(
                c,
                '.' | '?' | '!' | ',' | ';' | ':' | '\u{2026}' | '\u{2014}'
            )
        })
}

/// Opening delimiters keep the space before them: `say "hello"` must not become
/// `say"hello"`.
fn is_opening_delimiter(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '(' | '[' | '{' | '\'' | '\u{2018}')
}

fn matching_opener(c: char) -> Option<char> {
    match c {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        '\u{201D}' => Some('\u{201C}'),
        // A straight quote can open or close; an empty pair is removed either way.
        '"' => Some('"'),
        '\'' => Some('\''),
        _ => None,
    }
}

fn is_space(token: &str) -> bool {
    !token.is_empty() && token.chars().all(char::is_whitespace)
}

/// Character class used to group runs: 0 word, 1 whitespace, 2 punctuation.
fn class_of(c: char) -> u8 {
    if is_word_char(c) {
        0
    } else if c.is_whitespace() {
        1
    } else {
        2
    }
}

/// Splits text into word, whitespace and punctuation runs, keeping every character.
fn tokenize(text: &str) -> Vec<(bool, String)> {
    let mut tokens: Vec<(bool, String)> = Vec::new();
    let mut last_class: Option<u8> = None;
    for c in text.chars() {
        let class = class_of(c);
        // Each punctuation mark is its own token, so a closing bracket is not
        // glued to the sentence mark before it.
        match tokens.last_mut() {
            Some((_, buffer)) if last_class == Some(class) && class != 2 => buffer.push(c),
            _ => tokens.push((class == 0, c.to_string())),
        }
        last_class = Some(class);
    }
    tokens
}

/// Filler removal is English-only: `um` is the article "a" in Portuguese and
/// `er` means "he" in German. An unknown language keeps the text untouched.
pub fn should_remove_fillers(language: Option<&str>, enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    language.is_some_and(|value| {
        value
            .split(['-', '_'])
            .next()
            .is_some_and(|base| base.eq_ignore_ascii_case("en"))
    })
}

/// Every lowercase word that appears inside a dictionary entry.
///
/// Protecting the whole phrase would need phrase matching; protecting its words
/// is what the interface promises ("Words in your dictionary are never removed")
/// and covers entries such as "Um Corp".
fn protected_words(protected: &[String]) -> std::collections::HashSet<String> {
    protected
        .iter()
        .flat_map(|entry| tokenize(entry))
        .filter(|(is_word, _)| *is_word)
        .map(|(_, word)| word.to_lowercase())
        .collect()
}

/// Removes filler words and the punctuation that only existed around them.
///
/// Text with no filler is returned unchanged, so line breaks and spacing survive.
pub fn remove_fillers(text: &str, protected: &[String]) -> String {
    let protected = protected_words(protected);
    let tokens = tokenize(text);
    let mut kept: Vec<(bool, String)> = Vec::with_capacity(tokens.len());
    let mut removed = false;
    let mut removed_at_start = false;
    let mut index = 0;

    while index < tokens.len() {
        let (is_word, ref token) = tokens[index];
        let is_filler = is_word
            && FILLERS.contains(&token.to_lowercase().as_str())
            && !protected.contains(&token.to_lowercase());
        if !is_filler {
            kept.push((is_word, token.clone()));
            index += 1;
            continue;
        }

        removed = true;
        if kept.iter().all(|(_, value)| is_space(value)) {
            removed_at_start = true;
        }
        index += 1;
        // The punctuation right after a filler belonged to it: "Um. Hello."
        // must not become ". Hello.", and "Uh... so" must not become "... so".
        while index < tokens.len() {
            let (next_word, ref next) = tokens[index];
            if next_word || !is_sentence_punctuation(next) {
                break;
            }
            index += 1;
        }
    }

    if !removed {
        return text.to_string();
    }
    let mut result = tidy(&kept);
    // A separator left dangling at the end belonged to a trailing filler.
    while result.ends_with(',') || result.ends_with(';') || result.ends_with(':') {
        result.pop();
        result = result.trim_end().to_string();
    }
    if removed_at_start {
        result = capitalize_first_word(&result);
    }
    result
}

/// Chinese, Japanese kana and Thai are written without spaces between words.
fn unspaced_script(c: char) -> bool {
    let code = c as u32;
    (0x3040..=0x30FF).contains(&code)
        || (0x3400..=0x9FFF).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFF00..=0xFFEF).contains(&code)
        || (0x0E00..=0x0E7F).contains(&code)
        || (0x3000..=0x303F).contains(&code)
}

/// Collapses the whitespace and doubled punctuation removal can leave behind.
fn tidy(tokens: &[(bool, String)]) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for (is_word, token) in tokens {
        if !*is_word && is_space(token) {
            pending_space = true;
            continue;
        }
        let is_punctuation = !*is_word;
        let had_space = pending_space;
        if had_space && !out.is_empty() && !is_punctuation {
            // Removing a filler between two Chinese words must not add a space.
            let joins_unspaced = out
                .chars()
                .last()
                .zip(token.chars().next())
                .is_some_and(|(left, right)| unspaced_script(left) && unspaced_script(right));
            if !joins_unspaced && !out.ends_with(' ') {
                out.push(' ');
            }
        }
        pending_space = false;
        if is_punctuation {
            // A comma right after a comma, or before any text, is a leftover
            // separator from the removed filler.
            if token.trim() == "," && (out.trim().is_empty() || out.trim_end().ends_with(',')) {
                continue;
            }
            let first = token.chars().next().unwrap_or(' ');
            // An empty pair left behind by the filler goes away entirely, so
            // "(Um.)" becomes nothing rather than "(.)".
            if let Some(opener) = matching_opener(first) {
                let trimmed = out.trim_end();
                if trimmed.ends_with(opener) {
                    let keep = trimmed.len() - opener.len_utf8();
                    out.truncate(keep);
                    continue;
                }
            }
            // Do not leave a space before punctuation, except before an opening
            // delimiter, where the space separates real words.
            if is_opening_delimiter(first) {
                if had_space && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
            } else {
                while out.ends_with(' ') {
                    out.pop();
                }
            }
        }
        out.push_str(token);
    }
    out.trim().to_string()
}

fn capitalize_first_word(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::remove_fillers;

    fn clean(text: &str) -> String {
        remove_fillers(text, &[])
    }

    #[test]
    fn removes_a_filler_between_commas() {
        assert_eq!(clean("Make it red, um, blue."), "Make it red, blue.");
        assert_eq!(clean("So, uh, we shipped it."), "So, we shipped it.");
    }

    #[test]
    fn removes_fillers_without_punctuation() {
        assert_eq!(clean("I um think uh so"), "I think so");
        assert_eq!(clean("Um the file is gone"), "The file is gone");
    }

    #[test]
    fn capitalizes_after_a_leading_filler() {
        assert_eq!(clean("Um, hello."), "Hello.");
        assert_eq!(clean("uh, what?"), "What?");
    }

    #[test]
    fn takes_the_punctuation_that_belonged_to_the_filler() {
        assert_eq!(clean("Um. Hello."), "Hello.");
        assert_eq!(clean("Hello. Um. How are you?"), "Hello. How are you?");
        assert_eq!(clean("I think so, um."), "I think so");
        assert_eq!(clean("Uh... so we go"), "So we go");
        assert_eq!(clean("Hmm? What?"), "What?");
    }

    #[test]
    fn keeps_quotes_and_brackets_readable() {
        assert_eq!(clean("Um, say \"hello\"."), "Say \"hello\".");
        assert_eq!(clean("(Um.) Hello."), "Hello.");
        assert_eq!(clean("He said (um) nothing."), "He said nothing.");
        assert_eq!(clean("[Um] Fine."), "Fine.");
    }

    #[test]
    fn keeps_hyphenated_words_whole() {
        assert_eq!(clean("Uh-huh, yes."), "Uh-huh, yes.");
        assert_eq!(clean("Um-hum and a nod"), "Um-hum and a nod");
    }

    #[test]
    fn keeps_words_that_contain_a_filler() {
        assert_eq!(
            clean("The umbrella is humming."),
            "The umbrella is humming."
        );
        assert_eq!(
            clean("A human, a hum, and a hummer."),
            "A human, a hum, and a hummer."
        );
    }

    #[test]
    fn leaves_measurements_and_meaningful_words_alone() {
        // "mm" is a unit and "er" is German for "he"; neither is on the list.
        for text in [
            "A 5 mm bolt.",
            "Er ist hier.",
            "I like it, you know.",
            "So we right-sized it.",
        ] {
            assert_eq!(clean(text), text);
        }
    }

    #[test]
    fn only_english_transcripts_are_cleaned() {
        assert!(super::should_remove_fillers(Some("en"), true));
        assert!(super::should_remove_fillers(Some("en-US"), true));
        assert!(!super::should_remove_fillers(Some("de"), true));
        assert!(!super::should_remove_fillers(Some("pt-BR"), true));
        assert!(!super::should_remove_fillers(None, true));
        assert!(!super::should_remove_fillers(Some("unknown"), true));
        assert!(!super::should_remove_fillers(Some("en"), false));
    }

    #[test]
    fn keeps_dictionary_terms_including_multi_word_entries() {
        let protected = vec!["Um".to_string()];
        assert_eq!(
            remove_fillers("Um asked, um, about it.", &protected),
            "Um asked, um, about it."
        );
        // Protection is word-level, so every "um" survives an "Um Corp" entry.
        assert_eq!(
            remove_fillers("Talk to Um Corp, um, today", &["Um Corp".to_string()]),
            "Talk to Um Corp, um, today"
        );
        assert_eq!(
            remove_fillers("Khent said um hello", &["Khent".to_string()]),
            "Khent said hello"
        );
    }

    #[test]
    fn returns_untouched_text_unchanged() {
        for text in ["Line one\nLine two", "我 喜欢 你", "", "   ", "Hello."] {
            assert_eq!(clean(text), text);
        }
    }

    #[test]
    fn handles_filler_only_text() {
        assert_eq!(clean("um"), "");
        assert_eq!(clean("um uh um"), "");
        assert_eq!(clean("Um."), "");
    }

    #[test]
    fn cleans_real_model_output() {
        // Raw transcript from Moonshine Tiny on a synthetic clip that said
        // "Um, I think, uh, the build is, um, ready."
        assert_eq!(
            clean("Um, I think the build is um, ready."),
            "I think the build is ready."
        );
    }

    #[test]
    fn keeps_chinese_text_untouched() {
        assert_eq!(clean("请用 um 打开它。"), "请用打开它。");
        assert_eq!(clean("请用 GitHub 打开它。"), "请用 GitHub 打开它。");
    }
}
