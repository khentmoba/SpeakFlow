//! FunASR stdin/stdout protocol types and response parsing.

use crate::utils::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Command sent to the Python server
///
/// The JSON format the Python side expects is flat:
/// - `{"action": "status"}`
/// - `{"action": "transcribe", "audio_path": "/path/to/file.wav"}`
/// - `{"action": "exit"}`
///
/// Uses `#[serde(tag = "action")]` to produce flat JSON with an `action` field,
/// and `rename_all = "snake_case"` turns variant names into lowercase snake_case.
#[derive(Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ServerCommand {
    StreamStart {
        session_id: u64,
        context: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    StreamFeed {
        session_id: u64,
        offset: usize,
        audio_base64: String,
        audio_format: String,
        sample_rate: u32,
    },
    StreamFinish {
        session_id: u64,
    },
    StreamCancel {
        session_id: u64,
    },
    /// Transcribe an audio file
    Transcribe {
        #[serde(skip_serializing_if = "Option::is_none")]
        options: Option<crate::state::user_profile::R2T2Config>,
        /// Path of the audio file
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_path: Option<String>,
        /// In-memory audio payload (Base64)
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_base64: Option<String>,
        /// In-memory audio encoding format
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_format: Option<String>,
        /// In-memory audio sample rate
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
        /// Hotword list (optional)
        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
        /// Recognition language (optional). Absent means let the model auto-detect.
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
        /// Have models that support translation output English directly.
        #[serde(skip_serializing_if = "Option::is_none")]
        translate_to_english: Option<bool>,
        /// Whether to filter silence with VAD first (the server treats absent as enabled)
        #[serde(skip_serializing_if = "Option::is_none")]
        filter_silence: Option<bool>,
    },
    /// Query server status
    Status,
    /// Update the GPU idle-unload seconds. 0 means off; does not restart the process.
    SetGpuIdle {
        seconds: u64,
    },
    /// Update the read-only model folder list without restarting the process.
    SetModelDirs {
        dirs: Vec<String>,
    },
    /// Run speaker diarization on an audio buffer/path when the loaded model supports it.
    Diarize {
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_base64: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_format: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
    },
    /// Exit the server
    Exit,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizeSegment {
    #[serde(alias = "t0_ms")]
    pub t0_ms: u64,
    #[serde(alias = "t1_ms")]
    pub t1_ms: u64,
    #[serde(alias = "speaker_id")]
    pub speaker_id: u32,
    pub p: f64,
}

/// JSON response from the Python server
///
/// This struct matches the JSON format the Python server returns.
/// `Option<T>` means the field may or may not be present.
#[derive(Debug, Deserialize)]
pub(super) struct ServerResponse {
    pub(super) tentative_text: Option<String>,
    pub(super) session_id: Option<u64>,
    pub(super) sample_count: Option<usize>,
    #[serde(rename = "final", alias = "is_final")]
    pub(super) is_final: Option<bool>,
    pub(super) supports_streaming: Option<bool>,
    pub(super) supports_diarization: Option<bool>,
    pub(super) supported: Option<bool>,
    pub(super) segments: Option<Vec<DiarizeSegment>>,
    /// Request ID; with the new protocol it is used to drop stale responses that arrive late after a cancellation/timeout
    pub(super) request_id: Option<u64>,
    /// Whether the operation succeeded
    pub(super) success: Option<bool>,
    /// Status identifier
    pub(super) status: Option<String>,
    /// Transcribed text
    pub(super) text: Option<String>,
    /// Audio duration
    pub(super) duration: Option<f64>,
    /// Error message
    pub(super) error: Option<String>,
    /// Detected language
    pub(super) language: Option<String>,
    /// Additional message
    pub(super) message: Option<String>,
    /// Whether the model has loaded
    pub(super) model_loaded: Option<bool>,
    /// Whether the model has initialized (returned by Python status)
    pub(super) initialized: Option<bool>,
    /// Model loading state
    pub(super) models: Option<ServerModelStatus>,
    /// Device info
    pub(super) device: Option<String>,
    /// GPU name
    pub(super) gpu_name: Option<String>,
    /// Total GPU memory (GB)
    pub(super) gpu_memory_total: Option<f64>,
    /// Current engine
    pub(super) engine: Option<String>,
    /// Input mode the server actually uses (memory/path)
    pub(super) input_mode: Option<String>,
}

/// Model state returned by Python status
#[derive(Debug, Deserialize, Clone)]
pub(super) struct ServerModelStatus {
    pub(super) asr: Option<bool>,
    pub(super) vad: Option<bool>,
    pub(super) punc: Option<bool>,
}

impl ServerResponse {
    pub(super) fn is_model_loaded(&self) -> bool {
        self.model_loaded.unwrap_or_else(|| {
            self.models
                .as_ref()
                .map(|m| {
                    m.asr.unwrap_or(false) && m.vad.unwrap_or(false) && m.punc.unwrap_or(false)
                })
                .unwrap_or(false)
        })
    }
}

pub(super) async fn read_json_response<T, R>(
    reader: &mut R,
    timeout: Duration,
    context: &str,
) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    read_json_response_matching(reader, timeout, context, |_| true).await
}

pub(super) async fn read_json_response_matching<T, R>(
    reader: &mut R,
    timeout: Duration,
    context: &str,
    mut accept: impl FnMut(&T) -> bool,
) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    let start_at = Instant::now();
    let mut line_bytes = Vec::new();

    loop {
        let remaining = timeout
            .checked_sub(start_at.elapsed())
            .ok_or_else(|| AppError::Asr(format!("{context} timed out")))?;

        line_bytes.clear();
        let read_result =
            tokio::time::timeout(remaining, reader.read_until(b'\n', &mut line_bytes)).await;

        match read_result {
            Ok(Ok(0)) => {
                return Err(AppError::Asr(format!("{context} failed: stdout closed")));
            }
            Ok(Ok(_)) => {
                let line = match std::str::from_utf8(&line_bytes) {
                    Ok(line) => std::borrow::Cow::Borrowed(line),
                    Err(err) => {
                        log::warn!(
                            "{context} received non-UTF-8 output; tolerated as corrupted text: {err}"
                        );
                        String::from_utf8_lossy(&line_bytes)
                    }
                };

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(value) = serde_json::from_str::<T>(trimmed) {
                    if accept(&value) {
                        return Ok(value);
                    }
                    log::warn!("{context} discarded mismatched stale JSON response");
                    continue;
                }

                // On some Windows machines, third-party libraries squeeze noise output and the JSON response onto the same line.
                // Try to extract valid JSON from the first '{' to the last '}' so one piece of dirty output does not fail the whole initialization.
                if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
                    if start < end {
                        if let Ok(value) = serde_json::from_str::<T>(&trimmed[start..=end]) {
                            if !accept(&value) {
                                log::warn!("{context} discarded mismatched stale JSON response");
                                continue;
                            }
                            log::warn!("{context} recovered JSON response from mixed output");
                            return Ok(value);
                        }
                    }
                }

                log::warn!(
                    "{context} received non-JSON output ({} characters)",
                    trimmed.chars().count()
                );
                continue;
            }
            Ok(Err(e)) => {
                return Err(AppError::Asr(format!("{context} failed: {e}")));
            }
            Err(_) => {
                return Err(AppError::Asr(format!("{context} timed out")));
            }
        }
    }
}
