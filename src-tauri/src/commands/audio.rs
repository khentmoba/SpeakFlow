use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::Emitter;

use crate::services::audio_service;
use crate::state::{
    AppState, PendingRecordingSession, RecordingMode, RecordingOutcomeKind, RecordingPhase,
    RecordingSession, RecordingSlot, RecordingSnapshot, RecordingTrigger,
};
use crate::utils::AppError;

pub(crate) const RECORDING_NOT_READY_ERROR: &str =
    "Speech recognition service is not ready yet; please wait for initialization to finish";
pub(crate) const RECORDING_ALREADY_ACTIVE_ERROR: &str = "A recording is already in progress";
pub(crate) const RECORDING_START_CANCELLED_ERROR: &str = "Recording startup was cancelled";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureStartErrorResolution {
    Cancelled,
    StartError,
    Stale,
}

fn resolve_capture_start_error(
    stop_requested: bool,
    owns_start: bool,
) -> CaptureStartErrorResolution {
    if stop_requested {
        CaptureStartErrorResolution::Cancelled
    } else if owns_start {
        CaptureStartErrorResolution::StartError
    } else {
        CaptureStartErrorResolution::Stale
    }
}

fn emit_recording_state(
    app_handle: &tauri::AppHandle,
    snapshot: &RecordingSnapshot,
    is_starting: bool,
    is_recording: bool,
    is_processing: bool,
    error: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "sessionId": snapshot.session_id,
        "revision": snapshot.revision,
        "phase": snapshot.phase,
        "isStarting": is_starting,
        "isRecording": is_recording,
        "isProcessing": is_processing,
        "mode": snapshot.mode,
    });
    if let Some(error) = error {
        payload["error"] = serde_json::json!(error);
    }
    let _ = app_handle.emit("recording-state", payload);
}

fn emit_start_error(app_handle: &tauri::AppHandle, snapshot: &RecordingSnapshot, error: &str) {
    audio_service::flush_pending_paste(app_handle);
    emit_recording_state(app_handle, snapshot, false, false, false, Some(error));
    let _ = app_handle.emit(
        "recording-outcome",
        serde_json::json!({
            "sessionId": snapshot.session_id,
            "revision": snapshot.revision,
            "phase": snapshot.phase,
            "outcome": RecordingOutcomeKind::StartError,
            "mode": snapshot.mode,
            "detail": error,
        }),
    );
}

pub(crate) async fn start_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
    trigger: RecordingTrigger,
    // 调用方（如 hotkey 路径）在 start 之前已 spawn 的选中文本抓取任务，
    // 会被存进最终创建的 RecordingSession，由 finalize/discard 路径负责回收。
    // 本函数如果在任何路径上提前失败，这个本地 Option 会自然 drop，JoinHandle 被 detach。
    edit_grab: Option<tokio::task::JoinHandle<Option<String>>>,
) -> Result<u64, AppError> {
    start_recording_with_start_owner(app_handle, state, trigger, edit_grab, None).await
}

async fn maybe_switch_engine_for_keyboard(app_handle: &tauri::AppHandle, state: &AppState) {
    let Some(locale) = crate::services::keyboard_layout::detect_active_keyboard_language() else {
        return;
    };
    let Some(target_engine) = state.with_profile(|p| {
        crate::services::keyboard_layout::match_engine_for_keyboard(p, Some(&locale))
    }) else {
        return;
    };
    let current = crate::utils::paths::read_engine_config();
    if target_engine == current || !crate::utils::paths::is_supported_engine(&target_engine) {
        return;
    }
    if !crate::utils::paths::is_online_engine(&target_engine)
        && !crate::services::funasr_service::inspect_model_files_for_engine(&target_engine)
            .all_present
    {
        log::debug!(
            "Keyboard layout rule matched {} -> {}, but model files are missing; keeping {}",
            locale,
            target_engine,
            current
        );
        return;
    }
    if state.engine.download_task.lock().await.is_some() || state.engine.is_funasr_starting() {
        return;
    }
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    if state.engine.native_asr_owner.try_lock().is_err() {
        return;
    }
    if let Err(err) = crate::utils::paths::write_engine_config(&target_engine) {
        log::warn!("Keyboard auto engine switch failed to write config: {err}");
        return;
    }
    state.engine.block_funasr_starting();
    let stop_ok = crate::services::funasr_service::stop_server(state)
        .await
        .is_ok();
    state.engine.unblock_funasr_starting();
    if !stop_ok {
        return;
    }
    if crate::utils::paths::is_online_engine(&target_engine) {
        crate::commands::funasr::reload_online_asr_key(app_handle, state);
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
    } else if let Err(err) = crate::services::funasr_service::start_server(app_handle, state).await
    {
        log::warn!("Keyboard auto engine switch failed starting {target_engine}: {err}");
        return;
    }
    let _ = app_handle.emit(
        "keyboard-engine-switched",
        serde_json::json!({
            "locale": locale,
            "engine": target_engine,
            "previousEngine": current,
        }),
    );
}

pub(crate) async fn start_recording_with_start_owner(
    app_handle: tauri::AppHandle,
    state: &AppState,
    trigger: RecordingTrigger,
    mut edit_grab: Option<tokio::task::JoinHandle<Option<String>>>,
    start_owner: Option<(Arc<crate::commands::hotkey::HotkeyStartOwner>, u64)>,
) -> Result<u64, AppError> {
    crate::services::external_correction_service::cancel();
    maybe_switch_engine_for_keyboard(&app_handle, state).await;
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    // Starting a recording and dispatching queued output share the same boundary.
    let paste_guard = state.recording.paste_dispatch.lock().await;
    if !state.is_funasr_ready() {
        return Err(AppError::Audio(RECORDING_NOT_READY_ERROR.into()));
    }

    // 在任何字幕窗口/异步任务启动前抓取目标应用。后续收尾可能延迟数秒，
    // 不能再读取届时的前台窗口来决定历史或截图策略。
    let foreground_app = crate::utils::foreground::get_foreground_app();
    let paste_target = crate::services::selection_service::foreground_window_id();

    let (session_id, show_gen, stop_flag, stop_notify, starting_snapshot) = {
        let mut guard = state.recording.recording.lock();
        if guard.is_some() {
            return Err(AppError::Audio(RECORDING_ALREADY_ACTIVE_ERROR.into()));
        }
        let session_id = state
            .recording
            .session_counter
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        // Bind while holding the recording slot lock. A concurrent release
        // either cancels this pending intent or obtains this exact session ID
        // and waits for installation before the guarded stop can run.
        if start_owner
            .as_ref()
            .is_some_and(|(owner, generation)| !owner.bind(*generation, session_id))
        {
            return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
        }
        // Reserve the generation before spawning any async window work. A
        // delayed show from this session can never advance the generation
        // after a newer session has already reserved its own.
        let show_gen = crate::commands::window::reserve_subtitle_show_generation(&app_handle);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_notify = Arc::new(tokio::sync::Notify::new());
        *guard = Some(RecordingSlot::Starting(PendingRecordingSession {
            session_id,
            subtitle_show_gen: show_gen,
            trigger,
            stop_flag: stop_flag.clone(),
            stop_notify: stop_notify.clone(),
        }));
        let slot_snapshot = guard
            .as_ref()
            .expect("starting slot was just installed")
            .snapshot(0);
        let snapshot = state
            .recording
            .transition_snapshot_while_recording_locked(
                session_id,
                slot_snapshot.phase,
                slot_snapshot.mode,
                None,
                None,
            )
            .expect("new recording session must own the latest snapshot");
        (session_id, show_gen, stop_flag, stop_notify, snapshot)
    };
    audio_service::stop_microphone_level_monitor(state);
    drop(paste_guard);
    drop(lifecycle_guard);

    emit_recording_state(&app_handle, &starting_snapshot, true, false, false, None);

    // Window creation/layout and the synchronous cpal startup happen in
    // parallel. We still join the show task before interpreting the capture
    // result so a start_error cannot schedule a hide against a pre-show gen.
    let show_task = {
        let app = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            crate::commands::window::show_subtitle_window_for_session(app, session_id, show_gen)
                .await
        })
    };

    let samples: Arc<parking_lot::Mutex<Vec<i16>>> =
        Arc::new(parking_lot::Mutex::new(Vec::with_capacity(16000 * 30)));
    let interim_cache: Arc<parking_lot::Mutex<Option<crate::state::InterimCache>>> =
        Arc::new(parking_lot::Mutex::new(None));

    let context_sources = crate::state::user_profile::ContextSources::default();
    let mut context_capture = context_sources.enabled().then(|| {
        let app = app_handle.clone();
        let sources = context_sources.clone();
        tokio::task::spawn_blocking(move || {
            crate::services::context_awareness::capture(&app, sources, paste_target)
        })
    });

    let (audio_behavior, labs, selected_device) = state.with_profile(|p| {
        (
            p.audio_behavior.clone(),
            p.labs.clone(),
            state.selected_input_device_name(),
        )
    });
    let _ = labs;
    audio_service::set_avoid_bluetooth_microphone(audio_behavior.avoid_bluetooth_microphone);
    audio_service::update_audio_behavior(audio_behavior.clone());
    let preroll = audio_service::stop_preroll_and_take();

    let capture_task = {
        let capture_stop = stop_flag.clone();
        let capture_samples = samples.clone();
        tokio::task::spawn_blocking(move || {
            audio_service::spawn_audio_capture_thread_with_preroll(
                capture_stop,
                capture_samples,
                selected_device,
                preroll,
            )
        })
    };

    match show_task.await {
        Ok(Ok(_)) => {}
        Ok(Err(err)) => log::warn!("显示字幕窗口失败（录音继续）: {}", err),
        Err(err) => log::warn!("字幕窗口显示任务异常结束（录音继续）: {}", err),
    }
    let capture_result = match capture_task.await {
        Ok(result) => result,
        Err(err) => Err(AppError::Audio(format!(
            "Recording startup task terminated unexpectedly: {}",
            err
        ))),
    };

    // Cancellation wins over a simultaneous capture error. This keeps a
    // quick tap from being presented as a microphone failure.
    if stop_flag.load(Ordering::Acquire) {
        if let Ok((audio_thread, actual_sample_rate)) = capture_result {
            audio_service::discard_recording(RecordingSession {
                context_capture: context_capture.take(),
                context_sources: context_sources.clone(),
                session_id,
                subtitle_show_gen: show_gen,
                trigger,
                stop_flag,
                stop_notify,
                samples,
                sample_rate: actual_sample_rate,
                audio_thread: Some(audio_thread),
                interim_task: None,
                native_recording: None,
                interim_cache,
                foreground_app: foreground_app.clone(),
                paste_target,
                edit_grab: edit_grab.take(),
            })
            .await;
        }
        // The ring was consumed by this attempt; arm it again so the next
        // dictation still has its pre-roll.
        audio_service::restart_idle_preroll_if_eligible(state);
        crate::commands::window::schedule_subtitle_hide(
            &app_handle,
            session_id,
            show_gen,
            trigger.mode(),
            0,
        );
        return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
    }

    let (audio_thread, actual_sample_rate) = match capture_result {
        Ok(result) => result,
        Err(error) => {
            let detail = error.to_string();
            let (resolution, outcome_snapshot) = {
                let mut guard = state.recording.recording.lock();
                let owns_start = matches!(
                    guard.as_ref(),
                    Some(RecordingSlot::Starting(p)) if p.session_id == session_id
                );
                let resolution =
                    resolve_capture_start_error(stop_flag.load(Ordering::Acquire), owns_start);
                let snapshot = if resolution == CaptureStartErrorResolution::StartError {
                    *guard = None;
                    state.recording.transition_snapshot_while_recording_locked(
                        session_id,
                        RecordingPhase::Outcome,
                        trigger.mode(),
                        Some(RecordingOutcomeKind::StartError),
                        Some(&detail),
                    )
                } else {
                    None
                };
                (resolution, snapshot)
            };
            // The ring was consumed by this attempt; arm it again on every exit,
            // including the cancellation return below.
            audio_service::restart_idle_preroll_if_eligible(state);
            if resolution == CaptureStartErrorResolution::Cancelled {
                crate::commands::window::schedule_subtitle_hide(
                    &app_handle,
                    session_id,
                    show_gen,
                    trigger.mode(),
                    0,
                );
                return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
            }
            if let Some(snapshot) = outcome_snapshot {
                emit_start_error(&app_handle, &snapshot, &detail);
                crate::commands::window::schedule_subtitle_hide(
                    &app_handle,
                    session_id,
                    show_gen,
                    trigger.mode(),
                    audio_service::RESULT_HIDE_DELAY_MS,
                );
            }
            return Err(error);
        }
    };

    let current_engine = crate::utils::paths::read_engine_config();
    let use_native_stream = current_engine == "confucius4-r2t2"
        || (crate::utils::paths::is_transcribe_cpp_engine(&current_engine)
            && state
                .engine
                .supports_native_streaming
                .load(Ordering::Acquire));
    let (interim_task, native_recording) = if use_native_stream {
        (
            None,
            Some(audio_service::spawn_native_recording(
                app_handle.clone(),
                session_id,
                samples.clone(),
                actual_sample_rate,
            )),
        )
    } else {
        (
            Some(audio_service::spawn_interim_loop(
                app_handle.clone(),
                session_id,
                stop_flag.clone(),
                stop_notify.clone(),
                samples.clone(),
                actual_sample_rate,
                interim_cache.clone(),
            )),
            None,
        )
    };

    audio_service::spawn_waveform_emitter(
        app_handle.clone(),
        session_id,
        stop_flag.clone(),
        samples.clone(),
        actual_sample_rate,
    );

    let mut session = Some(RecordingSession {
        context_capture: context_capture.take(),
        context_sources,
        session_id,
        subtitle_show_gen: show_gen,
        trigger,
        stop_flag,
        stop_notify,
        samples,
        sample_rate: actual_sample_rate,
        audio_thread: Some(audio_thread),
        interim_task,
        native_recording,
        interim_cache,
        foreground_app,
        paste_target,
        edit_grab: edit_grab.take(),
    });

    let (cancelled, recording_snapshot) = {
        let mut guard = state.recording.recording.lock();
        match guard.as_ref() {
            Some(RecordingSlot::Starting(p)) if p.session_id == session_id => {
                if let Some(s) = session.take() {
                    *guard = Some(RecordingSlot::Active(s));
                }
                let slot_snapshot = guard
                    .as_ref()
                    .expect("active slot was just installed")
                    .snapshot(0);
                let snapshot = state.recording.transition_snapshot_while_recording_locked(
                    session_id,
                    slot_snapshot.phase,
                    slot_snapshot.mode,
                    None,
                    None,
                );
                (None, snapshot)
            }
            _ => (session.take(), None),
        }
    };

    if let Some(s) = cancelled {
        s.stop_flag.store(true, Ordering::Relaxed);
        s.stop_notify.notify_waiters();
        audio_service::discard_recording(s).await;
        audio_service::restart_idle_preroll_if_eligible(state);
        crate::commands::window::schedule_subtitle_hide(
            &app_handle,
            session_id,
            show_gen,
            trigger.mode(),
            0,
        );
        return Err(AppError::Audio(RECORDING_START_CANCELLED_ERROR.into()));
    }

    if let Some(snapshot) = recording_snapshot {
        emit_recording_state(&app_handle, &snapshot, false, true, false, None);
    }

    crate::services::audio_service::duck_audio_with_config(&audio_behavior);

    if state.ui.sound_enabled.load(Ordering::Acquire) {
        match trigger.mode() {
            RecordingMode::Dictation => crate::utils::sound::play_start_sound(),
            RecordingMode::Assistant => crate::utils::sound::play_assistant_start_sound(),
        }
    }
    log::info!(
        "录音已开始 (session {}, {}Hz, mode={})",
        session_id,
        actual_sample_rate,
        trigger.mode().as_str()
    );
    Ok(session_id)
}

pub(crate) async fn stop_recording_inner(
    app_handle: tauri::AppHandle,
    state: &AppState,
    expected_session: Option<(u64, RecordingTrigger)>,
) -> Result<Option<u64>, AppError> {
    let adaptive_release_tail = state.with_profile(|p| p.audio_behavior.adaptive_release_tail);
    let (recording, transition) = {
        let mut guard = state.recording.recording.lock();
        if let Some((expected_session_id, expected_trigger)) = expected_session {
            let matches_expected = guard.as_ref().is_some_and(|slot| {
                slot.session_id() == expected_session_id && slot.trigger() == expected_trigger
            });
            if !matches_expected {
                return Ok(None);
            }
        }
        let recording = guard.take();
        if let Some(slot) = recording.as_ref() {
            match slot {
                RecordingSlot::Starting(pending) => {
                    pending.stop_flag.store(true, Ordering::Release);
                    pending.stop_notify.notify_waiters();
                }
                RecordingSlot::Active(session) => {
                    let defer_for_tail = adaptive_release_tail
                        && session.trigger.mode() == RecordingMode::Dictation
                        && session.native_recording.is_none();
                    if !defer_for_tail {
                        session.stop_flag.store(true, Ordering::Release);
                        session.stop_notify.notify_waiters();
                    }
                }
            }
        }
        let transition = match recording.as_ref() {
            Some(RecordingSlot::Starting(p)) => {
                state.recording.transition_snapshot_while_recording_locked(
                    p.session_id,
                    RecordingPhase::Idle,
                    p.trigger.mode(),
                    None,
                    None,
                )
            }
            Some(RecordingSlot::Active(session)) => {
                state.recording.transition_snapshot_while_recording_locked(
                    session.session_id,
                    RecordingPhase::Processing,
                    session.trigger.mode(),
                    None,
                    None,
                )
            }
            None => None,
        };
        (recording, transition)
    };

    audio_service::flush_pending_paste(&app_handle);

    let session = match recording {
        None => {
            // No session means no release tail, so nothing is still capturing.
            crate::services::audio_service::restore_audio();
            log::warn!("stop_recording 被调用但没有活跃的录音会话");
            return Ok(None);
        }
        Some(RecordingSlot::Starting(p)) => {
            // A cancelled start never opened the capture stream, so no tail runs.
            crate::services::audio_service::restore_audio();
            log::info!("录音启动阶段已取消 (session {})", p.session_id);
            if let Some(snapshot) = transition.as_ref() {
                emit_recording_state(&app_handle, snapshot, false, false, false, None);
            }
            crate::commands::window::schedule_subtitle_hide(
                &app_handle,
                p.session_id,
                p.subtitle_show_gen,
                p.trigger.mode(),
                0,
            );
            return Ok(Some(p.session_id));
        }
        Some(RecordingSlot::Active(s)) => s,
    };

    let session_id = session.session_id;
    if state.ui.sound_enabled.load(Ordering::Acquire) {
        match session.trigger.mode() {
            RecordingMode::Dictation => crate::utils::sound::play_stop_sound(),
            RecordingMode::Assistant => crate::utils::sound::play_assistant_stop_sound(),
        }
    }
    log::info!("正在停止录音 (session {})", session_id);
    if let Some(snapshot) = transition.as_ref() {
        emit_recording_state(&app_handle, snapshot, false, false, true, None);
    }

    tokio::spawn(async move {
        audio_service::finalize_recording(app_handle, session).await;
    });

    Ok(Some(session_id))
}

#[tauri::command]
pub async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u64, AppError> {
    start_recording_inner(
        app_handle,
        state.inner(),
        RecordingTrigger::DictationOriginal,
        None,
    )
    .await
}

#[tauri::command]
pub async fn stop_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let _ = stop_recording_inner(app_handle, state.inner(), None).await?;
    Ok(())
}

#[tauri::command]
pub fn get_recording_snapshot(state: tauri::State<'_, AppState>) -> Option<RecordingSnapshot> {
    state.recording.snapshot()
}

#[tauri::command]
pub async fn test_microphone(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let name = state.selected_input_device_name();
    tokio::task::spawn_blocking(move || audio_service::test_microphone_sync(name))
        .await
        .map_err(|e| AppError::Audio(format!("Microphone test task failed: {}", e)))?
}

#[tauri::command]
pub async fn list_input_devices(
    state: tauri::State<'_, AppState>,
) -> Result<audio_service::InputDeviceListPayload, AppError> {
    let name = state.selected_input_device_name();
    tokio::task::spawn_blocking(move || audio_service::list_input_devices_sync(name))
        .await
        .map_err(|e| AppError::Audio(format!("Device enumeration task failed: {}", e)))?
}

#[tauri::command]
pub async fn set_input_device(
    state: tauri::State<'_, AppState>,
    name: Option<String>,
) -> Result<(), AppError> {
    let previous = state.selected_input_device_name();
    state.set_selected_input_device_name(name);
    let current = state.selected_input_device_name();
    // Compare the stored (normalized) values, so a padded or empty name does
    // not trigger the rebuild this guard exists to skip.
    if previous != current {
        audio_service::stop_preroll();
    }
    audio_service::restart_idle_preroll_if_eligible(state.inner());
    Ok(())
}

#[tauri::command]
pub async fn start_microphone_level_monitor(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    audio_service::start_microphone_level_monitor(app_handle, state.inner())
}

#[tauri::command]
pub async fn stop_microphone_level_monitor(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    audio_service::stop_microphone_level_monitor(state.inner());
    Ok(())
}

#[tauri::command]
pub async fn set_sound_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    state.ui.sound_enabled.store(enabled, Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn set_input_method(
    state: tauri::State<'_, AppState>,
    method: String,
) -> Result<(), AppError> {
    // 仅允许这两个取值。clipboard.rs 的 paste_text_impl 把 "clipboard" 单独
    // 分支处理，其余值都走 SendInput，所以"任意 String"等于把所有未知值悄悄
    // 解释为 sendInput。这里在入口卡死，避免 UI 错位/typo 写入静默退化。
    match method.as_str() {
        "sendInput" | "clipboard" => {
            *state.ui.input_method.lock() = method;
            Ok(())
        }
        other => Err(AppError::Other(format!(
            "Unknown input method: {}. Valid values: sendInput, clipboard",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_error_after_stop_is_cancelled() {
        assert_eq!(
            resolve_capture_start_error(true, false),
            CaptureStartErrorResolution::Cancelled
        );
        assert_eq!(
            resolve_capture_start_error(true, true),
            CaptureStartErrorResolution::Cancelled
        );
    }

    #[test]
    fn owned_capture_error_is_presented_as_start_error() {
        assert_eq!(
            resolve_capture_start_error(false, true),
            CaptureStartErrorResolution::StartError
        );
    }

    #[test]
    fn superseded_capture_error_is_stale() {
        assert_eq!(
            resolve_capture_start_error(false, false),
            CaptureStartErrorResolution::Stale
        );
    }
}
