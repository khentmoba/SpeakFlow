use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use tauri::Emitter;

use super::capture::{
    load_best_input_config, mix_to_mono_capped_f32, mix_to_mono_capped_i16, mix_to_mono_capped_u16,
    resolve_input_device,
};
use super::resample::{f32_to_i16, u16_to_i16};
use super::MICROPHONE_LEVEL_EMIT_INTERVAL_MS;
use crate::state::{AppState, MicrophoneLevelMonitor};
use crate::utils::AppError;

// ---------- peak 计算 ----------

fn mono_peak_i16(data: &[i16], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|s| s.unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| {
                let s: i32 = f.iter().map(|&v| v as i32).sum();
                (s / ch as i32).unsigned_abs()
            })
            .max()
            .unwrap_or(0)
    }
}
fn mono_peak_f32(data: &[f32], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|&s| f32_to_i16(s).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| f32_to_i16(f.iter().sum::<f32>() / ch as f32).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    }
}
fn mono_peak_u16(data: &[u16], ch: usize) -> u32 {
    if ch <= 1 {
        data.iter()
            .map(|&s| u16_to_i16(s).unsigned_abs() as u32)
            .max()
            .unwrap_or(0)
    } else {
        data.chunks_exact(ch)
            .map(|f| {
                let s: u64 = f.iter().map(|&v| v as u64).sum();
                u16_to_i16((s / ch as u64) as u16).unsigned_abs() as u32
            })
            .max()
            .unwrap_or(0)
    }
}

fn peak_to_meter(peak: u32) -> u32 {
    ((peak.min(32767) as f32 / 32767.0) * 1000.0).round() as u32
}

fn monitor_start_is_current(
    current_generation: u64,
    requested_generation: u64,
    recording_active: bool,
) -> bool {
    current_generation == requested_generation && !recording_active
}

// ---------- 麦克风测试 / 预览 ----------

pub fn stop_microphone_level_monitor(state: &AppState) {
    let old_monitor = {
        let mut monitor = state.recording.microphone_level_monitor.lock();
        state
            .recording
            .microphone_level_monitor_generation
            .fetch_add(1, Ordering::AcqRel);
        monitor.take()
    };
    if let Some(mut m) = old_monitor {
        m.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = m.handle.take() {
            let _ = h.join();
        }
    }
}

pub fn start_microphone_level_monitor(
    app_handle: tauri::AppHandle,
    state: &AppState,
) -> Result<String, AppError> {
    use cpal::traits::StreamTrait;

    stop_microphone_level_monitor(state);

    // Reserve and publish under the same lock order as recording startup. Stop
    // invalidates reservations even when the worker has not published yet.
    let requested_generation = {
        let recording = state.recording.recording.lock();
        let _monitor = state.recording.microphone_level_monitor.lock();
        if recording.is_some() {
            return Err(AppError::Audio(
                "Cannot start microphone preview during recording".into(),
            ));
        }
        state
            .recording
            .microphone_level_monitor_generation
            .fetch_add(1, Ordering::AcqRel)
            + 1
    };

    let (device, device_name) =
        resolve_input_device(state.selected_input_device_name().as_deref())?;
    let config = load_best_input_config(&device)?;
    let fmt = config.sample_format();
    let ch = config.channels() as usize;

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("mic-level-monitor".into())
        .spawn({
            let app = app_handle.clone();
            let dn = device_name.clone();
            move || {
                let peak = Arc::new(AtomicU32::new(0));
                let err_cb =
                    |e: cpal::StreamError| log::warn!("Microphone preview stream error: {}", e);

                let mk_i16 = {
                    let p = peak.clone();
                    move |d: &[i16], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_i16(d, ch)), Ordering::AcqRel);
                    }
                };
                let mk_f32 = {
                    let p = peak.clone();
                    move |d: &[f32], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_f32(d, ch)), Ordering::AcqRel);
                    }
                };
                let mk_u16 = {
                    let p = peak.clone();
                    move |d: &[u16], _: &cpal::InputCallbackInfo| {
                        p.fetch_max(peak_to_meter(mono_peak_u16(d, ch)), Ordering::AcqRel);
                    }
                };

                let stream = match build_input_stream_dispatch!(
                    device, config, fmt, err_cb, mk_i16, mk_f32, mk_u16
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(format!(
                            "Failed to create microphone preview stream: {}",
                            e
                        )));
                        return;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(format!(
                        "Failed to start microphone preview stream: {}",
                        e
                    )));
                    return;
                }
                let _ = ready_tx.send(Ok(()));

                while !stop.load(Ordering::Relaxed) {
                    let meter = peak.swap(0, Ordering::AcqRel) as f32 / 1000.0;
                    let _ = app.emit(
                        "microphone-level",
                        serde_json::json!({ "deviceName": dn, "level": meter.clamp(0.0, 1.0) }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(
                        MICROPHONE_LEVEL_EMIT_INTERVAL_MS,
                    ));
                }
                let _ = app.emit(
                    "microphone-level",
                    serde_json::json!({ "deviceName": dn, "level": 0.0 }),
                );
                drop(stream);
            }
        })
        .map_err(|e| {
            AppError::Audio(format!("Failed to create microphone preview thread: {}", e))
        })?;

    match ready_rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio(e));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            let _ = handle.join();
            return Err(AppError::Audio(
                "Microphone preview startup timed out".into(),
            ));
        }
        Err(_) => {
            return Err(AppError::Audio(
                "Microphone preview thread returned no result".into(),
            ))
        }
    }

    let recording = state.recording.recording.lock();
    let mut monitor = state.recording.microphone_level_monitor.lock();
    if !monitor_start_is_current(
        state
            .recording
            .microphone_level_monitor_generation
            .load(Ordering::Acquire),
        requested_generation,
        recording.is_some(),
    ) {
        drop(monitor);
        drop(recording);
        stop_flag.store(true, Ordering::Release);
        let _ = handle.join();
        return Err(AppError::Audio(
            "Microphone preview startup request expired".into(),
        ));
    }
    *monitor = Some(MicrophoneLevelMonitor {
        stop_flag,
        handle: Some(handle),
    });
    Ok(device_name)
}

/// Records `seconds` of mono audio at 16 kHz from the selected microphone.
///
/// Used by the model test in Settings: it captures a short clip and returns it,
/// without touching the recording pipeline, history or the active application.
pub fn record_test_clip_sync(
    selected_device_name: Option<String>,
    seconds: u32,
) -> Result<Vec<i16>, AppError> {
    use cpal::traits::StreamTrait;
    use std::sync::Mutex;

    let seconds = seconds.clamp(1, 15);
    let (device, _name) = resolve_input_device(selected_device_name.as_deref())?;
    let config = load_best_input_config(&device)?;
    let input_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let fmt = config.sample_format();
    let captured: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::new()));
    // Hard cap: the longest clip the test can ask for, at the highest rate cpal offers.
    let cap = 48_000usize * 15 * 2;

    let err_cb = |e: cpal::StreamError| log::warn!("Model test stream error: {}", e);

    let c1 = captured.clone();
    let c2 = captured.clone();
    let c3 = captured.clone();
    let stream = build_input_stream_dispatch!(
        device,
        config,
        fmt,
        err_cb,
        move |data: &[i16], _: &cpal::InputCallbackInfo| {
            if let Ok(mut buffer) = c1.lock() {
                mix_to_mono_capped_i16(data, channels, &mut buffer, cap);
            }
        },
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            if let Ok(mut buffer) = c2.lock() {
                mix_to_mono_capped_f32(data, channels, &mut buffer, cap);
            }
        },
        move |data: &[u16], _: &cpal::InputCallbackInfo| {
            if let Ok(mut buffer) = c3.lock() {
                mix_to_mono_capped_u16(data, channels, &mut buffer, cap);
            }
        }
    )
    .map_err(|e| AppError::Audio(format!("Failed to create audio stream: {}", e)))?;

    stream
        .play()
        .map_err(|e| AppError::Audio(format!("Failed to start audio stream: {}", e)))?;
    std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
    drop(stream);

    let samples = captured
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    if samples.is_empty() {
        return Err(AppError::Audio(
            "No audio was captured from the microphone".into(),
        ));
    }
    let resampled = super::resample::resample_to_16k(&samples, input_rate)
        .map_err(|error| AppError::Audio(format!("Failed to resample the test clip: {error}")))?;
    Ok(resampled.into_owned())
}

pub fn test_microphone_sync(selected_device_name: Option<String>) -> Result<String, AppError> {
    use cpal::traits::StreamTrait;

    let (device, device_name) = resolve_input_device(selected_device_name.as_deref())?;
    let config = load_best_input_config(&device)?;
    let received = Arc::new(AtomicBool::new(false));
    let fmt = config.sample_format();

    let err_cb = |e: cpal::StreamError| log::warn!("Microphone test stream error: {}", e);

    let r1 = received.clone();
    let r2 = received.clone();
    let r3 = received.clone();
    let stream = build_input_stream_dispatch!(
        device,
        config,
        fmt,
        err_cb,
        move |_: &[i16], _: &cpal::InputCallbackInfo| {
            r1.store(true, Ordering::Relaxed);
        },
        move |_: &[f32], _: &cpal::InputCallbackInfo| {
            r2.store(true, Ordering::Relaxed);
        },
        move |_: &[u16], _: &cpal::InputCallbackInfo| {
            r3.store(true, Ordering::Relaxed);
        }
    )
    .map_err(|e| AppError::Audio(format!("Failed to create audio stream: {}", e)))?;

    stream
        .play()
        .map_err(|e| AppError::Audio(format!("Failed to start audio stream: {}", e)))?;
    std::thread::sleep(std::time::Duration::from_millis(220));
    drop(stream);

    if received.load(Ordering::Relaxed) {
        Ok(format!("Microphone normal ({})", device_name))
    } else {
        Ok(format!(
            "Microphone connected but no audio detected ({})",
            device_name
        ))
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::monitor_start_is_current;

    #[test]
    fn stop_and_new_recording_reject_late_monitor_start() {
        assert!(!monitor_start_is_current(2, 1, false));
        assert!(!monitor_start_is_current(1, 1, true));
        assert!(monitor_start_is_current(1, 1, false));
    }
}
