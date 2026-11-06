//! Settings for the local dictation tools: snippets, tone, cleanup, spoken
//! commands, whisper mode and the local-only switch.
//!
//! One read and one write keep the settings screen simple and keep the IPC
//! surface small. Every field is persisted in `engine.json` next to the other
//! speech settings.

use crate::services::local_only;
use crate::services::snippets::{self, Snippet};
use crate::services::tone_modes::ToneMode;
use crate::utils::{paths, AppError};

/// The complete set of local dictation tools.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DictationToolsSettings {
    pub snippets: Vec<Snippet>,
    /// A `ToneMode` identifier: `off`, `formal`, `casual`, `concise`,
    /// `bullets` or `email`.
    pub tone_mode: String,
    pub tidy_transcript: bool,
    pub voice_commands: bool,
    pub whisper_mode: bool,
    pub local_only: bool,
}

impl DictationToolsSettings {
    fn load() -> Self {
        Self {
            snippets: paths::read_snippets(),
            tone_mode: paths::read_tone_mode().as_str().to_string(),
            tidy_transcript: paths::read_tidy_transcript(),
            voice_commands: paths::read_voice_commands(),
            whisper_mode: paths::read_whisper_mode(),
            local_only: paths::read_local_only(),
        }
    }

    /// Applies the settings that other subsystems cache in memory.
    fn apply_runtime_effects(&self) {
        local_only::apply(self.local_only);
    }
}

#[tauri::command]
pub async fn get_dictation_tools() -> Result<DictationToolsSettings, AppError> {
    Ok(DictationToolsSettings::load())
}

/// Saves the whole set and returns what was stored.
///
/// Snippets are normalized first, so the returned list is the one the user will
/// actually get: trimmed, deduplicated and capped.
#[tauri::command]
pub async fn set_dictation_tools(
    settings: DictationToolsSettings,
) -> Result<DictationToolsSettings, AppError> {
    let normalized = snippets::normalize(settings.snippets.iter().cloned());
    paths::write_snippets(&normalized).map_err(save_error)?;
    paths::write_tone_mode(ToneMode::parse(&settings.tone_mode)).map_err(save_error)?;
    paths::write_tidy_transcript(settings.tidy_transcript).map_err(save_error)?;
    paths::write_voice_commands(settings.voice_commands).map_err(save_error)?;
    paths::write_whisper_mode(settings.whisper_mode).map_err(save_error)?;
    paths::write_local_only(settings.local_only).map_err(save_error)?;

    let stored = DictationToolsSettings {
        snippets: normalized,
        tone_mode: ToneMode::parse(&settings.tone_mode).as_str().to_string(),
        ..settings
    };
    stored.apply_runtime_effects();
    Ok(stored)
}

fn save_error(error: std::io::Error) -> AppError {
    AppError::Other(format!("Failed to save the dictation tools: {error}"))
}
