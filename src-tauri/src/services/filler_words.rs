//! Removes spoken filler words from a finished English transcript.
//!
//! Two rules keep this safe on real text:
//!
//! - It only runs for English transcripts. `um` is a filler in English and the
//!   word "a" in Portuguese, and `er` means "he" in German.
//! - Any word that appears inside a dictionary entry is never removed, so a
//!   person or company whose name contains a filler keeps it.
//!
//! Removal is surgical: every token outside the seam where a filler stood is
//! copied through untouched, so quotes, brackets, newlines and spacing elsewhere
//! in the transcript survive.

/// Single-token fillers. Matching is case-insensitive and whole-word only.
///
/// `er`, `mm` and `mmm` are deliberately absent: they are ordinary words or
/// units ("er" is "he" in German, "5 mm" is a measurement).
const FILLERS: &[&str] = &[
    "ah", "ahh", "eh", "erm", "hm", "hmm", "hmmm", "uh", "uhh", "uhm", "um", "umm", "ummm",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Punct,
}

struct Token {
    kind: Kind,
    text: String,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-'
}

fn class_of(c: char) -> Kind {
    if is_word_char(c) {
        Kind::Word
    } else if c.is_whitespace() {
        Kind::Space
    } else {
        Kind::Punct
    }
}

/// Splits text into word, whitespace and punctuation runs, keeping every
/// character. Each punctuation mark is its own token so a closing bracket is not
/// glued to the sentence mark before it.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut last: Option<Kind> = None;
    for c in text.chars() {
        let kind = class_of(c);
        match tokens.last_mut() {
            Some(token) if last == Some(kind) && kind != Kind::Punct => token.text.push(c),
            _ => tokens.push(Token {
                kind,
                text: c.to_string(),
            }),
        }
        last = Some(kind);
    }
    tokens
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
        .filter(|token| token.kind == Kind::Word)
        .map(|token| token.text.to_lowercase())
        .collect()
}

fn is_filler(word: &str, protected: &std::collections::HashSet<String>) -> bool {
    let lower = word.to_lowercase();
    FILLERS.contains(&lower.as_str()) && !protected.contains(&lower)
}

fn first_char(token: &Token) -> Option<char> {
    token.text.chars().next()
}

fn is_separator(c: char) -> bool {
    matches!(c, ',' | ';' | ':')
}

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '\u{2026}')
}

fn is_dash(c: char) -> bool {
    matches!(c, '\u{2014}' | '\u{2013}' | '-')
}

fn is_opening_delimiter(c: char) -> bool {
    matches!(c, '"' | '\u{201C}' | '(' | '[' | '{' | '\u{2018}')
}

fn closing_for(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '\u{201C}' => Some('\u{201D}'),
        '\u{2018}' => Some('\u{2019}'),
        '"' => Some('"'),
        _ => None,
    }
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

/// Removes filler words and the punctuation that only existed around them.
///
/// Text with no filler is returned unchanged.
pub fn remove_fillers(text: &str, protected: &[String]) -> String {
    let protected = protected_words(protected);
    let tokens = tokenize(text);
    let mut out = String::with_capacity(text.len());
    let mut removed = false;
    // Set after a removal until the next visible token is written.
    let mut seam = false;
    let mut capitalize_next = false;
    // Whether a straight double quote is currently open. Without this, a closing
    // quote would be mistaken for an opening one.
    let mut straight_quote_open = false;
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        if token.kind != Kind::Word || !is_filler(&token.text, &protected) {
            if token.kind == Kind::Space && seam {
                // Keep a line break and the indentation after it; collapse only
                // horizontal space at the seam.
                if token.text.contains('\n') {
                    let cut = out.trim_end_matches([' ', '\t']).len();
                    out.truncate(cut);
                    // A break already ending the output is not repeated.
                    let mut run = token.text.as_str();
                    while out.ends_with('\n') && run.starts_with('\n') {
                        run = &run[1..];
                    }
                    out.push_str(run);
                    seam = false;
                    capitalize_next = false;
                }
                index += 1;
                continue;
            }
            if seam {
                let text = write_seam(&mut out, token, &mut capitalize_next, straight_quote_open);
                seam = false;
                out.push_str(&text);
                track_quote(&text, &mut straight_quote_open);
                index += 1;
                continue;
            }
            if capitalize_next && token.kind == Kind::Word {
                let text = capitalize_word(&token.text);
                out.push_str(&text);
                capitalize_next = false;
            } else {
                out.push_str(&token.text);
            }
            track_quote(&token.text, &mut straight_quote_open);
            index += 1;
            continue;
        }

        removed = true;
        // The filler occupied the horizontal whitespace before it; line breaks
        // and indentation before it stay.
        let cut = out.trim_end_matches([' ', '\t']).len();
        if !out[..cut].ends_with('\n') {
            out.truncate(cut);
        }
        let before = out.clone();
        let mut opener = before
            .chars()
            .last()
            .filter(|c| is_opening_delimiter(*c) && (*c != '"' || straight_quote_open));
        // An opener at the start of the text still begins a sentence: `"Um, hi"`.
        let mut head = before.as_str();
        while let Some(last) = head
            .chars()
            .last()
            .filter(|c| is_opening_delimiter(*c) && (*c != '"' || straight_quote_open))
        {
            head = &head[..head.len() - last.len_utf8()];
        }
        let at_sentence_start = head.trim_end().is_empty()
            || head
                .trim_end()
                .chars()
                .last()
                .is_some_and(|c| is_terminator(c) || c == '\n');

        // Punctuation that belonged to the filler.
        let mut pair_closed = false;
        let mut next = index + 1;
        while next < tokens.len() {
            let candidate = &tokens[next];
            if candidate.kind != Kind::Punct {
                break;
            }
            let Some(c) = first_char(candidate) else {
                break;
            };
            let closes_empty_pair = opener.and_then(closing_for) == Some(c);
            // After a pair closed, a separator only collapses a doubled one:
            // "He said (um), and left" keeps its comma.
            let separator_belongs =
                is_separator(c) && (!pair_closed || out.ends_with([',', ';', ':']));
            let belongs_to_filler = separator_belongs
                || (opener.is_some() && is_terminator(c))
                || (at_sentence_start && is_terminator(c))
                || closes_empty_pair;
            if !belongs_to_filler {
                break;
            }
            if closes_empty_pair {
                // "(um)" leaves nothing behind, not "()".
                if let Some(opener) = opener {
                    let trimmed = out.trim_end().len();
                    if let Some(keep) = trimmed.checked_sub(opener.len_utf8()) {
                        if out.is_char_boundary(keep) {
                            out.truncate(keep);
                        }
                    }
                }
                if c == '"' {
                    straight_quote_open = false;
                }
                pair_closed = true;
                // The space before the opener went with the pair.
                let cut = out.trim_end_matches([' ', '\t']).len();
                out.truncate(cut);
                // A nested pair is now the innermost one: "((um))" leaves nothing.
                opener = out
                    .chars()
                    .last()
                    .filter(|c| is_opening_delimiter(*c) && (*c != '"' || straight_quote_open));
            }
            next += 1;
        }

        // "Hello, um." keeps "Hello." rather than "Hello,.".
        let next_visible = tokens[next..].iter().find(|t| t.kind != Kind::Space);
        let ends_sentence = next_visible
            .is_none_or(|t| t.kind == Kind::Punct && first_char(t).is_some_and(is_terminator));
        if ends_sentence {
            // The dash first: "Yes, — um." must not keep the comma it exposed.
            trim_trailing_dash(&mut out);
            trim_trailing_separator(&mut out);
        }
        // "I think — um — yes" keeps one dash; "Um — hello" drops the leading one.
        if let Some(position) = tokens[next..]
            .iter()
            .position(|t| t.kind != Kind::Space)
            .map(|offset| next + offset)
        {
            if is_dash_token(&tokens[position]) {
                trim_trailing_separator(&mut out);
                let after_dash = tokens[position + 1..]
                    .iter()
                    .find(|t| t.kind != Kind::Space)
                    .is_none_or(|t| {
                        t.kind == Kind::Punct && first_char(t).is_some_and(is_terminator)
                    });
                if out.trim_end().chars().last().is_some_and(is_dash) {
                    // Both dashes framed the filler: keep one, unless the aside
                    // ends here, in which case neither is needed.
                    if after_dash {
                        trim_trailing_dash(&mut out);
                    }
                    next = position + 1;
                } else if at_sentence_start {
                    next = position + 1;
                }
            }
        }

        seam = true;
        // Mid-text, only a capitalized filler marks a sentence start; otherwise
        // "3 p.m. um we left" would gain a capital after an abbreviation.
        let at_text_start = head.trim_end().is_empty();
        // A later filler in the same run must not clear an earlier capital.
        capitalize_next = capitalize_next
            || at_text_start
            || (at_sentence_start && token.text.chars().next().is_some_and(char::is_uppercase));
        index = next;
    }

    if !removed {
        return text.to_string();
    }
    out
}

/// Tracks straight double quotes so a closing quote is not read as an opener.
fn track_quote(text: &str, open: &mut bool) {
    for c in text.chars() {
        if c == '"' {
            *open = !*open;
        }
    }
}

/// Writes the first visible token after a removal, rebuilding the spacing that
/// the filler and its punctuation occupied.
fn write_seam(
    out: &mut String,
    token: &Token,
    capitalize_next: &mut bool,
    straight_quote_open: bool,
) -> String {
    let Some(first) = first_char(token) else {
        return String::new();
    };
    let after_opener = out
        .chars()
        .last()
        .is_some_and(|c| is_opening_delimiter(c) && (c != '"' || straight_quote_open));
    let before_closer = token.kind == Kind::Punct
        && (is_terminator(first)
            || is_separator(first)
            || matches!(first, ')' | ']' | '}' | '\u{201D}' | '\u{2019}')
            || (first == '"' && straight_quote_open));
    // Chinese and Japanese are written without spaces between words.
    let joins_unspaced = out
        .chars()
        .last()
        .is_some_and(|left| unspaced_script(left) && unspaced_script(first));
    if !out.is_empty()
        && !out.ends_with(char::is_whitespace)
        && !after_opener
        && !before_closer
        && !joins_unspaced
    {
        out.push(' ');
    }
    // An opener keeps capitalization pending for the word inside it.
    if token.kind == Kind::Punct && is_opening_delimiter(first) {
        return token.text.clone();
    }
    let capitalize = *capitalize_next && token.kind == Kind::Word;
    *capitalize_next = false;
    if capitalize {
        return capitalize_word(&token.text);
    }
    token.text.clone()
}

fn capitalize_word(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(head) => head.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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

fn is_dash_token(token: &Token) -> bool {
    !token.text.is_empty() && token.text.chars().all(is_dash)
}

fn trim_trailing_dash(out: &mut String) {
    loop {
        let trimmed_len = out.trim_end().len();
        let Some(dash) = out[..trimmed_len].chars().last().filter(|c| is_dash(*c)) else {
            return;
        };
        out.truncate(trimmed_len - dash.len_utf8());
        let cut = out.trim_end().len();
        out.truncate(cut);
    }
}

/// Trims a separator only when it sits directly at the end, so a colon before a
/// line break stays where the user put it.
fn trim_trailing_separator(out: &mut String) {
    if out.ends_with([',', ';', ':']) {
        out.pop();
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
        assert_eq!(clean("Hello. Um, how are you?"), "Hello. How are you?");
    }

    #[test]
    fn keeps_the_period_that_ends_the_sentence() {
        assert_eq!(clean("It works um. Ship it."), "It works. Ship it.");
        assert_eq!(clean("Are you sure um? Yes."), "Are you sure? Yes.");
        assert_eq!(clean("Hello, um."), "Hello.");
        assert_eq!(clean("I think so, um."), "I think so.");
    }

    #[test]
    fn takes_punctuation_that_belonged_to_the_filler() {
        assert_eq!(clean("Um. Hello."), "Hello.");
        assert_eq!(clean("Hello. Um. How are you?"), "Hello. How are you?");
        assert_eq!(clean("Uh... so we go"), "So we go");
        assert_eq!(clean("Hmm? What?"), "What?");
    }

    #[test]
    fn keeps_quotes_and_brackets_readable() {
        assert_eq!(clean("Um, say \"hello\"."), "Say \"hello\".");
        assert_eq!(clean("(Um.) Hello."), "Hello.");
        assert_eq!(clean("He said (um) nothing."), "He said nothing.");
        assert_eq!(clean("[Um] Fine."), "Fine.");
        assert_eq!(clean("Well (um, maybe) yes."), "Well (maybe) yes.");
        assert_eq!(clean("She said, \"Um, okay.\""), "She said, \"okay.\"");
        assert_eq!(clean("\u{201C}Um,\u{201D} he said."), "He said.");
    }

    #[test]
    fn leaves_the_rest_of_the_text_alone() {
        assert_eq!(
            clean("Um, the options are \"yes\" \"no\"."),
            "The options are \"yes\" \"no\"."
        );
        assert_eq!(clean("Um, call f() now"), "Call f() now");
        assert_eq!(clean("Um, Tom & Jerry"), "Tom & Jerry");
        assert_eq!(clean("Um, see #42"), "See #42");
        assert_eq!(clean("Um, x = 5 + 3"), "X = 5 + 3");
        assert_eq!(
            clean("I think \u{2014} um \u{2014} yes"),
            "I think \u{2014} yes"
        );
        assert_eq!(clean("Um, line one\nline two"), "Line one\nline two");
    }

    #[test]
    fn a_closing_quote_is_not_an_opener() {
        assert_eq!(
            clean("He said \"hello\" um there."),
            "He said \"hello\" there."
        );
        assert_eq!(clean("He said \"hello\" um."), "He said \"hello\".");
    }

    #[test]
    fn keeps_newlines_and_indentation() {
        assert_eq!(clean("Hello.\num, world"), "Hello.\nworld");
        assert_eq!(clean("Um, hello.\n\n"), "Hello.\n\n");
        assert_eq!(clean("Um, one\n  two"), "One\n  two");
        assert_eq!(clean("- item\n  um, next item"), "- item\n  next item");
    }

    #[test]
    fn keeps_a_closing_quote_tight_against_the_word() {
        assert_eq!(
            clean("She said \"okay um\" and left."),
            "She said \"okay\" and left."
        );
        assert_eq!(clean("\"Hello, um\" she said."), "\"Hello,\" she said.");
        // Removing a quoted filler must not flip the quote state for later text.
        assert_eq!(
            clean("He said \"um\" and \"hello\" um there."),
            "He said and \"hello\" there."
        );
    }

    #[test]
    fn does_not_capitalize_after_an_abbreviation() {
        assert_eq!(clean("at 3 p.m. um we left"), "at 3 p.m. we left");
        assert_eq!(clean("e.g. um the file"), "e.g. the file");
        assert_eq!(clean("Hello\u{2026} um, world"), "Hello\u{2026} world");
        // A capitalized filler still marks a sentence start.
        assert_eq!(clean("Hello. Um, how are you?"), "Hello. How are you?");
    }

    #[test]
    fn drops_dashes_that_only_framed_the_filler() {
        assert_eq!(clean("Um \u{2014} hello"), "Hello");
        assert_eq!(clean("Um - hello"), "Hello");
        assert_eq!(clean("I think -- um -- yes"), "I think -- yes");
        assert_eq!(clean("I think, um \u{2014} yes"), "I think \u{2014} yes");
        assert_eq!(clean("Yes \u{2014} um."), "Yes.");
    }

    #[test]
    fn capitalizes_the_word_inside_an_opener() {
        assert_eq!(clean("Um, (hello)."), "(Hello).");
        assert_eq!(clean("Um, \"hello\"."), "\"Hello\".");
    }

    #[test]
    fn collapses_ascii_dashes_around_a_filler() {
        assert_eq!(clean("I think - um - yes"), "I think - yes");
        assert_eq!(
            clean("I think \u{2014} um \u{2014} yes"),
            "I think \u{2014} yes"
        );
    }

    #[test]
    fn a_repeated_closer_does_not_panic() {
        // The second closer must not reuse the opener that was already removed.
        // The stray closer is the user's own text and stays.
        assert_eq!(clean("She said \u{2014} (um))"), "She said \u{2014})");
        assert_eq!(clean("\u{e9}(um))"), "\u{e9})");
    }

    #[test]
    fn a_quoted_filler_keeps_the_punctuation_after_it() {
        assert_eq!(clean("He said \"um\"."), "He said.");
        assert_eq!(clean("He said \"um\"!"), "He said!");
        assert_eq!(clean("(um)."), "");
    }

    #[test]
    fn a_colon_and_indentation_survive_a_framing_dash() {
        assert_eq!(
            clean("Header:\n\tum \u{2014} next item"),
            "Header:\n\t\u{2014} next item"
        );
    }

    #[test]
    fn consecutive_fillers_keep_the_sentence_capital() {
        assert_eq!(clean("Hello. Um, uh, how are you?"), "Hello. How are you?");
    }

    #[test]
    fn trailing_dash_runs_are_cleaned_completely() {
        assert_eq!(clean("Yes -- um."), "Yes.");
        assert_eq!(clean("Yes \u{2014} um \u{2014}."), "Yes.");
    }

    #[test]
    fn a_closed_pair_keeps_the_comma_after_it() {
        assert_eq!(
            clean("He said (um), and then left."),
            "He said, and then left."
        );
        assert_eq!(
            clean("She replied \"um\", and walked off."),
            "She replied, and walked off."
        );
        assert_eq!(clean("Fine (uh); next."), "Fine; next.");
        // A doubled separator still collapses.
        assert_eq!(clean("Wait, um, what?"), "Wait, what?");
    }

    #[test]
    fn nested_empty_pairs_disappear() {
        assert_eq!(clean("He said ((um))."), "He said.");
        assert_eq!(clean("He said (\"um\")."), "He said.");
        assert_eq!(clean("He said \"(um)\"."), "He said.");
    }

    #[test]
    fn a_dash_trim_does_not_leave_an_exposed_comma() {
        assert_eq!(clean("Yes, \u{2014} um."), "Yes.");
    }

    #[test]
    fn no_indentation_is_left_before_a_break() {
        assert_eq!(clean("Line one\n  um\nLine two"), "Line one\nLine two");
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
