use tauri::Emitter;

use crate::services::funasr_service;
use crate::services::llm_provider;
use crate::state::AppState;
use crate::utils::{paths, AppError};

#[tauri::command]
pub async fn set_r2t2_config(
    state: tauri::State<'_, AppState>,
    context: String,
    language: Option<String>,
) -> Result<(), AppError> {
    // ISO language identifiers from the pinned Confucius4-R2T2 model spec.
    const LANGUAGES: &[&str] = &[
        "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru", "th", "vi", "ja",
        "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs", "fil", "fa", "el", "hu", "mk", "ro",
    ];
    let language = language
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());
    if language
        .as_deref()
        .is_some_and(|value| !LANGUAGES.contains(&value))
    {
        return Err(AppError::Other(
            "R2T2 does not support the selected language".into(),
        ));
    }
    crate::services::profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.r2t2.context = context.trim().to_string();
        profile.r2t2.language = language;
    });
    Ok(())
}

fn lock_idle_asr(state: &AppState) -> Result<tokio::sync::MutexGuard<'_, ()>, AppError> {
    let recording = state.recording.recording.lock().is_some();
    let processing = state
        .recording
        .snapshot()
        .is_some_and(|snapshot| snapshot.phase == crate::state::RecordingPhase::Processing);
    if recording || processing {
        return Err(AppError::Other(
            "Recording or processing is still in progress; please change the speech engine later"
                .into(),
        ));
    }
    state.engine.native_asr_owner.try_lock().map_err(|_| {
        AppError::Other(
            "Speech recognition is in progress; please change the speech engine later".into(),
        )
    })
}

#[tauri::command]
pub async fn start_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR server started".to_string())
}

/// IPC audio limit. The frontend is trusted (same-process renderer), but a uniform cap still applies:
/// 1) Guard against abnormal paths (leaked recordings, bugs, future callers) exhausting memory in one go.
/// 2) Online engines already have stricter limits (DashScope 10MB, GLM similar); one shared cap
///    gives local engines the same protection. Base64 is 4/3 the size of raw bytes, so 64MB raw
///    is ~85MB base64, about 33 minutes of 16kHz/16-bit mono, enough for
///    ordinary interactive recordings; longer audio should use a segmented approach.
const MAX_TRANSCRIBE_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_TRANSCRIBE_AUDIO_BASE64_BYTES: usize =
    MAX_TRANSCRIBE_AUDIO_BYTES.saturating_mul(4) / 3 + 4;

#[tauri::command]
pub async fn transcribe_audio(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    audio_base64: String,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    use base64::Engine;
    if audio_base64.len() > MAX_TRANSCRIBE_AUDIO_BASE64_BYTES {
        return Err(AppError::Asr(format!(
            "Audio is too large: base64 size {} bytes exceeds limit {} bytes",
            audio_base64.len(),
            MAX_TRANSCRIBE_AUDIO_BASE64_BYTES
        )));
    }
    let audio_data = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| AppError::Asr(format!("Base64 decode failed: {}", e)))?;
    if audio_data.len() > MAX_TRANSCRIBE_AUDIO_BYTES {
        return Err(AppError::Asr(format!(
            "Audio is too large: decoded size {} bytes exceeds limit {} bytes",
            audio_data.len(),
            MAX_TRANSCRIBE_AUDIO_BYTES
        )));
    }
    funasr_service::transcribe(state.inner(), audio_data, &app_handle).await
}

#[tauri::command]
pub async fn check_funasr_status(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<funasr_service::FunASRStatus, AppError> {
    funasr_service::check_status(state.inner(), &app_handle).await
}

#[tauri::command]
pub async fn check_model_files() -> Result<funasr_service::ModelCheckResult, AppError> {
    funasr_service::check_model_files().await
}

/// Result of the model test in Settings.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestTranscriptionResult {
    pub text: String,
    pub language: Option<String>,
    pub duration: Option<f64>,
}

/// Records a short clip and transcribes it with the selected engine.
///
/// This is a check, not a dictation: the text is returned to the caller and is
/// never polished, saved to history, or pasted into the active application.
#[tauri::command]
pub async fn test_transcription(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    seconds: u32,
) -> Result<TestTranscriptionResult, AppError> {
    if paths::is_online_engine(&paths::read_engine_config()) {
        return Err(AppError::Asr(
            "Model test works with local models only".to_string(),
        ));
    }
    if !state.is_funasr_ready() {
        return Err(AppError::Asr(
            "Speech recognition engine is not ready yet; please wait for initialization to finish"
                .to_string(),
        ));
    }
    // Own the engine for the whole test, so a model switch or a dictation
    // cannot take it between the recording and the decode.
    let _asr_guard = match state.engine.native_asr_owner.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            return Err(AppError::Asr(
                "A recording is in progress; finish it before testing the model".to_string(),
            ))
        }
    };
    let device = state.selected_input_device_name();
    let samples = tokio::task::spawn_blocking(move || {
        crate::services::audio_service::record_test_clip_sync(device, seconds)
    })
    .await
    .map_err(|error| AppError::Audio(format!("Model test recording failed: {error}")))??;

    let result = funasr_service::transcribe_pcm16_owned(
        state.inner(),
        &samples,
        16_000,
        &app_handle,
        funasr_service::TranscribeOptions { final_pass: true },
    )
    .await?;
    if !result.success {
        return Err(AppError::Asr(result.error.unwrap_or_else(|| {
            "The model could not transcribe the test clip".to_string()
        })));
    }
    Ok(TestTranscriptionResult {
        text: result.text,
        language: result.language,
        duration: result.duration,
    })
}

/// Speech-processing preferences that change how a recording is transcribed.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceProcessingSettings {
    /// Trim silence with the VAD before recognition.
    pub silence_filter: bool,
    /// Remove spoken filler words from the finished transcript.
    pub remove_fillers: bool,
    /// Saved recognition language for local transcribe.cpp models. `None`
    /// means auto-detect.
    pub language: Option<String>,
    /// Ask a translation-capable model to write English from any language.
    pub translate_to_english: bool,
}

#[tauri::command]
pub async fn get_voice_processing() -> Result<VoiceProcessingSettings, AppError> {
    Ok(VoiceProcessingSettings {
        silence_filter: paths::read_silence_filter(),
        remove_fillers: paths::read_remove_fillers(),
        language: paths::read_asr_language(),
        translate_to_english: paths::read_translate_to_english(),
    })
}

#[tauri::command]
pub async fn set_silence_filter(enabled: bool) -> Result<bool, AppError> {
    paths::write_silence_filter(enabled).map_err(|error| {
        AppError::Other(format!(
            "Failed to save the silence filter setting: {error}"
        ))
    })?;
    Ok(enabled)
}

#[tauri::command]
pub async fn set_remove_fillers(enabled: bool) -> Result<bool, AppError> {
    paths::write_remove_fillers(enabled).map_err(|error| {
        AppError::Other(format!(
            "Failed to save the filler removal setting: {error}"
        ))
    })?;
    Ok(enabled)
}

/// Saves the recognition language for local transcribe.cpp models. `None` and
/// `"auto"` both mean "let the model detect it". The selection is validated
/// against the selected model when a recording starts, so switching models
/// never leaves an unusable language behind.
#[tauri::command]
pub async fn set_asr_language(language: Option<String>) -> Result<Option<String>, AppError> {
    paths::write_asr_language(language.as_deref())
        .map_err(|error| AppError::Other(format!("Failed to save the language: {error}")))?;
    Ok(paths::read_asr_language())
}

#[tauri::command]
pub async fn set_translate_to_english(enabled: bool) -> Result<bool, AppError> {
    paths::write_translate_to_english(enabled).map_err(|error| {
        AppError::Other(format!("Failed to save the translation setting: {error}"))
    })?;
    Ok(enabled)
}

/// Extra read-only folders searched for model files, in search order.
#[tauri::command]
pub async fn get_model_dirs() -> Result<Vec<String>, AppError> {
    Ok(paths::read_model_dirs())
}

/// Replaces the folder list. Entries are trimmed, and empty, relative or
/// duplicate paths are dropped before saving.
#[tauri::command]
pub async fn set_model_dirs(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    dirs: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let normalized = paths::normalize_model_dirs(dirs.iter().map(String::as_str));
    paths::write_model_dirs(&normalized)
        .map_err(|error| AppError::Other(format!("Failed to save model folders: {error}")))?;
    // A resident engine keeps the list it started with, so hand it the new one.
    // If that fails, restart it: a stale list would load the wrong file or fail
    // to find a model that is now in a different folder.
    if let Err(error) = funasr_service::push_model_dirs(state.inner()).await {
        log::warn!("Model folder saved, but the running engine could not be notified; restarting instead: {error}");
        if !state.engine.is_funasr_starting() {
            let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
            let asr_guard = lock_idle_asr(state.inner())?;
            funasr_service::stop_server(state.inner()).await?;
            drop(asr_guard);
            drop(lifecycle_guard);
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            funasr_service::start_server(&app_handle, state.inner()).await?;
        }
    }
    Ok(normalized)
}

/// Catalog engines whose model file is already downloaded.
#[tauri::command]
pub async fn list_installed_models() -> Result<Vec<String>, AppError> {
    tokio::task::spawn_blocking(funasr_service::installed_catalog_engines)
        .await
        .map_err(|error| AppError::Other(format!("Listing installed models failed: {error}")))
}

#[tauri::command]
pub async fn download_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn cancel_model_download(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let cancellation = {
        let mut guard = state.engine.download_task.lock().await;
        guard.as_mut().and_then(|task| task.cancel.take())
    };

    if let Some(cancel) = cancellation {
        let _ = cancel.send(());
        Ok("Model download cancelled".to_string())
    } else if state.engine.download_task.lock().await.is_some() {
        Ok("Cancelling model download".to_string())
    } else {
        Ok("No active download task".to_string())
    }
}

#[tauri::command]
pub async fn restart_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let asr_guard = lock_idle_asr(state.inner())?;
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        // Online engines do not need a Python restart; just refresh the ready state
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        return Ok("Online engine status refreshed".to_string());
    }

    // The first launch may still be extracting engine.zip or loading the model; an automatic restart must not interrupt that.
    if state.engine.is_funasr_starting() {
        log::info!("FunASR is starting up; skipping this restart request");
        return Ok("FunASR is starting; skipping restart".to_string());
    }

    log::info!("Restarting FunASR server...");
    funasr_service::stop_server(state.inner()).await?;
    drop(asr_guard);
    drop(lifecycle_guard);
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR server restarted".to_string())
}

#[tauri::command]
pub async fn get_engine() -> Result<String, AppError> {
    Ok(paths::read_engine_config())
}

const GLM_ASR_KEYRING_USER: &str = "glm-asr-api-key";
const ALIBABA_ASR_INTL_KEYRING_USER: &str = "alibaba-asr-intl-api-key";
const ALIBABA_ASR_CN_KEYRING_USER: &str = "alibaba-asr-cn-api-key";

const VALID_ONLINE_KEYRING_USERS: &[&str] = &[
    GLM_ASR_KEYRING_USER,
    ALIBABA_ASR_INTL_KEYRING_USER,
    ALIBABA_ASR_CN_KEYRING_USER,
];

/// Compute the keyring user name for the currently active online engine.
///
/// GLM shares one entry across all regions (historical behavior); Alibaba keeps CN / Intl separate,
/// because Alibaba Cloud's international and mainland sites are two independent consoles with two separate DashScope API keys.
pub(crate) fn active_online_keyring_user() -> &'static str {
    let engine = paths::read_engine_config();
    if engine == "alibaba-asr" {
        match paths::read_alibaba_region().as_str() {
            "domestic" => ALIBABA_ASR_CN_KEYRING_USER,
            _ => ALIBABA_ASR_INTL_KEYRING_USER,
        }
    } else {
        GLM_ASR_KEYRING_USER
    }
}

/// Load the active online provider's API key from the keyring into the runtime cache.
pub(crate) fn reload_online_asr_key(app_handle: &tauri::AppHandle, state: &AppState) {
    use tauri_plugin_keyring::KeyringExt;
    let user = active_online_keyring_user();
    let key = app_handle
        .keyring()
        .get_password("speak-flow", user)
        .ok()
        .flatten()
        .unwrap_or_default();
    state.set_online_asr_api_key(&key);
}

fn online_status_payload(engine: &str, has_key: bool) -> serde_json::Value {
    let label = match engine {
        "alibaba-asr" => "Alibaba DashScope",
        _ => "GLM-ASR",
    };
    serde_json::json!({
        "status": if has_key { "ready" } else { "need_api_key" },
        "message": if has_key {
            format!("{} online service ready", label)
        } else {
            format!("Please configure the {} API Key", label)
        },
        "device": "cloud",
        "gpu_name": serde_json::Value::Null,
        "models_present": true,
        "missing_models": [],
    })
}

#[tauri::command]
pub async fn set_engine(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    engine: String,
) -> Result<String, AppError> {
    if !paths::is_supported_engine(&engine) {
        return Err(AppError::Other(format!(
            "Unsupported engine type: {}. Valid values: {}",
            engine,
            paths::local_asr_engines()
                .chain(paths::ONLINE_ASR_ENGINES.iter().copied())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let _asr_guard = lock_idle_asr(state.inner())?;
    if state.engine.download_task.lock().await.is_some() {
        return Err(AppError::Other(
            "Models are downloading; please wait for completion or cancel the download before switching engines".to_string(),
        ));
    }
    // The config file is replaced atomically; the old service is stopped only after the commit succeeds. This way a disk/permission error
    // cannot leave a still-valid old engine in a "config unchanged but process stopped" state.
    paths::write_engine_config(&engine)
        .map_err(|e| AppError::Other(format!("Failed to save engine configuration: {}", e)))?;
    state.engine.block_funasr_starting();
    let switch_result: Result<(), AppError> = async {
        funasr_service::stop_server(state.inner()).await?;

        // Online engine: after switching, reload the matching API key from the keyring, then refresh the ready state.
        if paths::is_online_engine(&engine) {
            reload_online_asr_key(&app_handle, state.inner());
            let has_key = !state.read_online_asr_api_key().is_empty();
            state.set_funasr_ready(has_key);
            let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
        }
        Ok(())
    }
    .await;
    state.engine.unblock_funasr_starting();
    switch_result?;

    log::info!("Engine switched to: {}", engine);
    Ok(engine)
}

#[tauri::command]
pub async fn get_gpu_idle_seconds(include_disabled: Option<bool>) -> Result<u64, AppError> {
    Ok(if include_disabled == Some(true) {
        paths::read_gpu_idle_timeout_seconds()
    } else {
        paths::read_gpu_idle_seconds()
    })
}

#[tauri::command]
pub async fn set_gpu_idle_seconds(
    state: tauri::State<'_, AppState>,
    seconds: u64,
    timeout_seconds: Option<u64>,
) -> Result<u64, AppError> {
    let requested_seconds = seconds.max(timeout_seconds.unwrap_or(0));
    if requested_seconds > paths::MAX_GPU_IDLE_SECONDS {
        return Err(AppError::Other(format!(
            "GPU idle timeout is too large: {}, maximum {} seconds",
            requested_seconds,
            paths::MAX_GPU_IDLE_SECONDS
        )));
    }
    paths::write_gpu_idle_seconds(seconds, timeout_seconds)
        .map_err(|e| AppError::Other(format!("Failed to save GPU idle setting: {}", e)))?;
    // Takes effect immediately for a running local engine; for an engine that is not started or an online engine it is only recorded in engine.json.
    funasr_service::push_gpu_idle_seconds(state.inner(), seconds).await?;
    Ok(seconds)
}

#[tauri::command]
pub async fn set_online_asr_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
    keyring_user: Option<String>,
) -> Result<(), AppError> {
    // The caller can specify the keyring slot explicitly to avoid a race between debounce and set_engine:
    // the user types in the GLM input, switches to Alibaba, and the debounced save lands after the switch.
    // At that point, computing the slot from the "currently active engine" would write the GLM key into Alibaba's entry. With the slot passed in from
    // the initiator, the slot at typing time stays locked.
    let active_user = active_online_keyring_user();
    let target_user: &str = match keyring_user.as_deref() {
        Some(user) if VALID_ONLINE_KEYRING_USERS.contains(&user) => user,
        _ => active_user,
    };

    // Serialize storage and cache publication with region/engine changes and
    // other saves. The captured target slot remains the caller's intent.
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    llm_provider::save_or_delete_api_key(&app_handle, target_user, &api_key)
        .map_err(AppError::Other)?;
    let current_active_user = active_online_keyring_user();

    // Update the runtime cache and ready state only when the target slot matches the current active slot,
    // otherwise an unrelated key would be written into state and leave state and the UI inconsistent.
    if target_user == current_active_user {
        state.set_online_asr_api_key(&api_key);
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            let has_key = !api_key.is_empty();
            state.set_funasr_ready(has_key);
            let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_online_asr_api_key(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    Ok(state.read_online_asr_api_key())
}

#[tauri::command]
pub async fn get_online_asr_endpoint() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn set_online_asr_endpoint(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    region: String,
) -> Result<serde_json::Value, AppError> {
    if region != "international" && region != "domestic" {
        return Err(AppError::Other(format!(
            "Unsupported region: {}. Valid values: international, domestic",
            region
        )));
    }
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    paths::write_online_asr_endpoint(&region)
        .map_err(|e| AppError::Other(format!("Failed to save endpoint configuration: {}", e)))?;

    // A region switch means a different API key source for Alibaba; reload the cache and refresh the ready state.
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        reload_online_asr_key(&app_handle, state.inner());
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
    }

    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn get_alibaba_asr_config() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({
        "region": paths::read_alibaba_region(),
        "url": paths::read_alibaba_endpoint(),
        "model": paths::read_alibaba_model(),
        // Fallback model list: lets users see a selectable set even without a key. Once a key is available the frontend calls
        // list_alibaba_asr_models to refresh it with the runtime-fetched result.
        "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
    }))
}

#[tauri::command]
pub async fn set_alibaba_asr_model(model: String) -> Result<serde_json::Value, AppError> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("Model ID cannot be empty".into()));
    }
    // Lightweight allowlist: model IDs may only contain ASCII letters, digits and `.-_`, so they cannot inject odd URLs / fields
    // into the JSON body. DashScope's model naming convention already falls within this range.
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(AppError::Other(format!(
            "Invalid model ID: {}. Only letters, numbers, and .-_ are allowed",
            trimmed
        )));
    }
    paths::write_alibaba_model(trimmed)
        .map_err(|e| AppError::Other(format!("Failed to save model configuration: {}", e)))?;
    Ok(serde_json::json!({
        "model": paths::read_alibaba_model(),
    }))
}

/// Call DashScope `/compatible-mode/v1/models` to fetch the models visible to the account and filter for
/// the ASR / Omni families that can do speech-to-text; on fetch failure fall back to the static allowlist.
///
/// Returns `{ models: [...], source: "live" | "fallback" }`. `source = "live"`
/// means the list came from a real API response; `"fallback"` means a network or auth failure, and the frontend can use it
/// to decide whether to notify the user.
#[tauri::command]
pub async fn list_alibaba_asr_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Option<Vec<ModelEntry>>,
    }
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: Option<String>,
    }

    let (api_key, base) = {
        let _configuration = state.engine.funasr_lifecycle_op.lock().await;
        use tauri_plugin_keyring::KeyringExt;
        let key_slot = if paths::read_alibaba_region() == "domestic" {
            ALIBABA_ASR_CN_KEYRING_USER
        } else {
            ALIBABA_ASR_INTL_KEYRING_USER
        };
        let api_key = app_handle
            .keyring()
            .get_password(llm_provider::KEYRING_SERVICE, key_slot)
            .ok()
            .flatten()
            .unwrap_or_default();
        (api_key, paths::read_alibaba_endpoint())
    };

    let fallback = || {
        serde_json::json!({
            "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
            "source": "fallback",
        })
    };

    if api_key.is_empty() {
        return Ok(fallback());
    }

    let url = format!("{}/compatible-mode/v1/models", base);
    let resp_result = state
        .http_client
        .get(&url)
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let Ok(resp) = resp_result else {
        log::warn!(
            "Failed to fetch DashScope model list (network): {:?}",
            resp_result.err()
        );
        return Ok(fallback());
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!(
            "DashScope /v1/models HTTP {} (response {} chars)",
            status,
            body.chars().count()
        );
        return Ok(fallback());
    }

    let parsed: ModelsResponse = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to parse DashScope /v1/models response: {}", e);
            return Ok(fallback());
        }
    };

    let mut ids: Vec<String> = parsed
        .data
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e.id)
        .filter(|id| paths::is_asr_capable_model_id(id))
        .collect();

    if ids.is_empty() {
        return Ok(fallback());
    }

    // Stable sort + dedupe, moving the default model qwen3-asr-flash to the front and ordering the rest lexicographically.
    ids.sort();
    ids.dedup();
    if let Some(pos) = ids.iter().position(|id| id == paths::ALIBABA_DEFAULT_MODEL) {
        let item = ids.remove(pos);
        ids.insert(0, item);
    }

    Ok(serde_json::json!({
        "models": ids,
        "source": "live",
    }))
}

#[tauri::command]
pub async fn get_models_dir() -> Result<serde_json::Value, AppError> {
    let effective = paths::strip_win_prefix(&paths::get_effective_models_dir());
    let is_custom = paths::read_models_dir().is_some();
    Ok(serde_json::json!({
        "path": effective,
        "is_custom": is_custom,
    }))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDirUpdateResult {
    message: String,
    runtime_warning: Option<String>,
}

fn models_dir_update_result(
    restore_default: bool,
    runtime_warning: Option<String>,
) -> ModelsDirUpdateResult {
    ModelsDirUpdateResult {
        message: if restore_default {
            "Restored default model directory".to_string()
        } else {
            "Model directory updated".to_string()
        },
        runtime_warning,
    }
}

#[tauri::command]
pub async fn pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, AppError> {
    let result = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .pick_folder()
        .await;
    Ok(result.map(|folder| paths::strip_win_prefix(folder.path())))
}

#[tauri::command]
pub async fn set_models_dir(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: Option<String>,
    migrate: bool,
) -> Result<ModelsDirUpdateResult, AppError> {
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let _asr_guard = lock_idle_asr(state.inner())?;
    if state.engine.download_task.lock().await.is_some() {
        return Err(AppError::Other(
            "Models are downloading; please wait for completion or cancel the download before changing directories".to_string(),
        ));
    }
    let old_dir = paths::get_effective_models_dir();
    let restore_default = path.as_deref().is_none_or(|value| value.trim().is_empty());
    let new_dir = match &path {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
        _ => paths::get_default_models_dir(),
    };

    // canonicalize before comparing to avoid Windows case/trailing-slash differences
    let canon_old = std::fs::canonicalize(&old_dir).unwrap_or_else(|_| old_dir.clone());
    // The target directory may not exist yet and canonicalize would fail; fall back to comparing the original paths
    let canon_new = std::fs::canonicalize(&new_dir).unwrap_or_else(|_| new_dir.clone());
    if canon_old == canon_new {
        if restore_default {
            paths::write_models_dir(None)
                .map_err(|e| AppError::Other(format!("Failed to save configuration: {}", e)))?;
            return Ok(models_dir_update_result(true, None));
        }
        return Ok(ModelsDirUpdateResult {
            message: "Path unchanged".to_string(),
            runtime_warning: None,
        });
    }

    // Make sure the target directory exists (in spawn_blocking so network drives do not block the async thread)
    let new_dir_clone = new_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&new_dir_clone))
        .await
        .map_err(|e| AppError::Other(format!("Failed to create directory: {}", e)))?
        .map_err(|e| AppError::Other(format!("Failed to create directory: {}", e)))?;

    let engine = paths::read_engine_config();
    let local_engine = !paths::is_online_engine(&engine);
    let prepared_sources = if migrate && old_dir.is_dir() {
        let _ = app_handle.emit(
            "models-migrate-status",
            serde_json::json!({ "status": "migrating", "message": "Copying model files..." }),
        );

        let old = old_dir.clone();
        let dest = new_dir.clone();
        let handle = app_handle.clone();
        tokio::task::spawn_blocking(move || migrate_model_dirs(&old, &dest, Some(&handle)))
            .await
            .map_err(|e| AppError::Other(format!("Migration task failed: {}", e)))?
            .map_err(|e| AppError::Other(format!("Migration failed: {}", e)))?
    } else {
        Vec::new()
    };

    // A failed prepare leaves only a safely overwritable target copy; the source and the old runtime stay unchanged.
    // The old startup is cancelled and the old service stopped only after the config commits atomically.
    let dir_str = paths::strip_win_prefix(&new_dir);
    paths::write_models_dir((!restore_default).then_some(dir_str.as_str()))
        .map_err(|e| AppError::Other(format!("Failed to save configuration: {}", e)))?;

    state.engine.block_funasr_starting();
    let update_result: Result<bool, AppError> = async {
        if local_engine {
            funasr_service::stop_server(state.inner()).await?;
        }
        let should_start = if local_engine {
            let model_check = funasr_service::check_model_files().await?;
            if !model_check.all_present {
                state.set_funasr_ready(false);
            }
            model_check.all_present
        } else {
            false
        };

        Ok(should_start)
    }
    .await;
    state.engine.unblock_funasr_starting();
    let mut runtime_warning = None;
    let should_start = match update_result {
        Ok(should_start) => should_start,
        Err(err) => {
            state.set_funasr_ready(false);
            runtime_warning = Some(format!(
                "Directory setting saved, but runtime update failed: {}",
                err
            ));
            false
        }
    };
    drop(lifecycle_guard);

    if should_start {
        if let Err(err) = funasr_service::start_server(&app_handle, state.inner()).await {
            state.set_funasr_ready(false);
            runtime_warning = Some(format!(
                "Directory setting saved, but service startup failed: {}",
                err
            ));
        } else if !prepared_sources.is_empty() {
            // Another directory switch may happen between start completing and cleanup. Re-enter the lifecycle
            // and confirm the current config still points at this target, so a source directory that was switched back into use is not deleted.
            let _cleanup_guard = state.engine.funasr_lifecycle_op.lock().await;
            let effective_dir = paths::get_effective_models_dir();
            let canonical_effective =
                std::fs::canonicalize(&effective_dir).unwrap_or(effective_dir.clone());
            let canonical_target = std::fs::canonicalize(&new_dir).unwrap_or(new_dir.clone());
            if canonical_effective == canonical_target {
                match tokio::task::spawn_blocking(move || {
                    cleanup_migrated_sources(&prepared_sources)
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        runtime_warning =
                            Some(format!("Model directory switched, but some old directories could not be cleaned up: {}", err));
                    }
                    Err(err) => {
                        runtime_warning =
                            Some(format!("Model directory switched, but the old directory cleanup task failed: {}", err));
                    }
                }
            } else {
                log::info!(
                    "Model directory changed again; skipping cleanup of the old source: current={}, expected={}",
                    canonical_effective.display(),
                    canonical_target.display()
                );
            }
        }
    }

    let result = models_dir_update_result(restore_default, runtime_warning);
    let _ = app_handle.emit(
        "models-migrate-status",
        serde_json::json!({
            "status": "completed",
            "message": result.runtime_warning.as_deref().unwrap_or(&result.message),
        }),
    );

    Ok(result)
}

/// Migrate models--* directories from src to dst
///
/// The prepare phase only copies and never renames/deletes the source directory. The caller must first commit the new config atomically
/// and confirm the new runtime works before doing best-effort cleanup of the returned source directories.
fn migrate_model_dirs(
    src: &std::path::Path,
    dst: &std::path::Path,
    handle: Option<&tauri::AppHandle>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let canonical_src = std::fs::canonicalize(src).map_err(|e| {
        format!(
            "Failed to resolve source model directory {}: {}",
            src.display(),
            e
        )
    })?;
    let canonical_dst = std::fs::canonicalize(dst).map_err(|e| {
        format!(
            "Failed to resolve target model directory {}: {}",
            dst.display(),
            e
        )
    })?;
    if canonical_dst.starts_with(&canonical_src) {
        return Err(
            "Target model directory cannot be inside the current model directory".to_string(),
        );
    }

    let entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| format!("Failed to read source directory: {}", e))?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("models--") && e.path().is_dir())
        .collect();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let total = entries.len();
    let mut copied_sources = Vec::with_capacity(total);

    // prepare phase: recursively fill in/overwrite source files even if the target already exists; the source is always kept.
    for (i, entry) in entries.iter().enumerate() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        copy_dir_recursive(&src_path, &dst_path)
            .map_err(|e| format!("Failed to copy {}: {}", name.to_string_lossy(), e))?;
        copied_sources.push(src_path);
        log::info!(
            "Migration prepare (copy, keep source): {}",
            name.to_string_lossy()
        );

        if let Some(handle) = handle {
            let _ = handle.emit(
                "models-migrate-status",
                serde_json::json!({
                    "status": "migrating",
                    "message": format!("Migrating {}/{}...", i + 1, total),
                    "progress": ((i + 1) as f64 / total as f64 * 100.0).round(),
                }),
            );
        }
    }

    Ok(copied_sources)
}

fn cleanup_migrated_sources(sources: &[std::path::PathBuf]) -> Result<(), String> {
    let mut failures = Vec::new();
    for source in sources {
        if let Err(e) = std::fs::remove_dir_all(source) {
            log::warn!(
                "Failed to clean up source directory (does not affect the migration result): {} — {}",
                source.display(),
                e
            );
            failures.push(format!("{} — {}", source.display(), e));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
            // The source is deleted after the switch, so the copy must be on disk first.
            std::fs::OpenOptions::new()
                .write(true)
                .open(&dst_path)?
                .sync_all()?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn inspect_gguf_model(
    path: String,
) -> Result<crate::services::custom_models::GgufInspection, AppError> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other(
            "GGUF file path cannot be empty".to_string(),
        ));
    }
    let pb = std::path::PathBuf::from(trimmed);
    tokio::task::spawn_blocking(move || crate::services::custom_models::inspect_gguf_file(&pb))
        .await
        .map_err(|e| AppError::Other(format!("Failed to inspect GGUF file: {e}")))
}

#[tauri::command]
pub async fn list_custom_models(
) -> Result<Vec<crate::services::custom_models::CustomModelEntry>, AppError> {
    tokio::task::spawn_blocking(crate::services::custom_models::load_custom_models)
        .await
        .map_err(|e| AppError::Other(format!("Failed to list custom models: {e}")))
}

#[tauri::command]
pub async fn add_custom_model(
    path: String,
    name: Option<String>,
    source: Option<String>,
    repo_id: Option<String>,
) -> Result<crate::services::custom_models::CustomModelEntry, AppError> {
    let pb = std::path::PathBuf::from(path.trim());
    let src = source.unwrap_or_else(|| "local_gguf".to_string());
    tokio::task::spawn_blocking(move || {
        crate::services::custom_models::register_custom_model(
            &pb,
            name.as_deref(),
            &src,
            repo_id.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::Other(format!("Failed to register custom model: {e}")))?
    .map_err(AppError::Other)
}

#[tauri::command]
pub async fn remove_custom_model(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<bool, AppError> {
    let trimmed = id.trim().to_string();
    let target_engine = if trimmed.starts_with("custom:") {
        trimmed.clone()
    } else {
        format!("custom:{trimmed}")
    };
    let removed = tokio::task::spawn_blocking(move || {
        crate::services::custom_models::remove_custom_model(&trimmed)
    })
    .await
    .map_err(|e| AppError::Other(format!("Failed to remove custom model: {e}")))?
    .map_err(AppError::Other)?;

    if removed && paths::read_engine_config() == target_engine {
        let fallback = "qwen3-asr-0.6b";
        let _ = paths::write_engine_config(fallback);
        let _ = funasr_service::stop_server(state.inner()).await;
        let _ = app_handle.emit("engine-changed", fallback);
    }
    Ok(removed)
}

#[tauri::command]
pub async fn scan_hf_cache_models(
) -> Result<Vec<crate::services::custom_models::DiscoveredHfModel>, AppError> {
    tokio::task::spawn_blocking(crate::services::custom_models::scan_hf_cache_models)
        .await
        .map_err(|e| AppError::Other(format!("Failed to scan HuggingFace cache: {e}")))
}

#[tauri::command]
pub async fn pick_gguf_file(window: tauri::WebviewWindow) -> Result<Option<String>, AppError> {
    let result = rfd::AsyncFileDialog::new()
        .set_title("Select GGUF Model File")
        .add_filter("GGUF Models", &["gguf"])
        .set_parent(&window)
        .pick_file()
        .await;
    Ok(result.map(|handle| handle.path().to_string_lossy().to_string()))
}

#[cfg(test)]
mod model_dir_migration_tests {
    use super::{cleanup_migrated_sources, migrate_model_dirs, models_dir_update_result};

    #[test]
    fn model_dir_result_distinguishes_committed_config_from_runtime_warning() {
        let result = models_dir_update_result(
            false,
            Some("Directory setting saved, but service startup failed".to_string()),
        );
        let value = serde_json::to_value(result).expect("serialize model directory result");

        assert_eq!(value["message"], "Model directory updated");
        assert_eq!(
            value["runtimeWarning"],
            "Directory setting saved, but service startup failed"
        );
    }

    #[test]
    fn cleanup_failure_is_reported_to_the_committed_result() {
        let missing = std::env::temp_dir().join(format!(
            "speak-flow-missing-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));

        let error = cleanup_migrated_sources(std::slice::from_ref(&missing))
            .expect_err("a cleanup failure must reach the structured result");

        assert!(error.contains(&missing.display().to_string()));
    }

    #[test]
    fn migration_prepare_keeps_source_until_config_commit() {
        let unique = format!(
            "speak-flow-model-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let source = root.join("source");
        let target = root.join("target");
        let model_source = source.join("models--org--model");
        std::fs::create_dir_all(&model_source).expect("create source model directory");
        std::fs::write(model_source.join("weights.bin"), b"weights").expect("write source model");
        std::fs::create_dir_all(&target).expect("create target directory");

        migrate_model_dirs(&source, &target, None).expect("prepare migration");

        assert!(
            model_source.join("weights.bin").is_file(),
            "prepare must keep source data until the new config is committed"
        );
        assert_eq!(
            std::fs::read(target.join("models--org--model").join("weights.bin"))
                .expect("read copied model"),
            b"weights"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migration_rejects_target_nested_inside_source() {
        let root = std::env::temp_dir().join(format!(
            "speak-flow-nested-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let source = root.join("source");
        let target = source.join("nested-target");
        let model_source = source.join("models--org--model");
        std::fs::create_dir_all(&model_source).expect("create source model directory");
        std::fs::write(model_source.join("weights.bin"), b"weights").expect("write source model");
        std::fs::create_dir_all(&target).expect("create nested target");

        let error = migrate_model_dirs(&source, &target, None)
            .expect_err("nested migration target must be rejected");

        assert!(error.contains("cannot be inside"));
        assert!(model_source.join("weights.bin").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
