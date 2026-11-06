//! Recognizes a spoken command at the end of a dictation.
//!
//! The command must be the last words of the transcript, because that is the
//! only position where the speaker is addressing the app rather than writing
//! text. Only phrases whose spoken form is unambiguous are recognized: "send
//! it" is deliberately absent because "I will send it" is ordinary content.
//!
//! "Scratch that" only applies to a dictation that is nothing but the command.
//! With other words in front, discarding would silently throw away text the
//! speaker did not ask to lose, so the phrase is kept as content instead.

/// An action the speaker asked for, applied after the text is finalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceCommand {
    /// Press Enter in the target window after the text is delivered.
    Enter,
    /// Insert a single line break.
    Newline,
    /// Insert a blank line.
    Paragraph,
    /// Throw the whole dictation away.
    Discard,
}

impl VoiceCommand {
    /// The literal text this command appends, if any.
    pub fn insertion(self) -> &'static str {
        match self {
            VoiceCommand::Enter | VoiceCommand::Discard => "",
            VoiceCommand::Newline => "\n",
            VoiceCommand::Paragraph => "\n\n",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            VoiceCommand::Enter => "enter",
            VoiceCommand::Newline => "newline",
            VoiceCommand::Paragraph => "paragraph",
            VoiceCommand::Discard => "discard",
        }
    }
}

/// Phrases in the order they are tried. Longer phrases come first so
/// "new paragraph" is not read as "new line".
const PHRASES: &[(&str, VoiceCommand)] = &[
    ("new paragraph", VoiceCommand::Paragraph),
    ("scratch that", VoiceCommand::Discard),
    ("press enter", VoiceCommand::Enter),
    ("new line", VoiceCommand::Newline),
];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-'
}

/// Lowercased words of `text`, ignoring punctuation and spacing.
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

/// Byte spans of every word in `text`, in order.
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

/// Removes a trailing command from `text`.
///
/// Returns the remaining text and the command, if one was recognized. The
/// returned text is trimmed of the separator that only existed to introduce the
/// command, so "write the note, press enter" becomes "write the note".
pub fn take_trailing_command(text: &str) -> (String, Option<VoiceCommand>) {
    let spans = word_spans(text);
    let Some(&(_, end)) = spans.last() else {
        return (text.to_string(), None);
    };
    let spoken = words(&text[..end]);
    if spoken.is_empty() {
        return (text.to_string(), None);
    }

    for (phrase, command) in PHRASES {
        let phrase_words = words(phrase);
        if phrase_words.len() > spoken.len() {
            continue;
        }
        let tail = &spoken[spoken.len() - phrase_words.len()..];
        if tail != phrase_words.as_slice() {
            continue;
        }
        if *command == VoiceCommand::Discard && spoken.len() != phrase_words.len() {
            // "buy milk scratch that" is not a request to lose "buy milk".
            continue;
        }

        // The phrase covers the last `phrase_words.len()` words.
        let cut = spans[spans.len() - phrase_words.len()].0;
        let head = text[..cut].trim_end();
        let head = head
            .trim_end_matches([',', ';', ':', '-', '\u{2014}', '\u{2013}'])
            .trim_end();
        return (head.to_string(), Some(*command));
    }

    (text.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_enter_is_stripped_from_the_end() {
        assert_eq!(
            take_trailing_command("Write the note, press enter."),
            ("Write the note".to_string(), Some(VoiceCommand::Enter))
        );
    }

    #[test]
    fn new_line_and_new_paragraph_keep_their_order() {
        assert_eq!(
            take_trailing_command("first item new line"),
            ("first item".to_string(), Some(VoiceCommand::Newline))
        );
        assert_eq!(
            take_trailing_command("done new paragraph"),
            ("done".to_string(), Some(VoiceCommand::Paragraph))
        );
    }

    #[test]
    fn scratch_that_only_discards_a_whole_dictation() {
        assert_eq!(
            take_trailing_command("Scratch that."),
            (String::new(), Some(VoiceCommand::Discard))
        );
        assert_eq!(
            take_trailing_command("buy milk scratch that"),
            ("buy milk scratch that".to_string(), None)
        );
    }

    #[test]
    fn ambiguous_phrases_are_left_as_content() {
        assert_eq!(
            take_trailing_command("I will send it"),
            ("I will send it".to_string(), None)
        );
        assert_eq!(
            take_trailing_command("the new line of products"),
            ("the new line of products".to_string(), None)
        );
    }

    #[test]
    fn a_command_word_inside_the_sentence_is_not_a_command() {
        assert_eq!(
            take_trailing_command("press enter to continue reading"),
            ("press enter to continue reading".to_string(), None)
        );
    }

    #[test]
    fn text_without_a_command_is_unchanged() {
        assert_eq!(
            take_trailing_command("Just a normal sentence."),
            ("Just a normal sentence.".to_string(), None)
        );
        assert_eq!(take_trailing_command(""), (String::new(), None));
        assert_eq!(take_trailing_command("   "), ("   ".to_string(), None));
    }

    #[test]
    fn the_command_alone_leaves_empty_text() {
        assert_eq!(
            take_trailing_command("press enter"),
            (String::new(), Some(VoiceCommand::Enter))
        );
    }

    #[test]
    fn a_hyphenated_word_before_the_command_survives() {
        assert_eq!(
            take_trailing_command("use the well-known fix, press enter"),
            (
                "use the well-known fix".to_string(),
                Some(VoiceCommand::Enter)
            )
        );
    }
}
