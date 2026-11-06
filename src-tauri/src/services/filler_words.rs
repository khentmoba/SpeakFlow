//! Removes spoken filler words from a finished transcript.
//!
//! The list is deliberately short and English-only. Words that carry meaning in
//! some contexts ("like", "so", "right", "you know") are not touched. A term the
//! user added to their dictionary is never removed, so a person whose name is
//! "Um" keeps it.

/// Single-token fillers. Matching is case-insensitive and whole-word only.
const FILLERS: &[&str] = &[
    "ah", "ahh", "eh", "er", "erm", "hm", "hmm", "hmmm", "mm", "mmm", "uh", "uhh", "uhm", "um",
    "umm", "ummm",
];

fn is_filler(word: &str, protected: &[String]) -> bool {
    let lower = word.to_ascii_lowercase();
    if !FILLERS.contains(&lower.as_str()) {
        return false;
    }
    !protected
        .iter()
        .any(|term| term.trim().eq_ignore_ascii_case(&lower))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}'
}

/// Splits text into word and separator runs, keeping every character.
fn tokenize(text: &str) -> Vec<(bool, String)> {
    let mut tokens: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let word = is_word_char(c);
        match tokens.last_mut() {
            Some((last_word, buffer)) if *last_word == word => buffer.push(c),
            _ => tokens.push((word, c.to_string())),
        }
    }
    tokens
}

/// True when `token` is only whitespace.
fn is_space(token: &str) -> bool {
    !token.is_empty() && token.chars().all(char::is_whitespace)
}

fn is_comma(token: &str) -> bool {
    token.trim() == ","
}

/// Removes filler words and the punctuation that only existed around them.
///
/// "Make it red, um, blue." becomes "Make it red, blue." and "Um, hello." becomes
/// "Hello." Punctuation that still separates real words is left alone.
pub fn remove_fillers(text: &str, protected: &[String]) -> String {
    let tokens = tokenize(text);
    let mut kept: Vec<(bool, String)> = Vec::with_capacity(tokens.len());
    let mut removed_at_start = false;
    let mut index = 0;

    while index < tokens.len() {
        let (is_word, ref token) = tokens[index];
        if is_word && is_filler(token, protected) {
            if kept.iter().all(|(_, value)| is_space(value)) {
                removed_at_start = true;
            }
            // The comma after a filler belonged to it: "is um, ready" keeps
            // "is ready". Between two commas ("red, um, blue") only the second
            // one goes, so a single separator survives.
            if tokens
                .get(index + 1)
                .is_some_and(|(word, value)| !*word && is_comma(value))
            {
                index += 1;
            }
            index += 1;
            continue;
        }
        kept.push((is_word, token.clone()));
        index += 1;
    }

    let mut result = tidy(&kept);
    // A separator left dangling at the end belonged to a trailing filler.
    while result.ends_with(',') {
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
        if pending_space && !out.is_empty() && !is_punctuation {
            // Removing a filler between two Chinese words must not add a space.
            let joins_unspaced = out
                .chars()
                .last()
                .zip(token.chars().next())
                .is_some_and(|(left, right)| unspaced_script(left) && unspaced_script(right));
            if !joins_unspaced {
                out.push(' ');
            }
        }
        pending_space = false;
        if is_punctuation {
            // A comma directly after a comma, or before any text, is a leftover
            // separator from the removed filler.
            if is_comma(token) && (out.trim().is_empty() || out.trim_end().ends_with(',')) {
                continue;
            }
            // Do not leave a space before punctuation.
            while out.ends_with(' ') {
                out.pop();
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
    fn keeps_dictionary_terms() {
        // A dictionary entry protects every case variant of that word.
        let protected = vec!["Um".to_string()];
        assert_eq!(
            remove_fillers("Um asked, um, about it.", &protected),
            "Um asked, um, about it."
        );
        // Terms that are not fillers are untouched either way.
        assert_eq!(
            remove_fillers("Khent said um hello", &["Khent".to_string()]),
            "Khent said hello"
        );
    }

    #[test]
    fn leaves_meaningful_words_alone() {
        for text in [
            "I like it, you know.",
            "So we right-sized it.",
            "Right, that works.",
        ] {
            assert_eq!(clean(text), text);
        }
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
    fn handles_empty_and_filler_only_text() {
        assert_eq!(clean(""), "");
        assert_eq!(clean("um"), "");
        assert_eq!(clean("um uh um"), "");
    }

    #[test]
    fn keeps_chinese_text_untouched() {
        assert_eq!(clean("请用 um 打开它。"), "请用打开它。");
        assert_eq!(clean("请用 GitHub 打开它。"), "请用 GitHub 打开它。");
    }
}
