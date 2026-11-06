//! Named writing styles for AI polish.
//!
//! A tone is a short instruction appended to the polish prompt. It is a preset
//! rather than free text so the settings screen can offer one click, and so the
//! same wording is used for every provider.

/// The writing style applied to polished dictation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToneMode {
    /// Keep the user's own voice; no extra instruction.
    #[default]
    Off,
    Formal,
    Casual,
    Concise,
    Bullets,
    Email,
}

impl ToneMode {
    /// The stable identifier stored in settings and sent over IPC.
    pub fn as_str(self) -> &'static str {
        match self {
            ToneMode::Off => "off",
            ToneMode::Formal => "formal",
            ToneMode::Casual => "casual",
            ToneMode::Concise => "concise",
            ToneMode::Bullets => "bullets",
            ToneMode::Email => "email",
        }
    }

    /// Unknown values fall back to `Off` rather than failing a settings load.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "formal" => ToneMode::Formal,
            "casual" => ToneMode::Casual,
            "concise" => ToneMode::Concise,
            "bullets" => ToneMode::Bullets,
            "email" => ToneMode::Email,
            _ => ToneMode::Off,
        }
    }

    /// The instruction added to the polish prompt, or `None` for `Off`.
    pub fn instruction(self) -> Option<&'static str> {
        match self {
            ToneMode::Off => None,
            ToneMode::Formal => {
                Some("Use a formal, professional register. Avoid contractions and slang.")
            }
            ToneMode::Casual => Some(
                "Use a casual, conversational register. Contractions and light slang are fine.",
            ),
            ToneMode::Concise => Some(
                "Keep it short. Remove anything that does not add meaning, and prefer one sentence over three.",
            ),
            ToneMode::Bullets => Some(
                "When the text contains more than one item, render the items as a short bullet list. Otherwise leave it as prose.",
            ),
            ToneMode::Email => Some(
                "Format the result as a short email: a greeting line, one or two short paragraphs, then a sign-off. Do not invent names, facts or commitments.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_round_trips_through_its_identifier() {
        let modes = [
            ToneMode::Off,
            ToneMode::Formal,
            ToneMode::Casual,
            ToneMode::Concise,
            ToneMode::Bullets,
            ToneMode::Email,
        ];
        for mode in modes {
            assert_eq!(ToneMode::parse(mode.as_str()), mode);
        }
    }

    #[test]
    fn parsing_is_case_insensitive_and_trims() {
        assert_eq!(ToneMode::parse("  FORMAL "), ToneMode::Formal);
    }

    #[test]
    fn unknown_values_fall_back_to_off() {
        assert_eq!(ToneMode::parse("pirate"), ToneMode::Off);
        assert_eq!(ToneMode::parse(""), ToneMode::Off);
    }

    #[test]
    fn off_has_no_instruction() {
        assert!(ToneMode::Off.instruction().is_none());
        let modes = [
            ToneMode::Formal,
            ToneMode::Casual,
            ToneMode::Concise,
            ToneMode::Bullets,
            ToneMode::Email,
        ];
        for mode in modes {
            assert!(mode.instruction().is_some_and(|text| !text.is_empty()));
        }
    }
}
