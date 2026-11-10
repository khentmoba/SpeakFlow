use crate::commands::audio::{
    start_recording_with_start_owner, stop_recording_inner, RECORDING_ALREADY_ACTIVE_ERROR,
    RECORDING_NOT_READY_ERROR, RECORDING_START_CANCELLED_ERROR,
};
use crate::state::{AppState, RecordingSlot, RecordingTrigger};
use crate::utils::AppError;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, Manager};

use super::{
    emit_recording_error, hotkey_configuration_lock, hotkey_gate_is_current,
    is_tap_or_hold_enabled, is_toggle_mode, now_unix_ms, reset_hotkey_event_gate,
    update_hotkey_diagnostic_for_trigger, HotkeyEventGate, HOTKEY_REPRESS_DEBOUNCE_MS,
};

const TAP_OR_HOLD_THRESHOLD_MS: u64 = 300;

pub(super) fn is_ignorable_start_audio_error(message: &str) -> bool {
    message == RECORDING_NOT_READY_ERROR
        || message == RECORDING_ALREADY_ACTIVE_ERROR
        || message == RECORDING_START_CANCELLED_ERROR
}

pub(super) fn handle_hotkey_start(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
    gate: Arc<HotkeyEventGate>,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let grab_handle =
            tokio::task::spawn_blocking(crate::commands::clipboard::grab_selected_text);

        match start_recording_with_start_owner(
            app_handle.clone(),
            state.inner(),
            trigger,
            Some(grab_handle),
            Some((gate.start_owner.clone(), generation)),
        )
        .await
        {
            Ok(session_id) => {
                log::info!(
                    "热键 {} 触发录音开始 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Err(AppError::Audio(message)) if is_ignorable_start_audio_error(&message) => {
                let _transition = gate.transition.lock();
                if gate.start_owner.fail(generation) {
                    reset_hotkey_event_gate(&gate);
                }
                log::debug!("忽略热键 {} 的开始请求: {}", shortcut_label, message);
            }
            Err(AppError::Audio(message)) => {
                let _transition = gate.transition.lock();
                if gate.start_owner.fail(generation) {
                    reset_hotkey_event_gate(&gate);
                }
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
            }
            Err(err) => {
                let _transition = gate.transition.lock();
                if gate.start_owner.fail(generation) {
                    reset_hotkey_event_gate(&gate);
                }
                let message = err.to_string();
                log::warn!("热键 {} 开始录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

pub(super) fn handle_hotkey_stop(
    app_handle: tauri::AppHandle,
    shortcut_label: String,
    trigger: RecordingTrigger,
    expected_session_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        match stop_recording_inner(
            app_handle.clone(),
            state.inner(),
            Some((expected_session_id, trigger)),
        )
        .await
        {
            Ok(Some(session_id)) => {
                log::info!(
                    "热键 {} 触发录音停止 (session {}, mode={})",
                    shortcut_label,
                    session_id,
                    trigger.mode().as_str()
                );
            }
            Ok(None) => {
                log::debug!("忽略热键 {} 的停止请求：当前没有活跃录音", shortcut_label);
            }
            Err(err) => {
                let message = err.to_string();
                log::warn!("热键 {} 停止录音失败: {}", shortcut_label, message);
                update_hotkey_diagnostic_for_trigger(&app_handle, trigger, |diagnostic| {
                    let now_ms = now_unix_ms();
                    diagnostic.last_error = Some(message.clone());
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                emit_recording_error(&app_handle, &message);
            }
        }
    });
}

pub(super) fn handle_terminal_paste_action(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = crate::commands::clipboard::paste_last_as_terminal(app_handle).await {
            log::warn!("Terminal paste hotkey failed: {err}");
        }
    });
}

pub(super) fn handle_typed_text_action(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let app_for_grab = app_handle.clone();
        let selected = tokio::task::spawn_blocking(move || {
            crate::commands::clipboard::grab_selected_text_robust(&app_for_grab)
        })
        .await
        .ok()
        .flatten();

        let state = app_handle.state::<AppState>();
        if let Some(selected_text) = selected {
            let fg = crate::utils::foreground::get_foreground_app();
            let target_hwnd = crate::services::selection_service::foreground_window_id();
            let resolved = state.with_profile(|profile| match fg.as_ref() {
                Some(app) => {
                    let domain = crate::services::browser_domain::detect_active_browser_domain(
                        &app.process_name,
                        &app.window_title,
                    );
                    profile.resolve_app_profile_with_domain(
                        &app.process_name,
                        &app.window_title,
                        domain.as_deref(),
                    )
                }
                None => Default::default(),
            });
            let active_transforms: Vec<String> = if resolved.transformations.is_empty() {
                state.with_profile(|p| p.global_transformations.clone())
            } else {
                resolved.transformations.clone()
            };
            let (output_behavior, labs) =
                state.with_profile(|p| (p.output_behavior.clone(), p.labs.clone()));
            let formatted = crate::services::spoken_formatting::apply(
                &selected_text,
                crate::services::spoken_formatting::SpokenFormattingOptions {
                    spoken_formatting: labs.spoken_formatting,
                    spoken_symbols: labs.spoken_symbols,
                },
            );
            let transformed = if active_transforms.is_empty() {
                formatted
            } else {
                let ctx = crate::services::scripting_service::TransformContext {
                    app_name: resolved.rule_name.clone(),
                    app_process: fg.as_ref().map(|a| a.process_name.clone()),
                    window_title: fg.as_ref().map(|a| a.window_title.clone()),
                    domain: fg.as_ref().and_then(|a| {
                        crate::services::browser_domain::detect_active_browser_domain(
                            &a.process_name,
                            &a.window_title,
                        )
                    }),
                    mode_name: Some("typed_text".to_string()),
                    tone: resolved.tone_mode.clone(),
                    raw_text: selected_text.clone(),
                    language: None,
                };
                crate::services::scripting_service::apply_transformations(
                    &formatted,
                    &active_transforms,
                    &ctx,
                )
            };
            let final_text = if resolved.ai_polish_enabled.unwrap_or(false)
                || resolved.custom_prompt.is_some()
                || resolved.tone_mode.is_some()
            {
                match crate::services::ai_polish_service::polish_text_with_overrides_detailed(
                    state.inner(),
                    &transformed,
                    &app_handle,
                    0,
                    crate::services::ai_polish_service::PolishOverrides {
                        ai_polish_enabled: Some(true),
                        custom_prompt: resolved.custom_prompt.clone(),
                        tone_mode_override: resolved.tone_mode.clone(),
                        app_vocabulary: resolved.vocabulary.clone(),
                        emit_status: false,
                        learn_from_result: false,
                        allow_jev_gate: false,
                        ..Default::default()
                    },
                )
                .await
                {
                    Ok(out) => out.text,
                    Err(_) => transformed,
                }
            } else {
                transformed
            };
            let opts = crate::services::audio_service::output::PasteDeliveryOptions {
                process_name: fg.as_ref().map(|a| a.process_name.clone()),
                paste_method_override: resolved.paste_method,
                copy_only: output_behavior.copy_only,
                keep_on_clipboard: output_behavior.keep_on_clipboard,
                paste_with_shift: output_behavior.paste_with_shift,
                auto_terminal_ctrl_shift_v: output_behavior.auto_terminal_ctrl_shift_v,
                return_to_app: labs.return_to_app,
                smart_spacing: false,
            };
            let method = state.ui.input_method.lock().clone();
            let strategy =
                crate::services::audio_service::output::resolve_paste_strategy(&method, &opts);
            let _ = crate::commands::clipboard::paste_text_with_strategy(
                &app_handle,
                &final_text,
                strategy,
                target_hwnd,
            )
            .await;
        } else {
            let _ = app_handle.emit("open-typed-text", serde_json::json!({}));
        }
    });
}

pub(super) fn handle_meeting_toggle_action(app_handle: tauri::AppHandle) {
    let _ = app_handle.emit("meeting-hotkey-toggle", serde_json::json!({}));
}

// ---------------------------------------------------------------------------
// Dispatch — supports hold, toggle, and tap-or-hold modes
// ---------------------------------------------------------------------------

pub(super) fn dispatch_hotkey_press(
    app_handle: &tauri::AppHandle,
    gate: &Arc<HotkeyEventGate>,
    trigger: RecordingTrigger,
    pressed_log: &str,
    shortcut_label: &str,
) {
    let _configuration = hotkey_configuration_lock().lock();
    let _transition = gate.transition.lock();
    if !gate.registered.load(Ordering::Acquire) || !hotkey_gate_is_current(trigger, gate) {
        return;
    }
    let active_trigger = app_handle
        .state::<AppState>()
        .recording
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::trigger);
    let allow_toggle_stop = (is_toggle_mode() || is_tap_or_hold_enabled())
        && (gate.toggle_active.load(Ordering::Acquire) || gate.tap_locked.load(Ordering::Acquire))
        && active_trigger == Some(trigger);

    if active_trigger.is_some() && !allow_toggle_stop {
        log::debug!(
            "忽略热键 {} 的按下：已有录音进行中 (active trigger={:?}, request trigger={:?})",
            shortcut_label,
            active_trigger,
            trigger
        );
        return;
    }

    if is_toggle_mode() {
        let was_active = gate.toggle_active.load(Ordering::Acquire);
        if was_active {
            gate.toggle_active.store(false, Ordering::Release);
            gate.is_pressed.store(false, Ordering::Release);
            let now_ms = now_unix_ms();
            gate.last_release_ms.store(now_ms, Ordering::Release);
            log::info!("切换模式：再次按下 {}，停止录音", shortcut_label);
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = false;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("released".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_released_at_ms = Some(now_ms);
            });
            if let Some(session_id) = gate.start_owner.release() {
                handle_hotkey_stop(
                    app_handle.clone(),
                    shortcut_label.to_string(),
                    trigger,
                    session_id,
                );
            }
        } else {
            let now_ms = now_unix_ms();
            let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);
            if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
                return;
            }
            gate.toggle_active.store(true, Ordering::Release);
            gate.is_pressed.store(true, Ordering::Release);
            log::info!("切换模式：{}", pressed_log);
            update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
                diagnostic.is_pressed = true;
                diagnostic.last_error = None;
                diagnostic.last_event = Some("pressed".to_string());
                diagnostic.last_event_at_ms = Some(now_ms);
                diagnostic.last_pressed_at_ms = Some(now_ms);
            });
            let generation = gate.start_owner.begin();
            handle_hotkey_start(
                app_handle.clone(),
                shortcut_label.to_string(),
                trigger,
                gate.clone(),
                generation,
            );
        }
        return;
    }

    // Tap-or-hold second tap stops an active tap-locked recording
    if is_tap_or_hold_enabled() && gate.tap_locked.swap(false, Ordering::AcqRel) {
        gate.is_pressed.store(false, Ordering::Release);
        let now_ms = now_unix_ms();
        gate.last_release_ms.store(now_ms, Ordering::Release);
        log::info!(
            "Tap-or-hold: second tap on {} stops recording",
            shortcut_label
        );
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("released".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_released_at_ms = Some(now_ms);
        });
        if let Some(session_id) = gate.start_owner.release() {
            handle_hotkey_stop(
                app_handle.clone(),
                shortcut_label.to_string(),
                trigger,
                session_id,
            );
        }
        return;
    }

    // Hold (or initial tap-or-hold press)
    let now_ms = now_unix_ms();
    let last_release_ms = gate.last_release_ms.load(Ordering::Acquire);
    if now_ms.saturating_sub(last_release_ms) < HOTKEY_REPRESS_DEBOUNCE_MS {
        log::debug!(
            "忽略热键 {} 的按下抖动（距离上次松开 {}ms）",
            shortcut_label,
            now_ms.saturating_sub(last_release_ms)
        );
        return;
    }

    if gate
        .is_pressed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        gate.pressed_at_ms.store(now_ms, Ordering::Release);
        log::info!("{}", pressed_log);
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = true;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("pressed".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_pressed_at_ms = Some(now_ms);
        });
        let generation = gate.start_owner.begin();
        handle_hotkey_start(
            app_handle.clone(),
            shortcut_label.to_string(),
            trigger,
            gate.clone(),
            generation,
        );
    }
}

pub(super) fn dispatch_hotkey_release(
    app_handle: &tauri::AppHandle,
    gate: &Arc<HotkeyEventGate>,
    trigger: RecordingTrigger,
    released_log: &str,
    shortcut_label: &str,
) {
    let _configuration = hotkey_configuration_lock().lock();
    let _transition = gate.transition.lock();
    if is_toggle_mode() {
        return;
    }

    if gate.is_pressed.swap(false, Ordering::AcqRel) {
        let now_ms = now_unix_ms();
        let pressed_at = gate.pressed_at_ms.load(Ordering::Acquire);
        if is_tap_or_hold_enabled()
            && pressed_at > 0
            && now_ms.saturating_sub(pressed_at) < TAP_OR_HOLD_THRESHOLD_MS
        {
            gate.tap_locked.store(true, Ordering::Release);
            log::info!(
                "Tap-or-hold: quick tap ({}ms) on {} keeps recording active until next tap",
                now_ms.saturating_sub(pressed_at),
                shortcut_label
            );
            return;
        }

        gate.last_release_ms.store(now_ms, Ordering::Release);
        log::info!("{}", released_log);
        update_hotkey_diagnostic_for_trigger(app_handle, trigger, |diagnostic| {
            diagnostic.is_pressed = false;
            diagnostic.last_error = None;
            diagnostic.last_event = Some("released".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
            diagnostic.last_released_at_ms = Some(now_ms);
        });
        if let Some(session_id) = gate.start_owner.release() {
            handle_hotkey_stop(
                app_handle.clone(),
                shortcut_label.to_string(),
                trigger,
                session_id,
            );
        }
    }
}
