use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use super::oauth_session::OAuthSessionState;
use super::user_profile::{LlmProviderConfig, UserProfile};
use crate::services::claude_oauth_service::ClaudeOauthSession;
use crate::services::codex_oauth_service::OpenaiCodexOauthSession;
use crate::services::grok_build_oauth_service::GrokBuildOauthSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    Dictation,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RecordingPhase {
    Idle,
    Starting,
    Recording,
    Processing,
    Outcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RecordingOutcomeKind {
    TooShort,
    NoSpeech,
    AsrError,
    ProcessingError,
    StartError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RecordingSnapshot {
    pub session_id: u64,
    pub revision: u64,
    pub phase: RecordingPhase,
    pub mode: RecordingMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub outcome: Option<RecordingOutcomeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detail: Option<String>,
}

impl RecordingSnapshot {
    pub fn new(session_id: u64, revision: u64, phase: RecordingPhase, mode: RecordingMode) -> Self {
        Self {
            session_id,
            revision,
            phase,
            mode,
            outcome: None,
            detail: None,
        }
    }

    pub fn outcome(
        session_id: u64,
        revision: u64,
        mode: RecordingMode,
        outcome: RecordingOutcomeKind,
        detail: Option<&str>,
    ) -> Self {
        Self {
            session_id,
            revision,
            phase: RecordingPhase::Outcome,
            mode,
            outcome: Some(outcome),
            detail: detail.map(str::to_owned),
        }
    }
}

impl RecordingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationOutputMode {
    Original,
    Translated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingTrigger {
    DictationOriginal,
    DictationTranslated,
    Assistant,
}

impl RecordingTrigger {
    pub fn mode(self) -> RecordingMode {
        match self {
            Self::Assistant => RecordingMode::Assistant,
            Self::DictationOriginal | Self::DictationTranslated => RecordingMode::Dictation,
        }
    }

    pub fn dictation_output(self) -> DictationOutputMode {
        match self {
            Self::DictationTranslated => DictationOutputMode::Translated,
            Self::DictationOriginal | Self::Assistant => DictationOutputMode::Original,
        }
    }
}

#[derive(Clone)]
pub struct InterimCache {
    pub text: String,
    pub sample_count: usize,
    pub language: Option<String>,
}

pub struct RecordingSession {
    pub context_sources: crate::state::user_profile::ContextSources,
    pub context_capture:
        Option<tokio::task::JoinHandle<crate::services::context_awareness::CapturedContext>>,
    pub session_id: u64,
    pub subtitle_show_gen: u64,
    pub trigger: RecordingTrigger,
    pub stop_flag: Arc<AtomicBool>,
    pub stop_notify: Arc<tokio::sync::Notify>,
    pub samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    pub sample_rate: u32,
    pub audio_thread: Option<JoinHandle<()>>,
    pub interim_task: Option<tokio::task::JoinHandle<()>>,
    pub native_recording: Option<crate::services::audio_service::NativeRecording>,
    pub interim_cache: Arc<parking_lot::Mutex<Option<InterimCache>>>,
    /// Snapshot of the foreground app when the recording hotkey fired. Rules, history metadata and screenshot authorization must all be based on
    /// the same boundary, so switching windows during finalization cannot apply the wrong privacy policy.
    pub foreground_app: Option<crate::utils::foreground::ForegroundApp>,
    /// Exact window identity captured before asynchronous recording startup.
    pub paste_target: Option<isize>,
    /// Selected-text task grabbed in parallel when the hotkey is pressed. It lives and dies with the session, avoiding the
    /// cross-session contamination a global share would cause (finalize_N reading the grab of hotkey_{N+1}).
    pub edit_grab: Option<tokio::task::JoinHandle<Option<String>>>,
}

#[derive(Clone)]
pub struct PendingRecordingSession {
    pub session_id: u64,
    pub subtitle_show_gen: u64,
    pub trigger: RecordingTrigger,
    pub stop_flag: Arc<AtomicBool>,
    pub stop_notify: Arc<tokio::sync::Notify>,
}

pub enum RecordingSlot {
    Starting(PendingRecordingSession),
    Active(RecordingSession),
}

impl RecordingSlot {
    pub fn session_id(&self) -> u64 {
        match self {
            Self::Starting(s) => s.session_id,
            Self::Active(s) => s.session_id,
        }
    }

    pub fn trigger(&self) -> RecordingTrigger {
        match self {
            Self::Starting(s) => s.trigger,
            Self::Active(s) => s.trigger,
        }
    }

    pub fn subtitle_show_gen(&self) -> u64 {
        match self {
            Self::Starting(s) => s.subtitle_show_gen,
            Self::Active(s) => s.subtitle_show_gen,
        }
    }

    pub fn snapshot(&self, revision: u64) -> RecordingSnapshot {
        let phase = match self {
            Self::Starting(_) => RecordingPhase::Starting,
            Self::Active(_) => RecordingPhase::Recording,
        };
        RecordingSnapshot::new(self.session_id(), revision, phase, self.trigger().mode())
    }
}

pub struct MicrophoneLevelMonitor {
    pub stop_flag: Arc<AtomicBool>,
    pub handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyDiagnosticState {
    pub shortcut: String,
    pub registered: bool,
    pub backend: String,
    pub is_pressed: bool,
    pub last_error: Option<String>,
    pub warning: Option<String>,
    /// Non-empty when another program has registered the same hotkey via RegisterHotKey
    pub system_conflict: Option<String>,
    pub last_event: Option<String>,
    pub last_event_at_ms: Option<u64>,
    pub last_registered_at_ms: Option<u64>,
    pub last_pressed_at_ms: Option<u64>,
    pub last_released_at_ms: Option<u64>,
}

impl Default for HotkeyDiagnosticState {
    fn default() -> Self {
        Self {
            shortcut: String::new(),
            registered: false,
            backend: "none".into(),
            is_pressed: false,
            last_error: None,
            warning: None,
            system_conflict: None,
            last_event: None,
            last_event_at_ms: None,
            last_registered_at_ms: None,
            last_pressed_at_ms: None,
            last_released_at_ms: None,
        }
    }
}

// ---------- AppState sub-structs grouped by domain ----------

/// ASR engine lifecycle + downloads + transport capability probing
pub struct EngineState {
    /// Serialize entire native utterances, including final flush and cancellation.
    pub native_asr_owner: Arc<Mutex<()>>,
    pub funasr_process: Arc<Mutex<Option<FunasrProcess>>>,
    /// Child process that has been spawned but has not finished initializing. Uses a synchronous handle so start_kill can run immediately on cancel/Drop.
    pub funasr_starting_process: Arc<parking_lot::Mutex<Option<StartingFunasrProcess>>>,
    /// Serializes lifecycle operations such as restart / engine switch / model-dir migration.
    pub funasr_lifecycle_op: Mutex<()>,
    /// Engine archive extraction singleflight, preventing several start/download tasks from replacing the engine directory in parallel.
    pub engine_install_op: Mutex<()>,
    /// Makes the generation check of extraction progress and the state broadcast one atomic commit, so a stale loading
    /// cannot reach the frontend after the final state of stop/switch.
    pub funasr_status_commit: Arc<parking_lot::Mutex<()>>,
    pub funasr_ready: Arc<AtomicBool>,
    pub supports_native_streaming: AtomicBool,
    pub supports_diarization: AtomicBool,
    /// Current FunASR start owner. 0=idle, u64::MAX=start forbidden during migration.
    funasr_starting_owner: AtomicU64,
    /// Engine lifecycle generation; stop_server increments it and start_server uses it to detect cancellation
    pub funasr_generation: Arc<AtomicU64>,
    pub download_task: Arc<Mutex<Option<DownloadTask>>>,
    /// In-memory audio transport support state: 0=unknown, 1=supported, 2=unsupported
    pub inline_audio_transport: AtomicU8,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            native_asr_owner: Default::default(),
            funasr_process: Default::default(),
            funasr_starting_process: Default::default(),
            funasr_lifecycle_op: Default::default(),
            engine_install_op: Default::default(),
            funasr_status_commit: Default::default(),
            funasr_ready: Default::default(),
            supports_native_streaming: AtomicBool::new(false),
            supports_diarization: AtomicBool::new(false),
            funasr_starting_owner: AtomicU64::new(0),
            funasr_generation: Arc::new(AtomicU64::new(0)),
            download_task: Default::default(),
            inline_audio_transport: AtomicU8::new(0),
        }
    }
}

/// Current recording session + paste queue + microphone-related runtime
pub struct RecordingState {
    pub recording: Arc<parking_lot::Mutex<Option<RecordingSlot>>>,
    recording_snapshot: Arc<parking_lot::Mutex<Option<RecordingSnapshot>>>,
    snapshot_revision: AtomicU64,
    pub subtitle_window_op: Mutex<()>,
    pub session_counter: AtomicU64,
    pub(crate) pending_paste:
        Arc<parking_lot::Mutex<crate::services::audio_service::output::PendingPasteQueue>>,
    pub(crate) paste_dispatch: Mutex<()>,
    pub selected_input_device_name: Arc<parking_lot::Mutex<Option<String>>>,
    pub microphone_level_monitor: Arc<parking_lot::Mutex<Option<MicrophoneLevelMonitor>>>,
    pub microphone_level_monitor_generation: AtomicU64,
    pub subtitle_show_gen: AtomicU64,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            recording: Default::default(),
            recording_snapshot: Default::default(),
            snapshot_revision: AtomicU64::new(0),
            subtitle_window_op: Default::default(),
            session_counter: AtomicU64::new(0),
            pending_paste: Default::default(),
            paste_dispatch: Default::default(),
            selected_input_device_name: Default::default(),
            microphone_level_monitor: Default::default(),
            microphone_level_monitor_generation: AtomicU64::new(0),
            subtitle_show_gen: AtomicU64::new(0),
        }
    }
}

impl RecordingState {
    pub fn snapshot(&self) -> Option<RecordingSnapshot> {
        self.recording_snapshot.lock().clone()
    }

    /// Updates presentation state while the caller holds `recording`. Keeping
    /// the lock order `recording -> recording_snapshot` makes slot promotion
    /// and its UI revision one atomic transition.
    pub fn transition_snapshot_while_recording_locked(
        &self,
        session_id: u64,
        phase: RecordingPhase,
        mode: RecordingMode,
        outcome: Option<RecordingOutcomeKind>,
        detail: Option<&str>,
    ) -> Option<RecordingSnapshot> {
        if self.session_counter.load(Ordering::Acquire) != session_id {
            return None;
        }
        let revision = self.snapshot_revision.fetch_add(1, Ordering::AcqRel) + 1;
        let snapshot = match outcome {
            Some(outcome) if phase == RecordingPhase::Outcome => {
                RecordingSnapshot::outcome(session_id, revision, mode, outcome, detail)
            }
            _ => RecordingSnapshot::new(session_id, revision, phase, mode),
        };
        *self.recording_snapshot.lock() = Some(snapshot.clone());
        Some(snapshot)
    }

    /// Acquires locks in the canonical order for transitions made outside a
    /// slot mutation (for example terminal outcomes from finalize tasks).
    pub fn transition_snapshot_if_current(
        &self,
        session_id: u64,
        phase: RecordingPhase,
        mode: RecordingMode,
        outcome: Option<RecordingOutcomeKind>,
        detail: Option<&str>,
    ) -> Option<RecordingSnapshot> {
        let _recording = self.recording.lock();
        self.transition_snapshot_while_recording_locked(session_id, phase, mode, outcome, detail)
    }

    /// Clears presentation state while the caller holds `recording`.
    pub fn clear_snapshot_while_recording_locked(&self, session_id: u64) -> bool {
        let mut snapshot = self.recording_snapshot.lock();
        if snapshot
            .as_ref()
            .is_some_and(|current| current.session_id == session_id)
        {
            *snapshot = None;
            return true;
        }
        false
    }

    /// Clears presentation state without touching a newer session.
    pub fn clear_snapshot_if_session(&self, session_id: u64) -> bool {
        let _recording = self.recording.lock();
        self.clear_snapshot_while_recording_locked(session_id)
    }
}

/// User config + AI / ASR API keys of every kind + capability cache
#[derive(Default)]
pub struct ProfileState {
    pub user_profile: Arc<parking_lot::Mutex<UserProfile>>,
    pub ai_polish_enabled: Arc<AtomicBool>,
    pub openai_codex_oauth_session: OAuthSessionState<OpenaiCodexOauthSession>,
    pub grok_build_oauth_session: OAuthSessionState<GrokBuildOauthSession>,
    pub claude_oauth_session: OAuthSessionState<ClaudeOauthSession>,
    pub online_asr_api_key: Arc<parking_lot::Mutex<String>>,
    pub web_search_api_keys: Arc<parking_lot::Mutex<HashMap<String, String>>>,
    web_search_key_operation: parking_lot::Mutex<()>,
    pub assistant_image_support_cache: Arc<parking_lot::Mutex<HashMap<String, bool>>>,
    pub ai_polish_stream_started_sessions: Arc<parking_lot::Mutex<HashSet<u64>>>,
}

/// UI / interaction preferences + diagnostics
pub struct UiState {
    pub input_method: Arc<parking_lot::Mutex<String>>,
    pub sound_enabled: Arc<AtomicBool>,
    pub hotkey_diagnostic: Arc<parking_lot::Mutex<HotkeyDiagnosticState>>,
    pub assistant_chat_generation: AtomicU64,
    pub assistant_chat_cancel: Arc<parking_lot::Mutex<Option<AssistantChatTask>>>,
    pub selection_generation: AtomicU64,
    pub selection_cancel: Arc<parking_lot::Mutex<Option<SelectionTask>>>,
}

pub struct AssistantChatTask {
    pub generation: u64,
    pub cancel: oneshot::Sender<()>,
}

pub struct SelectionTask {
    pub generation: u64,
    pub cancel: oneshot::Sender<()>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            input_method: Arc::new(parking_lot::Mutex::new("sendInput".into())),
            sound_enabled: Arc::new(AtomicBool::new(true)),
            hotkey_diagnostic: Default::default(),
            assistant_chat_generation: AtomicU64::new(0),
            assistant_chat_cancel: Default::default(),
            selection_generation: AtomicU64::new(0),
            selection_cancel: Default::default(),
        }
    }
}

impl EngineState {
    pub fn is_funasr_starting(&self) -> bool {
        self.funasr_starting_owner.load(Ordering::SeqCst) != 0
    }

    pub fn try_begin_funasr_start(&self, owner: u64) -> bool {
        debug_assert!(owner != 0 && owner != u64::MAX);
        self.funasr_starting_owner
            .compare_exchange(0, owner, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn finish_funasr_start(&self, owner: u64) {
        let _ = self.funasr_starting_owner.compare_exchange(
            owner,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    pub fn owns_funasr_start(&self, owner: u64) -> bool {
        self.funasr_starting_owner.load(Ordering::SeqCst) == owner
    }

    pub fn block_funasr_starting(&self) {
        self.funasr_starting_owner.store(u64::MAX, Ordering::SeqCst);
    }

    pub fn unblock_funasr_starting(&self) {
        let _ = self.funasr_starting_owner.compare_exchange(
            u64::MAX,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

pub struct AppState {
    pub engine: EngineState,
    pub recording: RecordingState,
    pub profile: ProfileState,
    pub ui: UiState,
    pub http_client: reqwest::Client,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            engine: Default::default(),
            recording: Default::default(),
            profile: Default::default(),
            ui: Default::default(),
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(3))
                // Provider redirects must not forward API keys or private request bodies.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("failed to create the application HTTP client"),
        }
    }
}

pub struct DownloadTask {
    pub id: u64,
    pub cancel: Option<oneshot::Sender<()>>,
}

pub struct FunasrProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

pub struct StartingFunasrProcess {
    pub owner: u64,
    pub generation: u64,
    pub child: Arc<parking_lot::Mutex<Option<Child>>>,
}

impl Drop for FunasrProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_funasr_ready(&self) -> bool {
        self.engine.funasr_ready.load(Ordering::Acquire)
    }

    pub fn set_funasr_ready(&self, ready: bool) {
        self.engine.funasr_ready.store(ready, Ordering::Release);
    }

    pub fn snapshot_profile(&self) -> UserProfile {
        self.profile.user_profile.lock().clone()
    }

    /// Borrow the profile for read-only operations without cloning
    pub fn with_profile<R>(&self, f: impl FnOnce(&UserProfile) -> R) -> R {
        f(&self.profile.user_profile.lock())
    }

    /// Mutate the profile without returning a clone (use when persistence is not needed)
    pub fn update_profile_mut<R>(&self, f: impl FnOnce(&mut UserProfile) -> R) -> R {
        f(&mut self.profile.user_profile.lock())
    }

    pub fn active_llm_provider(&self) -> String {
        self.with_profile(|p| p.llm_provider.resolve_active_provider())
    }

    pub fn llm_provider_config(&self) -> LlmProviderConfig {
        self.with_profile(|p| p.llm_provider.clone())
    }

    pub fn read_openai_codex_oauth_session(&self) -> Option<OpenaiCodexOauthSession> {
        self.profile.openai_codex_oauth_session.read()
    }

    pub fn openai_codex_oauth_state(&self) -> &OAuthSessionState<OpenaiCodexOauthSession> {
        &self.profile.openai_codex_oauth_session
    }

    pub fn read_grok_build_oauth_session(&self) -> Option<GrokBuildOauthSession> {
        self.profile.grok_build_oauth_session.read()
    }

    pub fn grok_build_oauth_state(&self) -> &OAuthSessionState<GrokBuildOauthSession> {
        &self.profile.grok_build_oauth_session
    }

    pub fn read_claude_oauth_session(&self) -> Option<ClaudeOauthSession> {
        self.profile.claude_oauth_session.read()
    }

    pub fn claude_oauth_state(&self) -> &OAuthSessionState<ClaudeOauthSession> {
        &self.profile.claude_oauth_session
    }

    pub fn read_online_asr_api_key(&self) -> String {
        self.profile.online_asr_api_key.lock().clone()
    }

    pub fn set_online_asr_api_key(&self, api_key: impl Into<String>) {
        *self.profile.online_asr_api_key.lock() = api_key.into();
    }

    pub fn read_web_search_api_key(&self, provider: &str) -> String {
        self.profile
            .web_search_api_keys
            .lock()
            .get(provider)
            .cloned()
            .unwrap_or_default()
    }

    pub fn with_web_search_key_operation<R>(&self, operation: impl FnOnce() -> R) -> R {
        let _operation = self.profile.web_search_key_operation.lock();
        operation()
    }

    pub fn set_web_search_api_key(&self, provider: impl Into<String>, api_key: impl Into<String>) {
        let provider = provider.into();
        let api_key = api_key.into();
        let mut keys = self.profile.web_search_api_keys.lock();
        if api_key.is_empty() {
            keys.remove(&provider);
        } else {
            keys.insert(provider, api_key);
        }
    }

    pub fn inline_audio_transport(&self) -> Option<bool> {
        match self.engine.inline_audio_transport.load(Ordering::Acquire) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    pub fn set_inline_audio_transport(&self, supported: Option<bool>) {
        let encoded = match supported {
            Some(true) => 1,
            Some(false) => 2,
            None => 0,
        };
        self.engine
            .inline_audio_transport
            .store(encoded, Ordering::Release);
    }

    pub fn assistant_image_support(&self, cache_key: &str) -> Option<bool> {
        self.profile
            .assistant_image_support_cache
            .lock()
            .get(cache_key)
            .copied()
    }

    pub fn set_assistant_image_support(&self, cache_key: impl Into<String>, supported: bool) {
        self.profile
            .assistant_image_support_cache
            .lock()
            .insert(cache_key.into(), supported);
    }

    pub fn mark_ai_polish_stream_started(&self, session_id: u64) {
        self.profile
            .ai_polish_stream_started_sessions
            .lock()
            .insert(session_id);
    }

    pub fn take_ai_polish_stream_started(&self, session_id: u64) -> bool {
        self.profile
            .ai_polish_stream_started_sessions
            .lock()
            .remove(&session_id)
    }

    pub fn selected_input_device_name(&self) -> Option<String> {
        self.recording.selected_input_device_name.lock().clone()
    }

    pub fn set_selected_input_device_name(&self, name: Option<String>) {
        *self.recording.selected_input_device_name.lock() = name.and_then(|v| {
            let trimmed = v.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    }

    pub fn hotkey_diagnostic_snapshot(&self) -> HotkeyDiagnosticState {
        self.ui.hotkey_diagnostic.lock().clone()
    }

    pub fn update_hotkey_diagnostic<R>(
        &self,
        f: impl FnOnce(&mut HotkeyDiagnosticState) -> R,
    ) -> (R, HotkeyDiagnosticState) {
        let mut guard = self.ui.hotkey_diagnostic.lock();
        let result = f(&mut guard);
        (result, guard.clone())
    }
}

#[cfg(test)]
mod recording_snapshot_tests {
    use super::*;

    fn pending_slot(session_id: u64, trigger: RecordingTrigger) -> RecordingSlot {
        RecordingSlot::Starting(PendingRecordingSession {
            session_id,
            subtitle_show_gen: 1,
            trigger,
            stop_flag: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
        })
    }

    fn active_slot(session_id: u64, trigger: RecordingTrigger) -> RecordingSlot {
        RecordingSlot::Active(RecordingSession {
            context_capture: None,
            context_sources: Default::default(),
            session_id,
            subtitle_show_gen: 1,
            trigger,
            stop_flag: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
            samples: Arc::new(parking_lot::Mutex::new(Vec::new())),
            sample_rate: 16_000,
            audio_thread: None,
            interim_task: None,
            native_recording: None,
            interim_cache: Arc::new(parking_lot::Mutex::new(None)),
            foreground_app: None,
            paste_target: None,
            edit_grab: None,
        })
    }

    #[test]
    fn recording_snapshot_serializes_as_frontend_contract() {
        let value = serde_json::to_value(RecordingSnapshot::outcome(
            42,
            7,
            RecordingMode::Assistant,
            RecordingOutcomeKind::StartError,
            Some("microphone unavailable"),
        ))
        .expect("snapshot should serialize");

        assert_eq!(
            value,
            serde_json::json!({
                "sessionId": 42,
                "revision": 7,
                "phase": "outcome",
                "mode": "assistant",
                "outcome": "start_error",
                "detail": "microphone unavailable",
            })
        );
    }

    #[test]
    fn recording_slot_maps_starting_and_active_phases() {
        assert_eq!(
            pending_slot(7, RecordingTrigger::DictationOriginal)
                .snapshot(1)
                .phase,
            RecordingPhase::Starting
        );
        let active = active_slot(8, RecordingTrigger::Assistant).snapshot(2);
        assert_eq!(active.phase, RecordingPhase::Recording);
        assert_eq!(active.mode, RecordingMode::Assistant);
    }

    #[test]
    fn recording_snapshot_clear_is_session_scoped() {
        let state = RecordingState::default();
        state.session_counter.store(9, Ordering::Release);
        assert!(state
            .transition_snapshot_if_current(
                9,
                RecordingPhase::Processing,
                RecordingMode::Dictation,
                None,
                None,
            )
            .is_some());

        assert!(!state.clear_snapshot_if_session(8));
        assert_eq!(
            state.snapshot().map(|snapshot| snapshot.session_id),
            Some(9)
        );
        assert!(state.clear_snapshot_if_session(9));
        assert!(state.snapshot().is_none());
    }

    #[test]
    fn stale_session_cannot_replace_current_snapshot() {
        let state = RecordingState::default();
        state.session_counter.store(11, Ordering::Release);
        assert!(state
            .transition_snapshot_if_current(
                11,
                RecordingPhase::Recording,
                RecordingMode::Dictation,
                None,
                None,
            )
            .is_some());
        assert!(state
            .transition_snapshot_if_current(
                10,
                RecordingPhase::Outcome,
                RecordingMode::Assistant,
                Some(RecordingOutcomeKind::ProcessingError),
                None,
            )
            .is_none());
        assert_eq!(
            state.snapshot().map(|snapshot| snapshot.session_id),
            Some(11)
        );
    }

    #[test]
    fn recording_snapshot_revision_is_monotonic_across_phases() {
        let state = RecordingState::default();
        state.session_counter.store(12, Ordering::Release);
        let starting = state
            .transition_snapshot_if_current(
                12,
                RecordingPhase::Starting,
                RecordingMode::Dictation,
                None,
                None,
            )
            .expect("starting snapshot");
        let recording = state
            .transition_snapshot_if_current(
                12,
                RecordingPhase::Recording,
                RecordingMode::Dictation,
                None,
                None,
            )
            .expect("recording snapshot");
        let processing = state
            .transition_snapshot_if_current(
                12,
                RecordingPhase::Processing,
                RecordingMode::Dictation,
                None,
                None,
            )
            .expect("processing snapshot");

        assert!(starting.revision < recording.revision);
        assert!(recording.revision < processing.revision);
    }
}
