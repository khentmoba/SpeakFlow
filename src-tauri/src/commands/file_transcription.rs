//! Transcribes an audio file the user picks.
//!
//! The file is decoded locally, resampled to 16 kHz mono, then sent through the
//! same engine the microphone uses. A cloud engine receives the encoded WAV and
//! is blocked by local-only mode like any other outbound request.
//!
//! The finished transcript is stored in history when history is enabled, under
//! the `file` workflow, so it can be searched, copied and re-polished later.

use std::path::PathBuf;

use crate::services::audio_service;
use crate::services::file_transcription;
use crate::services::funasr_service::{self, TranscribeOptions};
use crate::services::{alibaba_asr_service, glm_asr_service, history_service};
use crate::state::AppState;
use crate::utils::{paths, AppError};

/// What the interface shows after a file finishes.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTranscriptionResult {
    pub file_name: String,
    pub text: String,
    pub language: Option<String>,
    pub duration_sec: f64,
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("audio file")
        .to_string()
}

/// Opens the picker, decodes the chosen file and transcribes it.
///
/// Returns `None` when the user cancels the picker.
#[tauri::command]
pub async fn transcribe_audio_file(
    window: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Option<FileTranscriptionResult>, AppError> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_title("Choose an audio file to transcribe")
        .add_filter("Audio", file_transcription::SUPPORTED_EXTENSIONS);
    if let Some(directory) = dirs::audio_dir() {
        dialog = dialog.set_directory(directory);
    }
    let Some(handle) = dialog.pick_file().await else {
        return Ok(None);
    };
    let path = PathBuf::from(handle.path());

    let decoded = tokio::task::spawn_blocking({
        let path = path.clone();
        move || file_transcription::decode_file(&path)
    })
    .await
    .map_err(|error| AppError::Other(format!("File decoding stopped unexpectedly: {error}")))?
    .map_err(AppError::Other)?;

    let duration_sec = decoded.samples.len() as f64 / decoded.sample_rate as f64;
    let engine = paths::read_engine_config();

    let started = std::time::Instant::now();
    let transcription = match engine.as_str() {
        "alibaba-asr" => {
            let wav = audio_service::encode_wav(&decoded.samples, decoded.sample_rate)?;
            alibaba_asr_service::transcribe(state.inner(), wav).await
        }
        "glm-asr" => {
            let wav = audio_service::encode_wav(&decoded.samples, decoded.sample_rate)?;
            glm_asr_service::transcribe(state.inner(), wav).await
        }
        _ => {
            funasr_service::transcribe_pcm16(
                state.inner(),
                &decoded.samples,
                decoded.sample_rate,
                &app_handle,
                TranscribeOptions { final_pass: true },
            )
            .await
        }
    };

    let elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let result = match transcription {
        Ok(result) if result.success => result,
        Ok(result) => {
            return Err(AppError::Asr(
                result
                    .error
                    .unwrap_or_else(|| "The file could not be transcribed".into()),
            ))
        }
        Err(error) => return Err(error),
    };

    let text = result.text.trim().to_string();
    let name = file_name(&path);
    save_to_history(
        state.inner(),
        &name,
        &text,
        result.language.as_deref(),
        duration_sec,
        &engine,
        elapsed_ms,
    )
    .await;

    Ok(Some(FileTranscriptionResult {
        file_name: name,
        text,
        language: result.language,
        duration_sec,
    }))
}

/// Stores the transcript so it can be searched and re-polished like a dictation.
///
/// History being off is not an error: the transcript is still returned to the
/// caller and shown in the interface.
async fn save_to_history(
    state: &AppState,
    name: &str,
    text: &str,
    language: Option<&str>,
    duration_sec: f64,
    engine: &str,
    elapsed_ms: u64,
) {
    if text.is_empty() {
        return;
    }
    let (enabled, retention_days) = state.with_profile(|profile| {
        (
            profile.history_settings.enabled,
            profile.history_settings.retention_days,
        )
    });
    if !enabled {
        return;
    }
    let draft = history_service::HistoryDraft {
        session_id: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
            .unwrap_or_default(),
        mode: "dictation".into(),
        workflow: "file".into(),
        status: "success".into(),
        text: text.to_string(),
        original_text: text.to_string(),
        source_text: Some(name.to_string()),
        duration_sec: Some(duration_sec),
        language: language.map(str::to_string),
        engine: engine.to_string(),
        provider: None,
        model: None,
        app_process: None,
        app_window_title: Some(name.to_string()),
        app_rule_name: None,
        audio_file: None,
        asr_ms: Some(elapsed_ms),
        polish_ms: None,
        total_ms: Some(elapsed_ms),
        error: None,
        reprocessed_from_id: None,
    };
    if let Err(error) = history_service::insert(draft, retention_days).await {
        log::warn!("Could not save the file transcript to history: {error}");
    }
}
