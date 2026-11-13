//! FunASR speech recognition service module
//!
//! This is the most central and most complex module in the app.
//! It manages the full lifecycle of the FunASR Python backend server:
//!
//! 1. **Locate the Python interpreter**: search for a usable Python by priority
//! 2. **Start the server**: launch the Python script as a child process
//! 3. **Communication protocol**: talk to the Python process in JSON over stdin/stdout
//! 4. **Speech transcription**: send audio data and receive transcription results
//! 5. **State management**: check server state and model state
//! 6. **Stop the server**: shut the Python process down gracefully
//!
//! # Communication protocol
//! The Rust process (parent) and the Python process (child) communicate over stdin/stdout:
//! - The parent writes JSON commands to the child's stdin
//! - After processing, the child writes JSON results to stdout
//! - Each message occupies one line (newline-delimited)
//!
//! ```text
//! [Rust/Tauri] --stdin--> [Python/FunASR]
//! [Rust/Tauri] <--stdout-- [Python/FunASR]
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::state::{AppState, EngineState, FunasrProcess, StartingFunasrProcess};
use crate::utils::paths;
use crate::utils::AppError;

mod installation;
mod ipc;
mod lifecycle;
mod model_files;
pub(crate) mod native_stream;
mod protocol;
mod transcription;

use ipc::*;
pub use lifecycle::*;
pub use model_files::*;
pub use transcription::*;

#[cfg(test)]
mod native_stream_tests;

#[cfg(test)]
#[cfg(target_os = "windows")]
mod ipc_recovery_tests;

#[cfg(test)]
use installation::{engine_install_fingerprint_matches, replace_engine_dir};
pub use installation::{find_engine, EngineRuntime};
use protocol::{read_json_response, read_json_response_matching, ServerResponse};
pub use protocol::{DiarizeSegment, ServerCommand};

// ============================================================
// Data structure definitions
// ============================================================

/// Result of speech transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Transcribed text
    pub text: String,
    /// Audio duration (seconds)
    pub duration: Option<f64>,
    /// Whether it succeeded
    pub success: bool,
    /// Error message (if it failed)
    pub error: Option<String>,
    /// Detected language
    pub language: Option<String>,
}

/// Status information of the FunASR server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunASRStatus {
    /// Whether the server is running
    pub running: bool,
    /// Whether the server is ready (model loaded)
    pub ready: bool,
    /// Whether the model has loaded
    pub model_loaded: bool,
    /// Inference device (cpu/cuda)
    pub device: Option<String>,
    /// GPU name (if available)
    pub gpu_name: Option<String>,
    /// Total GPU memory (GB)
    pub gpu_memory_total: Option<f64>,
    /// Status description
    pub message: String,
    /// Current engine
    pub engine: Option<String>,
    /// Whether the model files are complete
    pub models_present: Option<bool>,
    /// List of missing models
    pub missing_models: Option<Vec<String>>,
    pub model_state: Option<ModelState>,
    pub corrupt_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelState {
    Ready,
    Missing,
    Corrupt,
}

fn model_state_with_engine(model_state: ModelState, has_engine: bool) -> ModelState {
    if !has_engine && model_state == ModelState::Ready {
        ModelState::Missing
    } else {
        model_state
    }
}

#[cfg(test)]
mod runtime_readiness_tests {
    use super::{model_state_with_engine, ModelState};

    #[test]
    fn missing_engine_requires_download_without_hiding_corrupt_models() {
        assert_eq!(
            model_state_with_engine(ModelState::Ready, false),
            ModelState::Missing
        );
        assert_eq!(
            model_state_with_engine(ModelState::Missing, false),
            ModelState::Missing
        );
        assert_eq!(
            model_state_with_engine(ModelState::Corrupt, false),
            ModelState::Corrupt
        );
        assert_eq!(
            model_state_with_engine(ModelState::Ready, true),
            ModelState::Ready
        );
    }
}

/// Result of checking local model files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCheckResult {
    /// Whether all required models are present
    pub all_present: bool,
    /// Whether the ASR (speech recognition) model exists
    pub asr_model: bool,
    /// Whether the VAD (voice activity detection) model exists
    pub vad_model: bool,
    /// Whether the punctuation model exists
    pub punc_model: bool,
    /// Current engine
    pub engine: String,
    /// Path to the model cache directory
    pub cache_path: String,
    /// List of missing models
    pub missing_models: Vec<String>,
    pub model_state: ModelState,
    pub corrupt_models: Vec<String>,
}

/// Downloadable model file for one local engine. Catalog models come from
/// `src-tauri/resources/asr_catalog.json`, which pins the revision and checksum
/// for `hf_cache_utils.py`; R2T2 is the only entry defined here.
struct LocalModelFile {
    engine: &'static str,
    repo_id: &'static str,
    filename: &'static str,
    description: String,
    /// Expected file size, used to recognize a copy in a shared model folder.
    size: u64,
}

const R2T2_MODEL: (&str, &str, u64) = (
    "davidxifeng/Confucius4-R2T2-gguf",
    "r2t2-q8_0.gguf",
    2_477_512_064,
);

/// Model for a local engine. Unknown or legacy values fall back to the default engine.
fn local_model_file(engine: &str) -> LocalModelFile {
    if engine == paths::R2T2_ENGINE {
        return LocalModelFile {
            engine: paths::R2T2_ENGINE,
            repo_id: R2T2_MODEL.0,
            filename: R2T2_MODEL.1,
            description: "Confucius4-R2T2 Q8 model".to_string(),
            size: R2T2_MODEL.2,
        };
    }
    let model = crate::utils::asr_catalog::find(engine)
        .or_else(|| crate::utils::asr_catalog::find(paths::DEFAULT_ENGINE))
        .expect("the default engine is in the catalog");
    LocalModelFile {
        engine: model.key.as_str(),
        repo_id: model.repo_id.as_str(),
        filename: model.filename.as_str(),
        description: format!("{} Q8 model", model.name),
        size: model.size,
    }
}

const HF_COMPLETE_MANIFEST_NAME: &str = ".speak_flow_complete.json";

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartTicket {
    owner: u64,
    generation: u64,
    engine: String,
}

#[derive(Clone)]
struct EngineProgressGate {
    generation: Arc<AtomicU64>,
    status_commit: Arc<parking_lot::Mutex<()>>,
    expected_generation: u64,
}

impl EngineProgressGate {
    fn is_current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.expected_generation
    }

    fn commit_if_current(&self, commit: impl FnOnce()) {
        let _status_guard = self.status_commit.lock();
        if self.is_current() {
            commit();
        }
    }
}

/// Startup guard: synchronously kills an unpromoted child process on early return
/// or future cancellation, clears the matching starting slot, and releases only its own owner.
struct StartingFlagGuard<'a> {
    engine: &'a EngineState,
    owner: u64,
    child: Option<Arc<parking_lot::Mutex<Option<tokio::process::Child>>>>,
}

impl StartingFlagGuard<'_> {
    fn register_child(&mut self, child: Arc<parking_lot::Mutex<Option<tokio::process::Child>>>) {
        self.child = Some(child);
    }

    fn release_child(&mut self) {
        self.child = None;
    }
}

impl Drop for StartingFlagGuard<'_> {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            if let Some(mut child) = child.lock().take() {
                let _ = child.start_kill();
            }
        }
        let mut starting = self.engine.funasr_starting_process.lock();
        if starting
            .as_ref()
            .is_some_and(|entry| entry.owner == self.owner)
        {
            starting.take();
        }
        self.engine.finish_funasr_start(self.owner);
    }
}

const SERVER_INIT_TIMEOUT_SECS: u64 = 120;
const SERVER_RESPONSE_TIMEOUT_SECS: u64 = 60;
const SERVER_EXIT_WRITE_TIMEOUT_MS: u64 = 300;
const SERVER_EXIT_WAIT_TIMEOUT_SECS: u64 = 2;
const INLINE_AUDIO_FORMAT_PCM_S16LE: &str = "pcm_s16le";
const TARGET_SAMPLE_RATE_FOR_INLINE_AUDIO: u32 = 16_000;
const ENGINE_ARCHIVE_FINGERPRINT: &str = env!("SPEAK_FLOW_ENGINE_ARCHIVE_FINGERPRINT");
static NEXT_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SERVER_START_OWNER: AtomicU64 = AtomicU64::new(1);

fn next_server_start_owner() -> u64 {
    loop {
        let owner = NEXT_SERVER_START_OWNER.fetch_add(1, Ordering::Relaxed);
        if owner != 0 && owner != u64::MAX {
            return owner;
        }
    }
}

fn start_ticket_is_current(state: &AppState, ticket: &StartTicket) -> bool {
    state.engine.owns_funasr_start(ticket.owner)
        && state.engine.funasr_generation.load(Ordering::SeqCst) == ticket.generation
        && paths::read_engine_config() == ticket.engine
}

fn take_starting_child(
    state: &AppState,
    ticket: &StartTicket,
    expected_handle: &Arc<parking_lot::Mutex<Option<tokio::process::Child>>>,
) -> Option<tokio::process::Child> {
    let entry = {
        let mut slot = state.engine.funasr_starting_process.lock();
        let matches = slot.as_ref().is_some_and(|entry| {
            entry.owner == ticket.owner
                && entry.generation == ticket.generation
                && Arc::ptr_eq(&entry.child, expected_handle)
        });
        matches.then(|| slot.take()).flatten()
    }?;
    let child = entry.child.lock().take();
    child
}

fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn status_with_defaults(
    running: bool,
    ready: bool,
    model_loaded: bool,
    message: String,
) -> FunASRStatus {
    FunASRStatus {
        running,
        ready,
        model_loaded,
        device: None,
        gpu_name: None,
        gpu_memory_total: None,
        message,
        engine: None,
        models_present: None,
        missing_models: None,
        model_state: None,
        corrupt_models: None,
    }
}

fn report_model_repo_file_state(
    repo_id: &str,
    required_file: Option<&str>,
    description: &str,
    missing_models: &mut Vec<String>,
    corrupt_models: &mut Vec<String>,
) -> ModelState {
    let model_state = hf_repo_model_state(repo_id, required_file);
    match model_state {
        ModelState::Ready => log::info!("Model files are ready: {} ({})", description, repo_id),
        ModelState::Missing => missing_models.push(description.to_string()),
        ModelState::Corrupt => corrupt_models.push(description.to_string()),
    }
    model_state
}

// ============================================================
// Core implementation
// ============================================================

#[cfg(test)]
mod request_identity_tests {
    use super::server_response_matches_request;

    #[test]
    fn untagged_late_reply_cannot_satisfy_a_new_request() {
        assert!(!server_response_matches_request(None, 2));
        assert!(!server_response_matches_request(Some(1), 2));
        assert!(server_response_matches_request(Some(2), 2));
    }
}

async fn try_send_exit_command(process: &mut FunasrProcess) -> Result<(), AppError> {
    let command_json = serde_json::to_string(&ServerCommand::Exit)
        .map_err(|e| AppError::Asr(format!("Failed to serialize exit command: {}", e)))?;
    let timeout = Duration::from_millis(SERVER_EXIT_WRITE_TIMEOUT_MS);

    tokio::time::timeout(
        timeout,
        process
            .stdin
            .write_all(format!("{}\n", command_json).as_bytes()),
    )
    .await
    .map_err(|_| AppError::Asr("Timed out writing exit command".to_string()))?
    .map_err(|e| AppError::Asr(format!("Failed to write exit command: {}", e)))?;

    tokio::time::timeout(timeout, process.stdin.flush())
        .await
        .map_err(|_| AppError::Asr("Timed out flushing exit command".to_string()))?
        .map_err(|e| AppError::Asr(format!("Failed to flush exit command: {}", e)))?;

    Ok(())
}

/// Sends the GPU idle timeout (in seconds) to the running Python process, or only saves the config when stopped.
///
/// Does not modify `ready`: post-unload status remains ready via the existing sticky logic.
pub async fn push_gpu_idle_seconds(state: &AppState, seconds: u64) -> Result<(), AppError> {
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };
    if !has_process {
        return Ok(());
    }
    let response =
        send_command_to_server::<tauri::Wry>(state, &ServerCommand::SetGpuIdle { seconds }, None)
            .await?;
    if response.success == Some(false) {
        return Err(AppError::Asr(response.error.unwrap_or_else(|| {
            "Failed to configure GPU idle unload".to_string()
        })));
    }
    Ok(())
}

/// Sends the current model-folder list to a running engine.
///
/// The engine reads the folders once at startup, so a change must reach the
/// resident process; otherwise an idle-unloaded model reloads from the old list.
pub async fn push_model_dirs(state: &AppState) -> Result<(), AppError> {
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };
    if !has_process {
        return Ok(());
    }
    let dirs = paths::read_model_dirs();
    let response =
        send_command_to_server::<tauri::Wry>(state, &ServerCommand::SetModelDirs { dirs }, None)
            .await?;
    if response.success == Some(false) {
        return Err(AppError::Asr(response.error.unwrap_or_else(|| {
            "Failed to apply the model folders".to_string()
        })));
    }
    Ok(())
}

/// Checks the status of the FunASR server
// Emitter trait is required for the emit method
use tauri::Emitter;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod model_readiness_tests;
