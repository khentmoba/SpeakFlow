//! Zero-LLM deterministic spoken formatting and spoken symbol insertion.
//!
//! Runs after ASR recognition (and optional filler/tidy rules) without calling an
//! LLM. Covers structural phrases (`new line`, `new paragraph`, `open/close quote`)
//! and developer/punctuation symbols (`hashtag`, `underscore`, `at sign`, etc.).

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpokenFormattingOptions {
    pub spoken_formatting: bool,
    pub spoken_symbols: bool,
}

/// Apply enabled zero-LLM spoken formatting and symbol rules to `text`.
pub fn apply(text: &str, options: SpokenFormattingOptions) -> String {
    if (!options.spoken_formatting && !options.spoken_symbols) || text.trim().is_empty() {
        return text.to_string();
    }

    let mut current = text.to_string();
    if options.spoken_formatting {
        current = apply_spoken_formatting(&current);
    }
    if options.spoken_symbols {
        current = apply_spoken_symbols(&current);
    }
    normalize_line_whitespace(&current)
}

fn apply_spoken_formatting(input: &str) -> String {
    let mut out = input.to_string();

    // Paragraph breaks (longest phrases first).
    out = replace_phrase_ci(&out, "new paragraph", "\n\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "next paragraph", "\n\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "nuevo párrafo", "\n\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "nuevo parrafo", "\n\n", PhraseJoin::Break);

    // Single line breaks.
    out = replace_phrase_ci(&out, "new line", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "line break", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "next line", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "nueva línea", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "nueva linea", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "salto de línea", "\n", PhraseJoin::Break);
    out = replace_phrase_ci(&out, "salto de linea", "\n", PhraseJoin::Break);

    // Paired quote boundaries ("open quote ... close quote" / "quote ... unquote").
    out = replace_phrase_ci(&out, "open quote", "\"", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "close quote", "\"", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "abrir comillas", "\"", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "cerrar comillas", "\"", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "unquote", "\"", PhraseJoin::AttachLeft);
    out = replace_quote_word(&out);

    out
}

fn replace_quote_word(input: &str) -> String {
    // Only convert standalone "quote" when followed by text and usually paired or prefixed cleanly,
    // avoiding false positives inside normal English sentences unless standalone phrase boundaries match.
    let lower = input.to_ascii_lowercase();
    if !lower.contains("quote") {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = find_word_ci(rest, "quote") {
        let raw_before = &rest[..idx];
        let after = &rest[idx + 5..];
        let trimmed_after = after.trim_start_matches(' ');
        // Treat "quote <content>" as opening quote if followed by whitespace + word
        // and preceded by start/space/punctuation.
        if !trimmed_after.is_empty()
            && (raw_before.is_empty() || raw_before.ends_with([' ', '\n', ':', '(']))
        {
            out.push_str(&rest[..idx]);
            out.push('"');
            rest = trimmed_after;
        } else {
            out.push_str(&rest[..idx + 5]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn apply_spoken_symbols(input: &str) -> String {
    let mut out = input.to_string();

    // Joining symbols that bind adjacent tokens tightly (`foo underscore bar` -> `foo_bar`,
    // `hashtag rust` -> `#rust`, `at sign alice` -> `@alice`, `forward slash` -> `/`).
    out = replace_phrase_ci(&out, "hashtag", "#", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "hash tag", "#", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "pound sign", "#", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "underscore", "_", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "guion bajo", "_", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "at sign", "@", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "arroba", "@", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "forward slash", "/", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "backslash", "\\", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "slash", "/", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "hyphen", "-", PhraseJoin::GlueBoth);
    out = replace_phrase_ci(&out, "dash", " - ", PhraseJoin::Word);
    out = replace_phrase_ci(&out, "semicolon", ";", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "colon", ":", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "comma", ",", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "full stop", ".", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "period", ".", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "question mark", "?", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "exclamation mark", "!", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "exclamation point", "!", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "open paren", "(", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "close paren", ")", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "open parenthesis", "(", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "close parenthesis", ")", PhraseJoin::AttachLeft);
    out = replace_phrase_ci(&out, "dollar sign", "$", PhraseJoin::AttachRight);
    out = replace_phrase_ci(&out, "percent sign", "%", PhraseJoin::AttachLeft);

    out
}

#[derive(Clone, Copy)]
enum PhraseJoin {
    Break,
    AttachLeft,
    AttachRight,
    GlueBoth,
    Word,
}

fn find_word_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    let haystack_lower = haystack.to_ascii_lowercase();
    let bytes = haystack.as_bytes();
    let needle_len = needle_lower.len();
    let mut start = 0;
    while let Some(rel) = haystack_lower[start..].find(needle_lower) {
        let idx = start + rel;
        let end = idx + needle_len;
        let left_ok = idx == 0 || !bytes[idx - 1].is_ascii_alphanumeric();
        let right_ok = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok && haystack.is_char_boundary(idx) && haystack.is_char_boundary(end) {
            return Some(idx);
        }
        start = idx + 1;
        while start < haystack.len() && !haystack.is_char_boundary(start) {
            start += 1;
        }
    }
    None
}

fn replace_phrase_ci(
    input: &str,
    phrase_lower: &str,
    replacement: &str,
    join: PhraseJoin,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = find_word_ci(rest, phrase_lower) {
        let mut prefix = &rest[..idx];
        let mut suffix = &rest[idx + phrase_lower.len()..];

        // Swallow trailing comma/period ASR sometimes puts right after a spoken cue ("new line,").
        if matches!(join, PhraseJoin::Break) {
            if let Some(after_punct) = suffix.strip_prefix([',', '.', ';', ':']) {
                suffix = after_punct;
            }
        }

        match join {
            PhraseJoin::Break => {
                prefix = prefix.trim_end_matches([' ', '\t']);
                suffix = suffix.trim_start_matches([' ', '\t']);
                out.push_str(prefix);
                out.push_str(replacement);
            }
            PhraseJoin::AttachLeft => {
                prefix = prefix.trim_end_matches([' ', '\t']);
                out.push_str(prefix);
                out.push_str(replacement);
            }
            PhraseJoin::AttachRight => {
                suffix = suffix.trim_start_matches([' ', '\t']);
                out.push_str(prefix);
                out.push_str(replacement);
            }
            PhraseJoin::GlueBoth => {
                prefix = prefix.trim_end_matches([' ', '\t']);
                suffix = suffix.trim_start_matches([' ', '\t']);
                out.push_str(prefix);
                out.push_str(replacement);
            }
            PhraseJoin::Word => {
                prefix = prefix.trim_end_matches([' ', '\t']);
                suffix = suffix.trim_start_matches([' ', '\t']);
                out.push_str(prefix);
                out.push_str(replacement);
            }
        }
        rest = suffix;
    }
    out.push_str(rest);
    out
}

fn normalize_line_whitespace(input: &str) -> String {
    let lines: Vec<String> = input
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_matches(|c| c == ' ' || c == '\t');
            let mut collapsed = String::with_capacity(trimmed.len());
            let mut prev_space = false;
            for ch in trimmed.chars() {
                if ch == ' ' || ch == '\t' {
                    if !prev_space {
                        collapsed.push(' ');
                    }
                    prev_space = true;
                } else {
                    prev_space = false;
                    collapsed.push(ch);
                }
            }
            collapsed
        })
        .collect();
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_lines_paragraphs_and_quotes_without_llm() {
        let opts = SpokenFormattingOptions {
            spoken_formatting: true,
            spoken_symbols: false,
        };
        assert_eq!(
            apply("First line new line, second line new paragraph Third", opts),
            "First line\nsecond line\n\nThird"
        );
        assert_eq!(
            apply("He said open quote hello world close quote", opts),
            "He said \"hello world\""
        );
        assert_eq!(
            apply("Say quote ship it unquote today", opts),
            "Say \"ship it\" today"
        );
    }

    #[test]
    fn formats_spoken_symbols_and_identifiers() {
        let opts = SpokenFormattingOptions {
            spoken_formatting: false,
            spoken_symbols: true,
        };
        assert_eq!(apply("hashtag release", opts), "#release");
        assert_eq!(apply("user underscore id", opts), "user_id");
        assert_eq!(apply("ping at sign alice", opts), "ping @alice");
        assert_eq!(apply("src slash main.rs", opts), "src/main.rs");
        assert_eq!(apply("hello comma world period", opts), "hello, world.");
    }

    #[test]
    fn disabled_flags_preserve_literal_words() {
        let opts = SpokenFormattingOptions::default();
        assert_eq!(
            apply("new line hashtag underscore", opts),
            "new line hashtag underscore"
        );
    }
}
