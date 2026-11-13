use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager};

use super::resample::ChunkedResampler;
use super::wav::encode_wav;
use super::{
    EDIT_GRAB_WAIT_MS, EMPTY_RESULT_HIDE_DELAY_MS, INTERIM_MAX_AUDIO_WINDOW_SEC,
    MIN_AUDIO_DURATION_SEC, PASTE_DELAY_MS, RESULT_HIDE_DELAY_MS, TARGET_SAMPLE_RATE,
};
use crate::services::{
    ai_polish_service, alibaba_asr_service, assistant_service, funasr_service, glm_asr_service,
    history_service,
};
use crate::state::user_profile::{ResolvedAppProfile, UserProfile};
use crate::state::{
    AppState, DictationOutputMode, RecordingMode, RecordingOutcomeKind, RecordingPhase,
    RecordingSession, RecordingSnapshot,
};
use crate::utils::foreground::ForegroundApp;
use crate::utils::paths;

const ASSISTANT_PIPELINE_TIMEOUT_SECS: u64 = 180;

fn can_reuse_interim(
    cache: &crate::state::InterimCache,
    final_count: usize,
    sample_rate: u32,
) -> bool {
    let max_window_samples = (sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
    final_count > 0
        && final_count <= max_window_samples
        && cache.sample_count == final_count
        && !cache.text.trim().is_empty()
}

// ---------- Final transcription + paste ----------

#[derive(Clone, Copy)]
enum EditGrabStatus {
    Ok,
    Timeout,
    Empty,
    Unsupported,
}

impl EditGrabStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Timeout => "timeout",
            Self::Empty => "empty",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionTiming {
    #[serde(skip_serializing_if = "Option::is_none")]
    asr_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polish_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_ms: Option<u64>,
}

#[derive(Clone)]
struct HistorySessionContext {
    enabled: bool,
    retention_days: u32,
    session_id: u64,
    mode: RecordingMode,
    workflow: String,
    duration_sec: f64,
    engine: String,
    app_process: Option<String>,
    app_window_title: Option<String>,
    app_rule_name: Option<String>,
    audio_file: Option<String>,
}

impl HistorySessionContext {
    #[allow(clippy::too_many_arguments)]
    async fn persist(
        &self,
        app_handle: &tauri::AppHandle,
        status: &str,
        text: &str,
        original_text: &str,
        source_text: Option<&str>,
        language: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        timing: Option<TranscriptionTiming>,
        error: Option<&str>,
    ) {
        if !self.enabled {
            return;
        }
        if let Err(error) = crate::commands::history::persist_history_insert(
            app_handle,
            history_service::HistoryDraft {
                session_id: self.session_id,
                mode: self.mode.as_str().to_string(),
                workflow: self.workflow.clone(),
                status: status.to_string(),
                text: text.to_string(),
                original_text: original_text.to_string(),
                source_text: source_text.map(str::to_string),
                duration_sec: Some(self.duration_sec),
                language: language.map(str::to_string),
                engine: self.engine.clone(),
                provider: provider.map(str::to_string),
                model: model.map(str::to_string),
                app_process: self.app_process.clone(),
                app_window_title: self.app_window_title.clone(),
                app_rule_name: self.app_rule_name.clone(),
                audio_file: self.audio_file.clone(),
                asr_ms: timing.and_then(|value| value.asr_ms),
                polish_ms: timing.and_then(|value| value.polish_ms),
                total_ms: timing.and_then(|value| value.total_ms),
                error: error.map(str::to_string),
                reprocessed_from_id: None,
            },
            self.retention_days,
        )
        .await
        {
            log::warn!("Failed to save transcription history: {error}");
        }
    }
}

async fn resolve_history_audio(
    task: Option<tokio::task::JoinHandle<Result<String, String>>>,
) -> Option<String> {
    match task {
        Some(task) => match task.await {
            Ok(Ok(file_name)) => Some(file_name),
            Ok(Err(error)) => {
                log::warn!("Failed to save history audio; continuing with text only: {error}");
                None
            }
            Err(error) => {
                log::warn!("History audio save task panicked; continuing with text only: {error}");
                None
            }
        },
        None => None,
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn screen_context_allowed(
    requested: bool,
    captured: Option<&ForegroundApp>,
    current: Option<&ForegroundApp>,
) -> bool {
    requested && captured.is_some() && captured == current
}

fn resolve_recording_app_profile(
    profile: &UserProfile,
    foreground_app: Option<&ForegroundApp>,
) -> ResolvedAppProfile {
    match foreground_app {
        Some(app) if !app.process_name.trim().is_empty() => {
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
        // When the process identity cannot be confirmed, we cannot tell whether the user configured privacy rules for that app.
        // Only features that capture or persist content are disabled; dictation and explicit assistant requests can continue.
        _ => ResolvedAppProfile {
            screen_context_enabled: Some(false),
            history_enabled: Some(false),
            ..Default::default()
        },
    }
}

/// Adaptive post-release speech tail: keeps capture open up to `1500ms` (with a
/// minimum `200ms` post-release grace) until `160ms` of quiet 20ms frames or
/// another recording begins, preventing clipped trailing syllables.
async fn wait_adaptive_release_tail(
    state: &AppState,
    stop_flag: &std::sync::atomic::AtomicBool,
    stop_notify: &tokio::sync::Notify,
    samples: &parking_lot::Mutex<Vec<i16>>,
    sample_rate: u32,
) {
    if stop_flag.load(Ordering::Acquire) {
        return;
    }
    let started = Instant::now();
    let min_tail = Duration::from_millis(200);
    let max_tail = Duration::from_millis(1500);
    let frame_len = ((sample_rate as usize) * 20 / 1000).max(160);
    let mut quiet_streak = 0usize;

    loop {
        if stop_flag.load(Ordering::Acquire) || state.recording.recording.lock().is_some() {
            break;
        }
        let elapsed = started.elapsed();
        if elapsed >= max_tail {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        if elapsed >= min_tail {
            let rms = {
                let guard = samples.lock();
                if guard.len() < frame_len {
                    0.0f32
                } else {
                    let slice = &guard[guard.len() - frame_len..];
                    let sum_sq: f32 = slice
                        .iter()
                        .map(|&s| {
                            let f = s as f32 / 32768.0;
                            f * f
                        })
                        .sum();
                    (sum_sq / slice.len() as f32).sqrt()
                }
            };
            if rms < 0.008 {
                quiet_streak += 1;
                if quiet_streak >= 8 {
                    break;
                }
            } else {
                quiet_streak = 0;
            }
        }
    }

    stop_flag.store(true, Ordering::Release);
    stop_notify.notify_waiters();
}

/// Compresses extended silence spans (`>= 650ms` below `0.008` RMS) down to
/// `180ms` when `labs.skip_pauses` is active.
pub(crate) fn compress_audio_pauses(samples: &[i16], sample_rate: u32) -> Vec<i16> {
    if samples.is_empty() || sample_rate == 0 {
        return samples.to_vec();
    }
    let frame_len = ((sample_rate as usize) * 20 / 1000).max(1);
    let min_silence_frames = 33usize; // ~660ms
    let keep_frames_each_side = 4usize; // 80ms + 80ms = 160ms padding
    if samples.len() <= frame_len * min_silence_frames {
        return samples.to_vec();
    }

    let mut frame_silent = Vec::with_capacity(samples.len() / frame_len + 1);
    for chunk in samples.chunks(frame_len) {
        let sum_sq: f32 = chunk
            .iter()
            .map(|&s| {
                let f = s as f32 / 32768.0;
                f * f
            })
            .sum();
        let rms = (sum_sq / chunk.len() as f32).sqrt();
        frame_silent.push(rms < 0.008);
    }

    let mut out = Vec::with_capacity(samples.len());
    let mut idx = 0usize;
    let total_frames = frame_silent.len();
    while idx < total_frames {
        if !frame_silent[idx] {
            let start = idx * frame_len;
            let end = ((idx + 1) * frame_len).min(samples.len());
            out.extend_from_slice(&samples[start..end]);
            idx += 1;
            continue;
        }
        let run_start = idx;
        while idx < total_frames && frame_silent[idx] {
            idx += 1;
        }
        let run_len = idx - run_start;
        if run_len >= min_silence_frames {
            let head_end = ((run_start + keep_frames_each_side) * frame_len).min(samples.len());
            let head_start = run_start * frame_len;
            out.extend_from_slice(&samples[head_start..head_end]);
            let tail_start = ((idx - keep_frames_each_side) * frame_len).min(samples.len());
            let tail_end = (idx * frame_len).min(samples.len());
            out.extend_from_slice(&samples[tail_start..tail_end]);
        } else {
            let start = run_start * frame_len;
            let end = (idx * frame_len).min(samples.len());
            out.extend_from_slice(&samples[start..end]);
        }
    }
    out
}

pub(crate) fn restart_idle_preroll_if_eligible(state: &AppState) {
    if state.recording.recording.lock().is_some() {
        return;
    }
    let (audio_behavior, labs, selected) = state.with_profile(|p| {
        (
            p.audio_behavior.clone(),
            p.labs.clone(),
            state.selected_input_device_name(),
        )
    });
    super::capture::start_preroll_if_eligible(selected, &audio_behavior, &labs, false);
}

/// How long finalize waits for the capture thread to stop before finishing without it.
const AUDIO_THREAD_JOIN_BUDGET: Duration = Duration::from_secs(5);

/// Waits a bounded time for the capture thread. Returns false when it is still running.
///
/// Finish is sent to the native stream only after this join, and the ASR lease stays
/// held until Finish arrives. An unbounded join would therefore queue every later
/// dictation behind a stuck capture device.
async fn join_capture_thread(handle: std::thread::JoinHandle<()>, budget: Duration) -> bool {
    let join = tokio::task::spawn_blocking(move || {
        let _ = handle.join();
    });
    tokio::time::timeout(budget, join).await.is_ok()
}

pub async fn finalize_recording(app_handle: tauri::AppHandle, session: RecordingSession) {
    let RecordingSession {
        context_capture,
        context_sources,
        session_id,
        subtitle_show_gen,
        trigger,
        stop_flag,
        stop_notify,
        sample_rate,
        audio_thread,
        interim_task,
        native_recording,
        samples,
        interim_cache,
        foreground_app,
        paste_target,
        edit_grab,
    } = session;
    let finalize_start = Instant::now();
    let state = app_handle.state::<AppState>();

    wait_adaptive_release_tail(&state, &stop_flag, &stop_notify, &samples, sample_rate).await;
    // Restore ducking only after the release tail, so the tail is captured
    // against the ducked background instead of the restored one.
    crate::services::audio_service::restore_audio();

    if let Some(h) = audio_thread {
        if !join_capture_thread(h, AUDIO_THREAD_JOIN_BUDGET).await {
            log::warn!(
                "Audio capture thread did not stop within {}s; finishing without waiting for it",
                AUDIO_THREAD_JOIN_BUDGET.as_secs()
            );
        }
    }
    let (reference_context, native_outcome, (), (edit_context, edit_grab_status)) = tokio::join!(
        async {
            let mut reference_context = match context_capture {
                Some(mut task) => {
                    match tokio::time::timeout(Duration::from_millis(300), &mut task).await {
                        Ok(Ok(context)) => context,
                        _ => {
                            task.abort();
                            crate::services::context_awareness::CapturedContext {
                                sources: context_sources.clone(),
                                ..Default::default()
                            }
                        }
                    }
                }
                None => crate::services::context_awareness::CapturedContext {
                    sources: context_sources,
                    ..Default::default()
                },
            };
            if state.recording.session_counter.load(Ordering::Acquire) == session_id {
                reference_context.refresh_clipboard(&app_handle);
            }
            reference_context
        },
        async {
            // The capture thread is joined: Finish now includes its final samples and
            // the resampler tail. A native result never falls back to whole-clip ASR.
            match native_recording {
                Some(recording) => {
                    let started = Instant::now();
                    let result = recording.finish().await.map_err(|error| error.to_string());
                    Some((result, elapsed_ms(started)))
                }
                None => None,
            }
        },
        async {
            // Wait for the interim task to finish on its own; on timeout, abort it to release the funasr_process lock
            if let Some(t) = interim_task {
                let abort_handle = t.abort_handle();
                if tokio::time::timeout(std::time::Duration::from_secs(5), t)
                    .await
                    .is_err()
                {
                    log::warn!("Interim task timed out (5s), aborting forcibly");
                    abort_handle.abort();
                }
            }
        },
        async {
            // The selected text stays in a local variable, not a global. Two concurrent finalizes are also isolated from each other:
            // edit_grab comes from each session's own RecordingSession and edit_context lives only in this function's
            // scope, so there is no chance of cross-session mixups.
            match edit_grab {
                Some(handle) => {
                    let abort_handle = handle.abort_handle();
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(EDIT_GRAB_WAIT_MS),
                        handle,
                    )
                    .await
                    {
                        Ok(Ok(Some(selected))) => (Some(selected), EditGrabStatus::Ok),
                        Ok(Ok(None)) => {
                            log::debug!(
                                "Selected text capture complete: no text currently selected"
                            );
                            (None, EditGrabStatus::Empty)
                        }
                        Ok(Err(join_err)) => {
                            log::debug!("Selected text capture task join failed: {}", join_err);
                            (None, EditGrabStatus::Unsupported)
                        }
                        Err(_) => {
                            log::warn!(
                                "Selected text capture exceeded {}ms; falling back to ordinary dictation (session {})",
                                EDIT_GRAB_WAIT_MS,
                                session_id
                            );
                            abort_handle.abort();
                            (None, EditGrabStatus::Timeout)
                        }
                    }
                }
                None => {
                    log::debug!("Current session has no selected text capture task");
                    (None, EditGrabStatus::Unsupported)
                }
            }
        },
    );
    let (native_result, native_finish_ms) = match native_outcome {
        Some((result, ms)) => (Some(result), Some(ms)),
        None => (None, None),
    };

    let app_profile = state
        .with_profile(|profile| resolve_recording_app_profile(profile, foreground_app.as_ref()));
    if foreground_app
        .as_ref()
        .is_none_or(|app| app.process_name.trim().is_empty())
    {
        log::warn!("Unable to verify target recording process; disabled screen context and history saving for this run");
    }
    let history_settings = state.with_profile(|profile| profile.history_settings.clone());
    let history_enabled = app_profile
        .history_enabled
        .unwrap_or(history_settings.enabled);
    let mut app_context = foreground_app.as_ref().and_then(|app| {
        crate::utils::foreground::prompt_context_from_parts(&app.process_name, &app.window_title)
    });
    let references =
        reference_context.prompt_sections(&crate::state::user_profile::ContextSources::default());
    if !references.is_empty() {
        app_context
            .get_or_insert_with(String::new)
            .push_str(&format!("\n{references}"));
    }
    let history_engine = paths::read_engine_config();

    let final_count = samples.lock().len();
    let cached = interim_cache.lock().clone();
    let duration_sec = final_count as f64 / sample_rate as f64;
    let mode = trigger.mode();

    if duration_sec < MIN_AUDIO_DURATION_SEC && native_result.is_none() {
        log::info!(
            "Recording too short ({:.2}s), skipping transcription",
            duration_sec
        );
        emit_terminal_outcome(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            RecordingOutcomeKind::TooShort,
            None,
        );
        flush_pending_paste(&app_handle);
        restart_idle_preroll_if_eligible(state.inner());
        return;
    }

    let history_audio_task = if history_enabled && history_settings.save_audio {
        match encode_wav(&samples.lock(), sample_rate) {
            Ok(wav) => Some(tokio::spawn(history_service::save_audio(session_id, wav))),
            Err(error) => {
                log::warn!("Failed to encode history audio; continuing with text only: {error}");
                None
            }
        }
    } else {
        None
    };

    let skip_pauses = state.with_profile(|p| p.labs.skip_pauses);
    if skip_pauses && native_result.is_none() {
        let compressed = {
            let guard = samples.lock();
            compress_audio_pauses(&guard, sample_rate)
        };
        if compressed.len() < final_count {
            log::debug!(
                "Skip pauses compressed audio {} -> {} samples",
                final_count,
                compressed.len()
            );
            *samples.lock() = compressed;
        }
    }

    let history_workflow = if mode == RecordingMode::Assistant {
        "assistant"
    } else if edit_context.is_some() {
        "edit"
    } else {
        "dictation"
    };
    let build_history_context = |audio_file: Option<String>| HistorySessionContext {
        enabled: history_enabled,
        retention_days: history_settings.retention_days,
        session_id,
        mode,
        workflow: history_workflow.to_string(),
        duration_sec,
        engine: history_engine.clone(),
        app_process: foreground_app.as_ref().map(|app| app.process_name.clone()),
        app_window_title: foreground_app.as_ref().map(|app| app.window_title.clone()),
        app_rule_name: app_profile.rule_name.clone(),
        audio_file,
    };

    // Prefer reusing the interim cache, otherwise run ASR again.
    //
    // Reuse conditions (reuse only if all hold, otherwise rerun do_final_asr):
    //   1. The recording must fall entirely inside the interim window (final_count <= 12s * sample_rate).
    //      The interim cache holds the transcript of the "last 12 seconds"; substituting it for a longer final would drop
    //      the text at the start of the recording (a ratio >0.9 drops it too, the user just perceives "the first few words are gone").
    //   2. The cache must cover every final sample; any trailing gap needs re-recognition.
    //      An unprocessed tail may contain speech, so silence cannot be inferred from the gap length alone.
    //   3. The interim result actually returned non-empty text.
    let asr_start = Instant::now();
    let (asr_text, detected_lang): (Result<String, String>, Option<String>) =
        if let Some(result) = native_result {
            match result {
                Ok(result) => (Ok(result.text), result.language),
                Err(error) => (Err(error), None),
            }
        } else {
            match cached {
                Some(ref c) if can_reuse_interim(c, final_count, sample_rate) => {
                    log::info!(
                        "Reusing interim cache (tail gap {:.0}ms)",
                        (final_count - c.sample_count) as f64 * 1000.0 / sample_rate as f64
                    );
                    // The cached text was corrected when the interim ran, so
                    // only the final-only step is applied here.
                    (
                        Ok(funasr_service::apply_filler_removal(
                            state.inner(),
                            &c.text,
                            c.language.as_deref(),
                        )),
                        c.language.clone(),
                    )
                }
                _ => match do_final_asr(&app_handle, state.inner(), &samples, sample_rate).await {
                    Ok(r) => (Ok(r.text), r.language),
                    Err(e) => (Err(e), None),
                },
            }
        };

    let asr_elapsed_ms = native_finish_ms.unwrap_or_else(|| elapsed_ms(asr_start));
    let text = match asr_text {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            let history = build_history_context(resolve_history_audio(history_audio_task).await);
            history
                .persist(
                    &app_handle,
                    "asr_error",
                    "",
                    "",
                    None,
                    None,
                    None,
                    None,
                    Some(TranscriptionTiming {
                        asr_ms: Some(asr_elapsed_ms),
                        polish_ms: None,
                        total_ms: Some(elapsed_ms(finalize_start)),
                    }),
                    Some(&e),
                )
                .await;
            emit_error(
                &app_handle,
                session_id,
                subtitle_show_gen,
                mode,
                RecordingOutcomeKind::AsrError,
                &e,
            );
            flush_pending_paste(&app_handle);
            return;
        }
    };

    let lang_ref = detected_lang.as_deref();

    if text.is_empty() {
        let history = build_history_context(resolve_history_audio(history_audio_task).await);
        history
            .persist(
                &app_handle,
                "no_speech",
                "",
                "",
                None,
                lang_ref,
                None,
                None,
                Some(TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: None,
                    total_ms: Some(elapsed_ms(finalize_start)),
                }),
                Some("No speech detected"),
            )
            .await;
        emit_terminal_outcome(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            RecordingOutcomeKind::NoSpeech,
            None,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    // Audio archiving runs in parallel with later processing; the final result is still published only after archiving is awaited and a history write is attempted.
    if mode == RecordingMode::Dictation && edit_context.is_some() {
        let selected_text = edit_context.unwrap_or_default();
        let edit_started = Instant::now();
        // Edit mode: the ASR result is a voice instruction used to rewrite the selected text
        log::info!(
            "Edit mode: instruction {} chars, selected text {} chars",
            text.chars().count(),
            selected_text.chars().count()
        );
        match ai_polish_service::edit_text(
            state.inner(),
            &selected_text,
            &text,
            &app_handle,
            session_id,
            &references,
        )
        .await
        {
            Ok(outcome) => {
                let result = outcome.text;
                let edit_elapsed_ms = elapsed_ms(edit_started);
                let history =
                    build_history_context(resolve_history_audio(history_audio_task).await);
                let timing = TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: Some(edit_elapsed_ms),
                    total_ms: Some(elapsed_ms(finalize_start)),
                };
                history
                    .persist(
                        &app_handle,
                        "success",
                        &result,
                        &text,
                        Some(&selected_text),
                        lang_ref,
                        Some(&outcome.provider),
                        Some(&outcome.model),
                        Some(timing),
                        None,
                    )
                    .await;
                emit_done(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    &result,
                    &result,
                    duration_sec,
                    true,
                    lang_ref,
                    edit_grab_status,
                );
                if !result.is_empty() {
                    let app = app_handle.clone();
                    let (output_behavior, labs) =
                        state.with_profile(|p| (p.output_behavior.clone(), p.labs.clone()));
                    let edit_paste_opts = super::output::PasteDeliveryOptions {
                        process_name: foreground_app.as_ref().map(|a| a.process_name.clone()),
                        paste_method_override: app_profile.paste_method,
                        copy_only: output_behavior.copy_only,
                        keep_on_clipboard: output_behavior.keep_on_clipboard,
                        paste_with_shift: output_behavior.paste_with_shift,
                        auto_terminal_ctrl_shift_v: output_behavior.auto_terminal_ctrl_shift_v,
                        return_to_app: labs.return_to_app,
                        smart_spacing: labs.smart_spacing,
                    };
                    tokio::spawn(async move {
                        do_paste(
                            &app,
                            &result,
                            session_id,
                            paste_target,
                            false,
                            edit_paste_opts,
                        )
                        .await;
                    });
                } else {
                    flush_pending_paste(&app_handle);
                }
            }
            Err(e) => {
                log::warn!("Failed to edit selected text; keeping original text: {}", e);
                let edit_elapsed_ms = elapsed_ms(edit_started);
                let history =
                    build_history_context(resolve_history_audio(history_audio_task).await);
                history
                    .persist(
                        &app_handle,
                        "processing_error",
                        "",
                        &text,
                        Some(&selected_text),
                        lang_ref,
                        None,
                        None,
                        Some(TranscriptionTiming {
                            asr_ms: Some(asr_elapsed_ms),
                            polish_ms: Some(edit_elapsed_ms),
                            total_ms: Some(elapsed_ms(finalize_start)),
                        }),
                        Some(&e),
                    )
                    .await;
                emit_error(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    RecordingOutcomeKind::ProcessingError,
                    &format!("Edit failed: {}", e),
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else if mode == RecordingMode::Assistant {
        let original_request = text;
        let assistant_started = Instant::now();
        let requested_polish_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let screen_context_explicit = app_profile.screen_context_enabled.is_some();
        let screen_routing_enabled = state.with_profile(|profile| profile.jev.screen_routing);
        let requested_polish_screen_context = if screen_routing_enabled && !screen_context_explicit
        {
            false
        } else {
            requested_polish_screen_context
        };
        let assistant_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let assistant_request_context = assistant_service::AssistantRequestContext::for_recording(
            assistant_screen_context,
            foreground_app.clone(),
            app_context.clone(),
        )
        .with_explicit_screen_context(screen_context_explicit);
        let (assistant_generation, cancel_rx) =
            crate::commands::assistant::begin_assistant_chat_task(state.inner());
        let assistant_pipeline = async {
            // Search decision, search keywords and final generation all use the corrected voice request.
            // Translation override is explicitly turned off so the "translated output" setting cannot change the instruction the user actually sent to the assistant.
            let polish_outcome = ai_polish_service::polish_text_with_overrides_detailed(
                state.inner(),
                &original_request,
                &app_handle,
                session_id,
                ai_polish_service::PolishOverrides {
                    ai_polish_enabled: app_profile.ai_polish_enabled,
                    translation_target: Some(None),
                    custom_prompt: app_profile.custom_prompt.clone(),
                    screen_context_enabled: Some(requested_polish_screen_context),
                    screen_context_explicit,
                    screen_context_foreground: foreground_app.clone(),
                    app_context: app_context.clone(),
                    ..Default::default()
                },
            )
            .await;
            let (assistant_request, screen_context_description) = match polish_outcome {
                Ok(outcome) => (outcome.text, outcome.screen_context_description),
                Err(err) => {
                    log::warn!(
                        "Assistant request pre-polish failed; using raw transcription: {}",
                        err
                    );
                    (original_request.clone(), None)
                }
            };
            let assistant_request_context = assistant_request_context
                .with_screen_context_description(screen_context_description);

            assistant_service::generate_content_with_context(
                state.inner(),
                &assistant_request,
                edit_context.as_deref(),
                &app_handle,
                session_id,
                assistant_request_context,
            )
            .await
        };
        let timeout_message = format!(
            "Assistant request timed out after {} seconds; please try again",
            ASSISTANT_PIPELINE_TIMEOUT_SECS
        );
        let assistant_result = crate::commands::assistant::run_cancellable_assistant_task(
            assistant_pipeline,
            cancel_rx,
            Duration::from_secs(ASSISTANT_PIPELINE_TIMEOUT_SECS),
            &timeout_message,
            "Assistant request was canceled",
        )
        .await;
        crate::commands::assistant::clear_assistant_chat_task(state.inner(), assistant_generation);
        let assistant_elapsed_ms = elapsed_ms(assistant_started);

        match assistant_result {
            Ok(outcome) => {
                let result = outcome.text;
                let history =
                    build_history_context(resolve_history_audio(history_audio_task).await);
                history
                    .persist(
                        &app_handle,
                        "success",
                        &result,
                        &original_request,
                        None,
                        lang_ref,
                        Some(&outcome.provider),
                        Some(&outcome.model),
                        Some(TranscriptionTiming {
                            asr_ms: Some(asr_elapsed_ms),
                            polish_ms: Some(assistant_elapsed_ms),
                            total_ms: Some(elapsed_ms(finalize_start)),
                        }),
                        None,
                    )
                    .await;
                emit_done(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    &result,
                    &original_request,
                    duration_sec,
                    false,
                    lang_ref,
                    edit_grab_status,
                );
                match crate::commands::window::set_subtitle_window_interactive_for_session(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    true,
                )
                .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        log::info!("Skipping subtitle window interactive toggle for expired assistant session (session {})", session_id)
                    }
                    Err(err) => log::warn!("Failed to toggle subtitle window interactive state when displaying assistant result: {}", err),
                }
            }
            Err(err) => {
                let history =
                    build_history_context(resolve_history_audio(history_audio_task).await);
                history
                    .persist(
                        &app_handle,
                        "processing_error",
                        "",
                        &original_request,
                        None,
                        lang_ref,
                        None,
                        None,
                        Some(TranscriptionTiming {
                            asr_ms: Some(asr_elapsed_ms),
                            polish_ms: Some(assistant_elapsed_ms),
                            total_ms: Some(elapsed_ms(finalize_start)),
                        }),
                        Some(&err.to_string()),
                    )
                    .await;
                emit_error(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    RecordingOutcomeKind::ProcessingError,
                    &err.to_string(),
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else {
        // Plain dictation mode
        let original = text.clone();
        let text = state.with_profile(|profile| {
            crate::services::external_correction_service::apply_spelling_rules(&text, profile)
        });
        let ai_polish_enabled = app_profile
            .ai_polish_enabled
            .unwrap_or_else(|| state.profile.ai_polish_enabled.load(Ordering::Acquire));
        let translation_override = app_profile
            .translation_target
            .clone()
            .or_else(|| match trigger.dictation_output() {
                DictationOutputMode::Original => Some(None),
                DictationOutputMode::Translated => None,
            });
        let (profile_translation_target, polish_audit_enabled) = state
            .with_profile(|profile| (profile.translation_target.clone(), profile.jev.polish_audit));
        let effective_translation_target = translation_override
            .clone()
            .unwrap_or(profile_translation_target);
        let audit_policy = if ai_polish_enabled
            && polish_audit_enabled
            && effective_translation_target.is_none()
        {
            Some(ai_polish_service::build_jev_audit_policy(
                state.inner(),
                &original,
                translation_override.clone(),
                app_profile.custom_prompt.as_deref(),
                app_context.as_deref(),
            ))
        } else {
            None
        };
        let requested_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let current_foreground = requested_screen_context
            .then(crate::utils::foreground::get_foreground_app)
            .flatten();
        let allow_screen_context = screen_context_allowed(
            requested_screen_context,
            foreground_app.as_ref(),
            current_foreground.as_ref(),
        );
        if requested_screen_context && !allow_screen_context {
            log::warn!(
                "AI polish screenshot skipped: foreground window left target application from start of recording (session {})",
                session_id
            );
        }
        let (output_behavior, labs, profile_transforms) = state.with_profile(|profile| {
            (
                profile.output_behavior.clone(),
                profile.labs.clone(),
                profile.global_transformations.clone(),
            )
        });
        let polish_start = Instant::now();
        let polish_result = ai_polish_service::polish_text_with_overrides_detailed(
            state.inner(),
            &text,
            &app_handle,
            session_id,
            ai_polish_service::PolishOverrides {
                ai_polish_enabled: Some(ai_polish_enabled),
                allow_jev_gate: references.is_empty(),
                translation_target: translation_override,
                custom_prompt: app_profile.custom_prompt.clone(),
                tone_mode_override: app_profile.tone_mode.clone(),
                app_vocabulary: app_profile.vocabulary.clone(),
                screen_context_enabled: Some(allow_screen_context),
                screen_context_explicit: app_profile.screen_context_enabled.is_some(),
                screen_context_foreground: foreground_app.clone(),
                app_context: app_context.clone(),
                ..Default::default()
            },
        )
        .await;
        let elapsed_polish_ms = elapsed_ms(polish_start);
        let (text, history_provider, history_model, polish_elapsed_ms) = match polish_result {
            Ok(outcome) => {
                let timing = outcome.executed.then_some(elapsed_polish_ms);
                (outcome.text, outcome.provider, outcome.model, timing)
            }
            Err(error) => {
                log::warn!(
                    "AI polish failed; retaining local correction result: {}",
                    error
                );
                (text.clone(), None, None, None)
            }
        };
        let polished = text != original;
        let text = crate::services::spoken_formatting::apply(
            &text,
            crate::services::spoken_formatting::SpokenFormattingOptions {
                spoken_formatting: labs.spoken_formatting,
                spoken_symbols: labs.spoken_symbols,
            },
        );
        let active_transforms: &[String] = if app_profile.transformations.is_empty() {
            &profile_transforms
        } else {
            &app_profile.transformations
        };
        let text = if active_transforms.is_empty() {
            text
        } else {
            let detected_domain = foreground_app.as_ref().and_then(|a| {
                crate::services::browser_domain::detect_active_browser_domain(
                    &a.process_name,
                    &a.window_title,
                )
            });
            let transform_ctx = crate::services::scripting_service::TransformContext {
                app_name: app_profile.rule_name.clone(),
                app_process: foreground_app.as_ref().map(|a| a.process_name.clone()),
                window_title: foreground_app.as_ref().map(|a| a.window_title.clone()),
                domain: detected_domain,
                mode_name: Some(mode.as_str().to_string()),
                tone: app_profile.tone_mode.clone(),
                raw_text: original.clone(),
                language: lang_ref.map(str::to_string),
            };
            crate::services::scripting_service::apply_transformations(
                &text,
                active_transforms,
                &transform_ctx,
            )
        };
        // Local tools run after polish so a saved snippet is never rewritten by
        // the model, and a spoken command is read from the final wording.
        let tools = crate::services::dictation_tools::OutputTools::load();
        let applied = crate::services::dictation_tools::apply(&text, lang_ref, &tools);
        if applied.command == Some(crate::services::voice_commands::VoiceCommand::Discard) {
            log::info!("Dictation discarded by spoken command (session {session_id})");
            emit_done_with_timing(
                &app_handle,
                session_id,
                subtitle_show_gen,
                mode,
                "",
                &original,
                duration_sec,
                false,
                lang_ref,
                edit_grab_status,
                None,
            );
            flush_pending_paste(&app_handle);
            restart_idle_preroll_if_eligible(state.inner());
            return;
        }
        let press_enter =
            applied.command == Some(crate::services::voice_commands::VoiceCommand::Enter);
        if let Some(command) = applied.command {
            log::info!(
                "Spoken command '{}' applied (session {session_id})",
                command.as_str()
            );
        }
        // Newline and Paragraph are delivered as text so they work in both the
        // clipboard and direct-input paths.
        let text = match applied.command {
            Some(command) => format!("{}{}", applied.text, command.insertion()),
            None => applied.text,
        };
        if polished {
            if let Some(policy) = audit_policy {
                crate::services::jev_review::schedule_polish_audit(
                    &app_handle,
                    state.inner(),
                    session_id,
                    &original,
                    &text,
                    policy,
                );
            }
        }
        let history = build_history_context(resolve_history_audio(history_audio_task).await);
        let timing = TranscriptionTiming {
            asr_ms: Some(asr_elapsed_ms),
            polish_ms: polish_elapsed_ms,
            total_ms: Some(elapsed_ms(finalize_start)),
        };
        history
            .persist(
                &app_handle,
                "success",
                &text,
                &original,
                None,
                lang_ref,
                history_provider.as_deref(),
                history_model.as_deref(),
                Some(timing),
                None,
            )
            .await;
        emit_done_with_timing(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            &text,
            &original,
            duration_sec,
            polished,
            lang_ref,
            edit_grab_status,
            Some(timing),
        );

        let paste_options = super::output::PasteDeliveryOptions {
            process_name: foreground_app.as_ref().map(|a| a.process_name.clone()),
            paste_method_override: app_profile.paste_method,
            copy_only: output_behavior.copy_only,
            keep_on_clipboard: output_behavior.keep_on_clipboard,
            paste_with_shift: output_behavior.paste_with_shift,
            auto_terminal_ctrl_shift_v: output_behavior.auto_terminal_ctrl_shift_v,
            return_to_app: labs.return_to_app,
            smart_spacing: labs.smart_spacing,
        };

        crate::commands::clipboard::set_last_dictation_record(
            crate::commands::clipboard::LastDictationRecord {
                session_id,
                text: text.clone(),
                original_text: original.clone(),
                target_window: paste_target,
                process_name: foreground_app.as_ref().map(|a| a.process_name.clone()),
                window_title: foreground_app.as_ref().map(|a| a.window_title.clone()),
            },
        );

        if !text.is_empty() {
            let app = app_handle.clone();
            tokio::spawn(async move {
                do_paste(
                    &app,
                    &text,
                    session_id,
                    paste_target,
                    press_enter,
                    paste_options,
                )
                .await;
            });
        } else if press_enter {
            // "press enter" on its own has no text to deliver, so the key press
            // is the whole output. Pasting an empty string would clear the
            // clipboard in clipboard mode.
            tokio::spawn(async move {
                if let Err(error) = crate::commands::clipboard::press_enter(paste_target).await {
                    log::warn!("Spoken enter command failed: {error}");
                }
            });
            flush_pending_paste(&app_handle);
        } else {
            flush_pending_paste(&app_handle);
        }
    }
    restart_idle_preroll_if_eligible(state.inner());
}

pub async fn discard_recording(session: RecordingSession) {
    if let Some(task) = session.context_capture {
        task.abort();
    }
    crate::services::audio_service::restore_audio();
    // Abort the grab handle held by this session (spawn_blocking is not preemptible, but abort detaches the
    // JoinHandle early so the result is discarded and does not affect later sessions).
    if let Some(grab) = session.edit_grab {
        grab.abort();
    }
    if let Some(h) = session.audio_thread {
        if !join_capture_thread(h, AUDIO_THREAD_JOIN_BUDGET).await {
            log::warn!(
                "Audio capture thread did not stop within {}s; discarding without waiting for it",
                AUDIO_THREAD_JOIN_BUDGET.as_secs()
            );
        }
    }
    if let Some(t) = session.interim_task {
        let abort_handle = t.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(5), t)
            .await
            .is_err()
        {
            abort_handle.abort();
        }
    }
    if let Some(recording) = session.native_recording {
        if let Err(error) = recording.cancel().await {
            log::warn!("Failed to cancel R2T2 recording: {error}");
        }
    }
    log::info!(
        "Discarded recording session (session {})",
        session.session_id
    );
}

async fn do_final_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    samples: &parking_lot::Mutex<Vec<i16>>,
    sample_rate: u32,
) -> Result<funasr_service::TranscriptionResult, String> {
    let data = std::mem::take(&mut *samples.lock());
    let (asr_audio, asr_sample_rate) = if sample_rate == TARGET_SAMPLE_RATE {
        (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
    } else {
        match ChunkedResampler::new(sample_rate) {
            Ok(mut resampler) => {
                let mut output = Vec::with_capacity(
                    ((data.len() as f64 * TARGET_SAMPLE_RATE as f64 / sample_rate as f64).ceil()
                        as usize)
                        + 8,
                );
                match resampler
                    .process_chunk(&data, &mut output)
                    .and_then(|_| resampler.finish(&mut output))
                {
                    Ok(()) => (std::borrow::Cow::Owned(output), TARGET_SAMPLE_RATE),
                    Err(err) => {
                        log::warn!(
                            "Final audio resampling failed; keeping raw sample rate {}Hz: {}",
                            sample_rate,
                            err
                        );
                        (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "Final audio resampling failed; keeping raw sample rate {}Hz: {}",
                    sample_rate,
                    err
                );
                (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
            }
        }
    };

    let engine = paths::read_engine_config();
    let result = if paths::is_online_engine(&engine) {
        let wav = encode_wav(&asr_audio, asr_sample_rate)
            .map_err(|e| format!("WAV encoding failed: {}", e))?;
        match engine.as_str() {
            "alibaba-asr" => alibaba_asr_service::transcribe(state, wav).await,
            _ => glm_asr_service::transcribe(state, wav).await,
        }
    } else {
        funasr_service::transcribe_pcm16(
            state,
            &asr_audio,
            asr_sample_rate,
            app_handle,
            funasr_service::TranscribeOptions { final_pass: true },
        )
        .await
    };

    match result {
        Ok(r) if r.success => Ok(r),
        Ok(r) => Err(r
            .error
            .unwrap_or_else(|| "Speech recognition failed".into())),
        Err(e) => Err(format!("Speech recognition failed: {}", e)),
    }
}

// ---------- Event emission ----------

#[allow(clippy::too_many_arguments)]
fn emit_done(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
    edit_grab_status: EditGrabStatus,
) {
    emit_done_with_timing(
        app,
        sid,
        show_gen,
        mode,
        text,
        original_text,
        dur,
        polished,
        language,
        edit_grab_status,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_done_with_timing(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
    edit_grab_status: EditGrabStatus,
    timing: Option<TranscriptionTiming>,
) {
    let delay = if text.is_empty() {
        EMPTY_RESULT_HIDE_DELAY_MS
    } else {
        RESULT_HIDE_DELAY_MS
    };
    let idle = app
        .state::<AppState>()
        .recording
        .transition_snapshot_if_current(sid, RecordingPhase::Idle, mode, None, None);
    if let Some(snapshot) = idle.as_ref() {
        app.state::<AppState>()
            .recording
            .clear_snapshot_if_session(sid);
        emit_recording_state_snapshot(app, snapshot, None);
    }
    emit_transcription_result(
        app,
        sid,
        mode,
        text,
        original_text,
        dur,
        polished,
        language,
        edit_grab_status,
        timing,
    );
    if mode != RecordingMode::Assistant || text.is_empty() {
        crate::commands::window::schedule_subtitle_hide(app, sid, show_gen, mode, delay);
    }
    flush_pending_paste(app);
}

#[allow(clippy::too_many_arguments)]
fn emit_transcription_result(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
    edit_grab_status: EditGrabStatus,
    timing: Option<TranscriptionTiming>,
) {
    let mut payload = serde_json::json!({
        "sessionId": sid, "text": text, "interim": false,
        "durationSec": dur, "charCount": text.chars().count(), "polished": polished,
        "language": language, "mode": mode.as_str(), "originalText": original_text,
        "editGrabStatus": edit_grab_status.as_str(),
    });
    if let Some(timing) = timing {
        payload["timing"] = serde_json::json!(timing);
    }
    crate::services::http_api_service::publish_stream_event("dictation.completed", payload.clone());
    let _ = app.emit("transcription-result", payload);
}

fn recording_outcome_payload(
    sid: u64,
    revision: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    detail: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "sessionId": sid,
        "revision": revision,
        "phase": RecordingPhase::Outcome,
        "outcome": outcome,
        "mode": mode.as_str(),
    });
    if let Some(detail) = detail {
        payload["detail"] = serde_json::json!(detail);
    }
    payload
}

fn emit_recording_outcome(app: &tauri::AppHandle, snapshot: &RecordingSnapshot) {
    let Some(outcome) = snapshot.outcome else {
        return;
    };
    let _ = app.emit(
        "recording-outcome",
        recording_outcome_payload(
            snapshot.session_id,
            snapshot.revision,
            snapshot.mode,
            outcome,
            snapshot.detail.as_deref(),
        ),
    );
}

fn emit_terminal_outcome(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    detail: Option<&str>,
) {
    flush_pending_paste(app);
    let snapshot = app
        .state::<AppState>()
        .recording
        .transition_snapshot_if_current(sid, RecordingPhase::Outcome, mode, Some(outcome), detail);
    if let Some(snapshot) = snapshot.as_ref() {
        emit_recording_state_snapshot(app, snapshot, detail);
        emit_recording_outcome(app, snapshot);
        crate::commands::window::schedule_subtitle_hide(
            app,
            sid,
            show_gen,
            mode,
            RESULT_HIDE_DELAY_MS,
        );
    }
}

fn emit_error(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    error: &str,
) {
    emit_terminal_outcome(app, sid, show_gen, mode, outcome, Some(error));
}

fn emit_recording_state_snapshot(
    app: &tauri::AppHandle,
    snapshot: &RecordingSnapshot,
    error: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "sessionId": snapshot.session_id,
        "revision": snapshot.revision,
        "phase": snapshot.phase,
        "isStarting": false,
        "isRecording": false,
        "isProcessing": snapshot.phase == RecordingPhase::Processing,
        "mode": snapshot.mode,
    });
    if let Some(err) = error {
        payload["error"] = serde_json::json!(err);
    }
    let _ = app.emit("recording-state", payload);
}

// ---------- Paste logic ----------

pub(crate) fn flush_pending_paste(app: &tauri::AppHandle) {
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
        let state = app.state::<AppState>();
        let recoveries = drain_pending_pastes(
            &state,
            crate::services::selection_service::foreground_window_id,
            |output| {
                let app = app.clone();
                let method = state.ui.input_method.lock().clone();
                async move {
                    let state = app.state::<AppState>();
                    let generation = crate::services::external_correction_service::cancel();
                    if output.options.return_to_app && !output.options.copy_only {
                        if let Some(target_hwnd) = output.target_window {
                            if crate::services::selection_service::foreground_window_id()
                                != Some(target_hwnd)
                            {
                                let _ = tokio::task::spawn_blocking(move || {
                                    crate::commands::clipboard::focus_window(target_hwnd)
                                })
                                .await;
                            }
                        }
                    }
                    let learn_corrections = state.with_profile(|p| p.external_correction_learning)
                        && !output.options.copy_only;
                    let smart_spacing = output.options.smart_spacing && !output.options.copy_only;
                    let target_win = output.target_window;
                    // Both reads inspect the focused field without writing to it, so they overlap.
                    let (before, prev_char) = tokio::join!(
                        async move {
                            if learn_corrections {
                                tokio::task::spawn_blocking(
                                    crate::services::external_correction_service::capture,
                                )
                                .await
                                .ok()
                                .flatten()
                            } else {
                                None
                            }
                        },
                        async move {
                            if smart_spacing {
                                tokio::task::spawn_blocking(move || {
                                    crate::commands::clipboard::grab_preceding_char_for_target(
                                        target_win,
                                    )
                                })
                                .await
                                .ok()
                                .flatten()
                            } else {
                                None
                            }
                        },
                    );
                    let delivered_text = if smart_spacing {
                        super::output::apply_smart_spacing(&output.text, prev_char)
                    } else {
                        output.text.clone()
                    };
                    let strategy = super::output::resolve_paste_strategy(&method, &output.options);
                    crate::commands::clipboard::paste_text_with_strategy(
                        &app,
                        &delivered_text,
                        strategy,
                        output.target_window,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                    if output.press_enter && !output.options.copy_only {
                        crate::commands::clipboard::press_enter(output.target_window)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                    if state.with_profile(|p| p.external_correction_learning)
                        && !output.options.copy_only
                    {
                        crate::services::external_correction_service::watch(
                            app,
                            output.target_window,
                            before,
                            delivered_text,
                            generation,
                        );
                    }
                    Ok(())
                }
            },
        )
        .await;
        for (output, reason) in recoveries {
            let _ = app.emit(
                "paste-recovery",
                serde_json::json!({
                    "sessionId": output.session_id,
                    "text": output.text,
                    "reason": reason,
                }),
            );
        }
    });
}

/// Recording reservation shares this dispatch mutex; native focus and input
/// remain sampled OS operations, and failures are returned for explicit recovery.
pub(super) async fn drain_pending_pastes<F, Fut>(
    state: &AppState,
    current_window: impl Fn() -> Option<isize>,
    paste: F,
) -> Vec<(super::output::PendingPaste, &'static str)>
where
    F: Fn(super::output::PendingPaste) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let _dispatch = state.recording.paste_dispatch.lock().await;
    let outputs = {
        let recording = state.recording.recording.lock();
        state
            .recording
            .pending_paste
            .lock()
            .take_if_idle(recording.is_some())
    };
    let mut recoveries = Vec::new();
    for output in outputs {
        let mut active_window = current_window();
        if !output.options.copy_only
            && output.options.return_to_app
            && !output.target_matches(active_window)
        {
            if let Some(target_hwnd) = output.target_window {
                let focused = tokio::task::spawn_blocking(move || {
                    crate::commands::clipboard::focus_window(target_hwnd)
                })
                .await
                .unwrap_or(false);
                if focused {
                    active_window = current_window();
                }
            }
        }
        let reason = if !output.options.copy_only && !output.target_matches(active_window) {
            Some(if output.target_window.is_none() {
                "target_unknown"
            } else {
                "target_changed"
            })
        } else if let Err(error) = paste(output.clone()).await {
            log::warn!(
                "Session {} automatic paste failed: {}",
                output.session_id,
                error
            );
            Some(
                if output.options.copy_only || output.target_matches(current_window()) {
                    "paste_failed"
                } else {
                    "target_changed"
                },
            )
        } else {
            None
        };
        if let Some(reason) = reason {
            recoveries.push((output, reason));
        }
    }
    recoveries
}

async fn do_paste(
    app: &tauri::AppHandle,
    text: &str,
    session_id: u64,
    target_window: Option<isize>,
    press_enter: bool,
    options: super::output::PasteDeliveryOptions,
) {
    crate::commands::clipboard::set_last_pasted_text(text);
    let state = app.state::<AppState>();
    {
        let _recording = state.recording.recording.lock();
        state
            .recording
            .pending_paste
            .lock()
            .push(super::output::PendingPaste {
                session_id,
                target_window,
                text: text.to_string(),
                press_enter,
                options,
            });
    }
    flush_pending_paste(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_thread_join_gives_up_on_a_thread_that_never_stops() {
        let (release, parked) = std::sync::mpsc::channel::<()>();
        let stuck = std::thread::spawn(move || {
            let _ = parked.recv();
        });
        let started = Instant::now();
        assert!(!join_capture_thread(stuck, Duration::from_millis(50)).await);
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(release);
    }

    #[tokio::test]
    async fn capture_thread_join_waits_for_a_thread_that_stops() {
        let stopped = std::thread::spawn(|| {});
        assert!(join_capture_thread(stopped, Duration::from_secs(5)).await);
    }

    #[test]
    fn polish_failure_keeps_local_spelling_corrections() {
        let source = include_str!("finalize.rs");
        let dictation = source.split("// Plain dictation mode").nth(1).unwrap();
        assert!(
            dictation.find("apply_spelling_rules").unwrap()
                < dictation
                    .find("polish_text_with_overrides_detailed")
                    .unwrap()
        );
        let fallback = dictation
            .split("Err(error) => {")
            .nth(1)
            .unwrap()
            .split("};")
            .next()
            .unwrap();
        assert!(fallback.contains("(text.clone(), None, None, None)"));
        assert!(!fallback.contains("original.clone()"));
    }

    #[test]
    fn interim_reuse_requires_every_final_sample() {
        let mut cache = crate::state::InterimCache {
            text: "unfinished sentence".into(),
            sample_count: 28_800,
            language: Some("zh".into()),
        };
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 31_999;
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 32_000;
        assert!(can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 32_001;
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 192_001;
        assert!(!can_reuse_interim(&cache, 192_001, 16_000));
        cache.sample_count = 0;
        assert!(!can_reuse_interim(&cache, 0, 16_000));
        cache.sample_count = 32_000;
        cache.text = " \n".into();
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
    }

    fn foreground(process_name: &str, window_title: &str) -> ForegroundApp {
        ForegroundApp {
            process_name: process_name.into(),
            window_title: window_title.into(),
        }
    }

    #[test]
    fn screen_context_requires_the_original_foreground_window() {
        let captured = foreground("Code.exe", "README - Code");
        let same = captured.clone();
        let switched = foreground("chrome.exe", "Inbox");

        assert!(screen_context_allowed(true, Some(&captured), Some(&same)));
        assert!(!screen_context_allowed(
            true,
            Some(&captured),
            Some(&switched)
        ));
        assert!(!screen_context_allowed(true, None, Some(&same)));
        assert!(!screen_context_allowed(false, Some(&captured), Some(&same)));
    }

    #[test]
    fn unidentified_foreground_disables_capture_and_history() {
        let profile = UserProfile::default();
        let missing_process = foreground("", "Protected document");

        for resolved in [
            resolve_recording_app_profile(&profile, None),
            resolve_recording_app_profile(&profile, Some(&missing_process)),
        ] {
            assert_eq!(resolved.screen_context_enabled, Some(false));
            assert_eq!(resolved.history_enabled, Some(false));
            assert_eq!(resolved.ai_polish_enabled, None);
        }
    }

    #[test]
    fn transcription_timing_serializes_as_frontend_camel_case_payload() {
        let value = serde_json::to_value(TranscriptionTiming {
            asr_ms: Some(42),
            polish_ms: Some(900),
            total_ms: Some(948),
        })
        .expect("timing should serialize");

        assert_eq!(
            value,
            serde_json::json!({
                "asrMs": 42,
                "polishMs": 900,
                "totalMs": 948
            })
        );
    }

    #[test]
    fn dictation_pastes_once_after_polish_completes() {
        let source = include_str!("finalize.rs");
        let dictation_branch = source
            .split("// Plain dictation mode")
            .nth(1)
            .and_then(|source| source.split("pub async fn discard_recording").next())
            .expect("normal dictation branch should exist");
        let polish_completed = dictation_branch
            .find("let polished = text != original;")
            .expect("dictation should resolve AI polish before pasting");
        let paste = dictation_branch
            .find("do_paste(")
            .expect("dictation should paste the final text");

        assert!(
            polish_completed < paste,
            "paste must happen after AI polish resolves"
        );
        assert_eq!(
            dictation_branch.matches("do_paste").count(),
            1,
            "normal dictation must have exactly one paste path"
        );
    }

    #[test]
    fn compress_audio_pauses_shortens_long_silence_spans() {
        let rate = 16_000u32;
        let speech = vec![4_000i16; 16_000 / 5]; // 200ms speech
        let long_silence = vec![0i16; 16_000]; // 1000ms silence
        let mut clip = Vec::new();
        clip.extend_from_slice(&speech);
        clip.extend_from_slice(&long_silence);
        clip.extend_from_slice(&speech);

        let compressed = compress_audio_pauses(&clip, rate);
        assert!(
            compressed.len() < clip.len(),
            "long silence should be shortened: {} vs {}",
            compressed.len(),
            clip.len()
        );
        // Should still preserve at least the speech + edge room tone padding.
        assert!(compressed.len() >= speech.len() * 2);
    }

    #[test]
    fn recording_outcome_payload_is_session_scoped_and_frontend_ready() {
        assert_eq!(
            recording_outcome_payload(
                42,
                7,
                RecordingMode::Dictation,
                RecordingOutcomeKind::TooShort,
                None,
            ),
            serde_json::json!({
                "sessionId": 42,
                "revision": 7,
                "phase": "outcome",
                "outcome": "too_short",
                "mode": "dictation",
            })
        );

        assert_eq!(
            recording_outcome_payload(
                43,
                8,
                RecordingMode::Assistant,
                RecordingOutcomeKind::ProcessingError,
                Some("provider failed"),
            ),
            serde_json::json!({
                "sessionId": 43,
                "revision": 8,
                "phase": "outcome",
                "outcome": "processing_error",
                "mode": "assistant",
                "detail": "provider failed",
            })
        );
    }
}
