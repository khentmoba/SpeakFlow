//! The local post-processing applied to a finished dictation.
//!
//! Everything here runs after recognition and after AI polish, in this order:
//!
//! 1. Repeated words and phrases are collapsed.
//! 2. A spoken snippet trigger is expanded into its saved text.
//! 3. A trailing voice command is removed and returned to the caller.
//!
//! The order matters. Snippets run after cleanup so a saved expansion is never
//! shortened by the repetition rules, and the command runs last so an expansion
//! that ends in "press enter" is still treated as text.

use crate::services::snippets::{self, Snippet};
use crate::services::transcript_cleanup;
use crate::services::voice_commands::{self, VoiceCommand};

/// The settings that decide which local steps run.
#[derive(Clone, Debug, Default)]
pub struct OutputTools {
    pub tidy: bool,
    pub voice_commands: bool,
    pub snippets: Vec<Snippet>,
}

impl OutputTools {
    /// Reads the persisted settings once per dictation.
    pub fn load() -> Self {
        Self {
            tidy: crate::utils::paths::read_tidy_transcript(),
            voice_commands: crate::utils::paths::read_voice_commands(),
            snippets: crate::utils::paths::read_snippets(),
        }
    }
}

/// A finished dictation plus the command the speaker asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedOutput {
    pub text: String,
    pub command: Option<VoiceCommand>,
}

/// Applies every enabled local step to `text`.
pub fn apply(text: &str, language: Option<&str>, tools: &OutputTools) -> AppliedOutput {
    let mut current = if tools.tidy {
        transcript_cleanup::tidy(text, language)
    } else {
        text.to_string()
    };

    if !tools.snippets.is_empty() {
        current = snippets::expand(&current, &tools.snippets);
    }

    let mut command = None;
    if tools.voice_commands {
        let (remaining, taken) = voice_commands::take_trailing_command(&current);
        if taken.is_some() {
            current = remaining;
            command = taken;
        }
    }

    AppliedOutput {
        text: current,
        command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> OutputTools {
        OutputTools {
            tidy: true,
            voice_commands: true,
            snippets: vec![Snippet {
                trigger: "my email".into(),
                expansion: "khent@example.com".into(),
            }],
        }
    }

    #[test]
    fn a_snippet_expansion_is_never_shortened_by_cleanup() {
        let mut tools = tools();
        tools.snippets = vec![Snippet {
            trigger: "the plan".into(),
            expansion: "the plan the plan".into(),
        }];
        let applied = apply("the plan", Some("en"), &tools);
        assert_eq!(applied.text, "the plan the plan");
    }

    #[test]
    fn cleanup_runs_before_expansion() {
        let applied = apply("my my email", Some("en"), &tools());
        assert_eq!(applied.text, "khent@example.com");
    }

    #[test]
    fn a_trailing_command_is_returned_and_removed() {
        let applied = apply("send the update, press enter", Some("en"), &tools());
        assert_eq!(applied.text, "send the update");
        assert_eq!(applied.command, Some(VoiceCommand::Enter));
    }

    #[test]
    fn disabled_steps_leave_the_text_alone() {
        let tools = OutputTools {
            tidy: false,
            voice_commands: false,
            snippets: Vec::new(),
        };
        let applied = apply("the the plan, press enter", Some("en"), &tools);
        assert_eq!(applied.text, "the the plan, press enter");
        assert_eq!(applied.command, None);
    }

    #[test]
    fn a_discard_command_survives_to_the_caller() {
        let applied = apply("scratch that", Some("en"), &tools());
        assert_eq!(applied.text, "");
        assert_eq!(applied.command, Some(VoiceCommand::Discard));
    }
}
