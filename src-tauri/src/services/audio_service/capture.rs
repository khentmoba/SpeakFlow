use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use tauri::Emitter;

use super::resample::{f32_to_i16, u16_to_i16};
use super::{AUDIO_CAPTURE_INIT_TIMEOUT_SECS, TARGET_SAMPLE_RATE};
use crate::services::audio_service::{InputDeviceInfo, InputDeviceListPayload};
use crate::utils::AppError;

/// 录音缓冲硬上限（单位：i16 样本，单声道，post mix-down）。即使 stop 信号
/// 丢失或某条异常路径漏掉了停止逻辑，缓冲也不会无限增长。
///
/// 30min × 48kHz mono = 86_400_000 samples = 172.8MB。覆盖任何合理录音
/// 时长；超过 30 分钟应分段录制。这是兜底安全阀，正常路径不应该触到。
pub(crate) const MAX_RECORD_SAMPLES: usize = 30 * 60 * 48_000;
const AUDIO_CAPTURE_TIMEOUT_JOIN_MS: u64 = 500;

/// Pre-roll 缓冲时长（毫秒）。避免用户按键和开口之间的极短延迟导致首音节被裁切。
pub(crate) const PRE_ROLL_DURATION_MS: u32 = 450;

static AVOID_BLUETOOTH_MIC: AtomicBool = AtomicBool::new(true);

pub fn set_avoid_bluetooth_microphone(enabled: bool) {
    AVOID_BLUETOOTH_MIC.store(enabled, Ordering::Release);
}

pub(crate) fn should_avoid_bluetooth_mic() -> bool {
    AVOID_BLUETOOTH_MIC.load(Ordering::Acquire)
}

/// Heuristic Bluetooth hands-free/headset microphone classifier for Windows
/// WASAPI/CPAL device names without misclassifying wired USB headsets.
pub fn is_bluetooth_mic_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("bluetooth")
        || lower.contains("hands-free")
        || lower.contains("ag audio")
        || lower.contains("bthhfenum")
        || lower.contains("(hfp)")
        || lower.contains("airpods")
        || lower.contains("galaxy buds")
        || lower.contains("pixel buds")
}

struct PreRollWorker {
    stop_flag: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    buffer: Arc<parking_lot::Mutex<PreRollBuffer>>,
    sample_rate: u32,
}

static PRE_ROLL_WORKER: parking_lot::Mutex<Option<PreRollWorker>> = parking_lot::Mutex::new(None);

/// Starts an idle background 450ms pre-roll ring listener when Pre-roll is
/// enabled and neither `release_mic_while_idle` nor audio ducking prevents idle capture.
pub fn start_preroll_if_eligible(
    selected_device_name: Option<String>,
    audio_behavior: &crate::state::user_profile::AudioBehaviorConfig,
    _labs: &crate::state::user_profile::LabsConfig,
    meeting_active: bool,
) {
    if !audio_behavior.pre_roll_enabled || audio_behavior.release_mic_while_idle || meeting_active {
        stop_preroll();
        return;
    }

    let mut guard = PRE_ROLL_WORKER.lock();
    if guard.is_some() {
        return;
    }

    let (device, _device_name) = match resolve_input_device(selected_device_name.as_deref()) {
        Ok(r) => r,
        Err(err) => {
            log::debug!("Pre-roll listener skipped (no input device): {err}");
            return;
        }
    };
    let config = match load_best_input_config(&device) {
        Ok(c) => c,
        Err(err) => {
            log::debug!("Pre-roll listener skipped (config unavailable): {err}");
            return;
        }
    };
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let buffer = Arc::new(parking_lot::Mutex::new(PreRollBuffer::new(
        sample_rate,
        PRE_ROLL_DURATION_MS,
    )));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    let thread_stop = stop_flag.clone();
    let thread_buf = buffer.clone();
    let handle = match std::thread::Builder::new()
        .name("audio-preroll".into())
        .spawn(move || {
            use cpal::traits::StreamTrait;
            let stop_err = thread_stop.clone();
            let err_cb = move |e: cpal::StreamError| {
                log::debug!("Pre-roll audio stream closed: {e}");
                stop_err.store(true, Ordering::Relaxed);
            };
            let push_i16 = {
                let buf = thread_buf.clone();
                let stop = thread_stop.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut mono = Vec::with_capacity(data.len() / channels.max(1) + 1);
                    mix_to_mono_capped_i16(data, channels, &mut mono, data.len().max(1));
                    buf.lock().push_samples(&mono);
                }
            };
            let push_f32 = {
                let buf = thread_buf.clone();
                let stop = thread_stop.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut mono = Vec::with_capacity(data.len() / channels.max(1) + 1);
                    mix_to_mono_capped_f32(data, channels, &mut mono, data.len().max(1));
                    buf.lock().push_samples(&mono);
                }
            };
            let push_u16 = {
                let buf = thread_buf.clone();
                let stop = thread_stop.clone();
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut mono = Vec::with_capacity(data.len() / channels.max(1) + 1);
                    mix_to_mono_capped_u16(data, channels, &mut mono, data.len().max(1));
                    buf.lock().push_samples(&mono);
                }
            };
            let stream = match build_input_stream_dispatch!(
                device,
                config,
                sample_format,
                err_cb,
                push_i16,
                push_f32,
                push_u16
            ) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(e.to_string()));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
            drop(stream);
            // An unexpected close (unplug, exclusive-mode conflict, resume from
            // sleep) leaves this worker registered but dead. Drop the slot so a
            // later start rebuilds the ring instead of early-returning on it.
            let mut slot = PRE_ROLL_WORKER.lock();
            let is_self = slot
                .as_ref()
                .is_some_and(|worker| Arc::ptr_eq(&worker.stop_flag, &thread_stop));
            if is_self {
                slot.take();
            }
        }) {
        Ok(h) => h,
        Err(err) => {
            log::debug!("Failed to spawn pre-roll listener: {err}");
            return;
        }
    };

    match ready_rx.recv_timeout(std::time::Duration::from_millis(600)) {
        Ok(Ok(())) => {
            *guard = Some(PreRollWorker {
                stop_flag,
                handle: Some(handle),
                buffer,
                sample_rate,
            });
            log::debug!(
                "Pre-roll audio ring started ({}Hz, {}ms)",
                sample_rate,
                PRE_ROLL_DURATION_MS
            );
        }
        _ => {
            stop_flag.store(true, Ordering::Release);
            drop(handle);
        }
    }
}

pub fn stop_preroll() {
    let worker = PRE_ROLL_WORKER.lock().take();
    if let Some(mut worker) = worker {
        worker.stop_flag.store(true, Ordering::Release);
        if let Some(handle) = worker.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn stop_preroll_and_take() -> Option<(Vec<i16>, u32)> {
    let worker = PRE_ROLL_WORKER.lock().take()?;
    worker.stop_flag.store(true, Ordering::Release);
    if let Some(handle) = worker.handle {
        let _ = handle.join();
    }
    let samples = worker.buffer.lock().take();
    if samples.is_empty() {
        None
    } else {
        Some((samples, worker.sample_rate))
    }
}

/// 环形 Pre-roll 缓冲区，保留最新的固定时长音频样本。
#[derive(Debug, Clone)]
pub struct PreRollBuffer {
    buffer: Vec<i16>,
    capacity: usize,
}

impl PreRollBuffer {
    pub fn new(sample_rate: u32, duration_ms: u32) -> Self {
        let capacity = ((sample_rate as usize) * (duration_ms as usize)) / 1000;
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push_samples(&mut self, samples: &[i16]) {
        if self.capacity == 0 || samples.is_empty() {
            return;
        }
        if samples.len() >= self.capacity {
            self.buffer.clear();
            self.buffer
                .extend_from_slice(&samples[samples.len() - self.capacity..]);
        } else {
            let overflow = (self.buffer.len() + samples.len()).saturating_sub(self.capacity);
            if overflow > 0 {
                self.buffer.drain(0..overflow);
            }
            self.buffer.extend_from_slice(samples);
        }
    }

    pub fn take(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.buffer)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(target_os = "windows")]
const WINDOWS_NATIVE_CAPTURE_INIT_TIMEOUT_MS: u64 = 2_000;
#[cfg(target_os = "windows")]
static WINDOWS_NATIVE_CAPTURE_DISABLED: AtomicBool = AtomicBool::new(false);

/// 一次性的"已触达录音缓冲硬上限"警告标志。仅在第一次撞上限时打日志。
static RECORD_CAP_WARNED: AtomicBool = AtomicBool::new(false);

pub(super) fn warn_if_record_cap_reached(current_len: usize) {
    if current_len >= MAX_RECORD_SAMPLES && !RECORD_CAP_WARNED.swap(true, Ordering::Relaxed) {
        log::warn!(
            "Recording buffer reached hard limit of {} i16 samples; subsequent input will be dropped",
            MAX_RECORD_SAMPLES
        );
    }
}

fn confirm_audio_thread_exit(handle: std::thread::JoinHandle<()>, wait: std::time::Duration) {
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let joiner = std::thread::Builder::new()
        .name("audio-capture-timeout-join".into())
        .spawn(move || {
            let result = handle.join();
            if result.is_err() {
                log::warn!("Recording thread panicked while exiting after startup timeout");
            }
            let _ = done_tx.send(());
        });

    match joiner {
        Ok(_) => {
            if done_rx.recv_timeout(wait).is_err() {
                log::warn!("Recording thread has not finished exiting after startup timeout; background join will continue waiting");
            }
        }
        Err(err) => {
            log::warn!(
                "Failed to create recording thread exit confirmation helper: {}",
                err
            );
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug, PartialEq, Eq)]
enum WindowsNativeCaptureStartup {
    Ready,
    Unavailable(String),
    TimedOut,
    Disconnected,
}

#[cfg(target_os = "windows")]
fn wait_for_windows_native_capture(
    receiver: &mpsc::Receiver<Result<(), String>>,
    timeout: std::time::Duration,
) -> WindowsNativeCaptureStartup {
    match receiver.recv_timeout(timeout) {
        Ok(Ok(())) => WindowsNativeCaptureStartup::Ready,
        Ok(Err(error)) => WindowsNativeCaptureStartup::Unavailable(error),
        Err(mpsc::RecvTimeoutError::Timeout) => WindowsNativeCaptureStartup::TimedOut,
        Err(mpsc::RecvTimeoutError::Disconnected) => WindowsNativeCaptureStartup::Disconnected,
    }
}

#[cfg(target_os = "windows")]
fn handoff_native_capture_samples(
    accept_rx: &mpsc::Receiver<bool>,
    probe_samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    captured_samples: &Arc<parking_lot::Mutex<Vec<i16>>>,
    cap: usize,
) -> bool {
    if !matches!(accept_rx.recv(), Ok(true)) {
        return false;
    }

    let priming_samples = std::mem::take(&mut *probe_samples.lock());
    let mut captured = captured_samples.lock();
    let remaining = cap.saturating_sub(captured.len());
    captured.extend(priming_samples.into_iter().take(remaining));
    true
}

#[cfg(target_os = "windows")]
fn run_windows_native_capture(
    stop: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
    rate_tx: &mpsc::SyncSender<Result<u32, String>>,
) -> bool {
    if WINDOWS_NATIVE_CAPTURE_DISABLED.load(Ordering::Acquire) {
        log::info!(
            "Windows native voice processing degraded for this process; using CPAL directly"
        );
        return false;
    }

    let (startup_tx, startup_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    let (accept_tx, accept_rx) = mpsc::sync_channel::<bool>(1);
    let worker_stop = stop.clone();
    let worker_samples = samples.clone();
    let native_handle = match std::thread::Builder::new()
        .name("windows-speech-capture".into())
        .spawn(move || {
            let probe_samples = Arc::new(parking_lot::Mutex::new(Vec::with_capacity(
                TARGET_SAMPLE_RATE as usize,
            )));
            match super::windows_capture::WindowsSpeechCapture::start(
                selected_device_name.as_deref(),
            ) {
                Ok(mut capture) => match capture.prime(&probe_samples) {
                    Ok(()) => {
                        // The watchdog must explicitly accept this stream. A late
                        // success after timeout is dropped instead of mixing its 16 kHz
                        // samples into the CPAL fallback buffer.
                        if startup_tx.send(Ok(())).is_ok()
                            && handoff_native_capture_samples(
                                &accept_rx,
                                &probe_samples,
                                &worker_samples,
                                MAX_RECORD_SAMPLES,
                            )
                        {
                            capture.run(&worker_stop, &worker_samples);
                            log::info!("Windows native audio capture stopped");
                        }
                    }
                    Err(error) => {
                        let _ = startup_tx.send(Err(format!(
                            "Windows native voice processing recording failed to receive first audio packet: {error}"
                        )));
                    }
                },
                Err(error) => {
                    let _ =
                        startup_tx.send(Err(format!("Windows native voice processing recording unavailable: {error}")));
                }
            }
        }) {
        Ok(handle) => handle,
        Err(error) => {
            log::warn!("Failed to create Windows native audio thread; falling back to CPAL: {}", error);
            return false;
        }
    };

    match wait_for_windows_native_capture(
        &startup_rx,
        std::time::Duration::from_millis(WINDOWS_NATIVE_CAPTURE_INIT_TIMEOUT_MS),
    ) {
        WindowsNativeCaptureStartup::Ready => {
            if accept_tx.send(true).is_err() {
                let _ = native_handle.join();
                log::warn!("Windows native audio thread exited before confirming startup; falling back to CPAL");
                return false;
            }
            let _ = rate_tx.send(Ok(TARGET_SAMPLE_RATE));
            if native_handle.join().is_err() {
                log::warn!("Windows native audio thread panicked on exit");
            }
            true
        }
        WindowsNativeCaptureStartup::Unavailable(error) => {
            drop(accept_tx);
            let _ = native_handle.join();
            log::warn!("{}; falling back to CPAL", error);
            false
        }
        WindowsNativeCaptureStartup::TimedOut => {
            WINDOWS_NATIVE_CAPTURE_DISABLED.store(true, Ordering::Release);
            drop(accept_tx);
            log::warn!(
                "Windows native voice processing recording startup exceeded {} ms; falling back to CPAL; subsequent recordings in this process will use CPAL directly",
                WINDOWS_NATIVE_CAPTURE_INIT_TIMEOUT_MS
            );
            // Do not join a worker that is blocked in a driver/COM call. Once it
            // returns, the closed acceptance channel releases the late stream.
            drop(native_handle);
            false
        }
        WindowsNativeCaptureStartup::Disconnected => {
            drop(accept_tx);
            let _ = native_handle.join();
            log::warn!(
                "Windows native audio thread returned no startup result; falling back to CPAL"
            );
            false
        }
    }
}

// ---------- cpal 设备管理 ----------

pub(super) fn resolve_input_device(
    preferred_name: Option<&str>,
) -> Result<(cpal::Device, String), AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();

    if let Some(name) = preferred_name.filter(|n| !n.trim().is_empty()) {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                let dn = device.name().unwrap_or_default();
                if dn == name {
                    return Ok((device, dn));
                }
            }
        }
        log::warn!(
            "Specified microphone unavailable; falling back to default device: {}",
            name
        );
    }

    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Audio("No available audio input device found".into()))?;
    let name = device.name().unwrap_or_else(|_| "Unknown Device".into());

    if should_avoid_bluetooth_mic() && is_bluetooth_mic_name(&name) {
        if let Ok(devices) = host.input_devices() {
            for candidate in devices {
                if let Ok(candidate_name) = candidate.name() {
                    if !is_bluetooth_mic_name(&candidate_name) {
                        log::info!(
                            "Avoided Bluetooth default microphone '{}' in favor of '{}'",
                            name,
                            candidate_name
                        );
                        return Ok((candidate, candidate_name));
                    }
                }
            }
        }
    }

    Ok((device, name))
}

pub(super) fn load_best_input_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, AppError> {
    use cpal::traits::DeviceTrait;
    use cpal::SampleFormat::{F32, I16, U16};

    let configs: Vec<_> = device
        .supported_input_configs()
        .map_err(|e| AppError::Audio(format!("Failed to query audio device configuration: {}", e)))?
        .collect();

    if configs.is_empty() {
        return Err(AppError::Audio(
            "Audio device does not support any input configuration".into(),
        ));
    }

    let supports_16k = |c: &&cpal::SupportedStreamConfigRange| {
        c.min_sample_rate().0 <= TARGET_SAMPLE_RATE && c.max_sample_rate().0 >= TARGET_SAMPLE_RATE
    };
    let fmt = |f| move |c: &&cpal::SupportedStreamConfigRange| c.sample_format() == f;

    let pick = configs
        .iter()
        .find(|c| fmt(I16)(c) && supports_16k(c))
        .or_else(|| configs.iter().find(|c| fmt(F32)(c) && supports_16k(c)))
        .or_else(|| configs.iter().find(|c| fmt(U16)(c) && supports_16k(c)))
        .map(|c| c.with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE)))
        .or_else(|| {
            configs
                .iter()
                .find(|c| fmt(I16)(c))
                .or_else(|| configs.iter().find(|c| fmt(F32)(c)))
                .or_else(|| configs.iter().find(|c| fmt(U16)(c)))
                .or(configs.first())
                .map(|c| c.with_max_sample_rate())
        });

    pick.ok_or_else(|| AppError::Audio("Unable to find suitable audio input configuration".into()))
}

pub fn list_input_devices_sync(
    selected_device_name: Option<String>,
) -> Result<InputDeviceListPayload, AppError> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    let mut devices: Vec<InputDeviceInfo> = host
        .input_devices()
        .map_err(|e| AppError::Audio(format!("Failed to enumerate audio input devices: {}", e)))?
        .filter_map(|d| {
            d.name().ok().map(|name| InputDeviceInfo {
                is_default: default_name.as_deref() == Some(&name),
                is_bluetooth: is_bluetooth_mic_name(&name),
                name,
            })
        })
        .collect();

    devices.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(InputDeviceListPayload {
        devices,
        selected_device_name,
    })
}

// ---------- 多声道混音到单声道 i16（带硬上限） ----------

/// Whisper mode doubles the input gain so quiet speech clears the VAD and the
/// recognizer's level assumptions. It is a capture-wide flag because the capture
/// callback cannot read settings from disk on every buffer.
static WHISPER_GAIN: AtomicBool = AtomicBool::new(false);

/// +6 dB. Loud speech clips at the i16 ceiling, which the saturating multiply
/// below handles without wrapping into noise.
pub(crate) const WHISPER_GAIN_MULTIPLIER: i32 = 2;

pub(crate) fn set_whisper_gain(enabled: bool) {
    WHISPER_GAIN.store(enabled, Ordering::Relaxed);
}

fn apply_whisper_gain(sample: i16) -> i16 {
    apply_gain(sample, WHISPER_GAIN.load(Ordering::Relaxed))
}

/// The gain itself, without the process-wide flag, so it can be tested.
fn apply_gain(sample: i16, enabled: bool) -> i16 {
    if !enabled {
        return sample;
    }
    (sample as i32 * WHISPER_GAIN_MULTIPLIER).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

pub(crate) fn mix_to_mono_capped_i16(
    data: &[i16],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    // mono input: 1 input sample == 1 output sample
    // multi-channel: chans input samples == 1 output sample
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend(limited.iter().map(|&sample| apply_whisper_gain(sample)));
    } else {
        out.extend(limited.chunks_exact(chans).map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            apply_whisper_gain((sum / chans as i32) as i16)
        }));
    }
}

pub(crate) fn mix_to_mono_capped_f32(
    data: &[f32],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend(limited.iter().map(|&s| apply_whisper_gain(f32_to_i16(s))));
    } else {
        out.extend(
            limited.chunks_exact(chans).map(|frame| {
                apply_whisper_gain(f32_to_i16(frame.iter().sum::<f32>() / chans as f32))
            }),
        );
    }
}

pub(crate) fn mix_to_mono_capped_u16(
    data: &[u16],
    channels: usize,
    out: &mut Vec<i16>,
    cap: usize,
) {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(out.len());
    if allowed_mono_samples == 0 {
        return;
    }
    let frame_count = data.len() / chans;
    let take_frames = frame_count.min(allowed_mono_samples);
    if take_frames == 0 {
        return;
    }
    let limited = &data[..take_frames * chans];
    if chans <= 1 {
        out.extend(limited.iter().map(|&s| u16_to_i16(s)));
    } else {
        out.extend(limited.chunks_exact(chans).map(|frame| {
            let sum: u64 = frame.iter().map(|&s| s as u64).sum();
            u16_to_i16((sum / chans as u64) as u16)
        }));
    }
}

// ---------- 录音波形可视化 ----------

const WAVEFORM_BAR_COUNT: usize = 9;
const WAVEFORM_EMIT_INTERVAL_MS: u64 = 55;
const WAVEFORM_WINDOW_SECS: f64 = 0.12;

fn compute_waveform_bars(samples: &[i16]) -> Vec<f32> {
    let mut bars = vec![0.0f32; WAVEFORM_BAR_COUNT];
    if samples.is_empty() {
        return bars;
    }
    let chunk = samples.len() / WAVEFORM_BAR_COUNT;
    if chunk == 0 {
        return bars;
    }
    for (i, bar) in bars.iter_mut().enumerate() {
        let start = i * chunk;
        let end = (start + chunk).min(samples.len());
        let rms = (samples[start..end]
            .iter()
            .map(|&s| (s as f32) * (s as f32))
            .sum::<f32>()
            / (end - start) as f32)
            .sqrt();
        *bar = (rms / 5000.0).min(1.0).sqrt();
    }
    bars
}

pub fn spawn_waveform_emitter(
    app_handle: tauri::AppHandle,
    session_id: u64,
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    sample_rate: u32,
) {
    let window_size = (sample_rate as f64 * WAVEFORM_WINDOW_SECS) as usize;

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(WAVEFORM_EMIT_INTERVAL_MS)).await;
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            let bars = {
                let guard = samples.lock();
                let start = guard.len().saturating_sub(window_size);
                compute_waveform_bars(&guard[start..])
            };
            let _ = app_handle.emit(
                "waveform",
                serde_json::json!({ "sessionId": session_id, "bars": bars }),
            );
        }
    });
}

// ---------- 音频捕获线程 ----------

pub fn spawn_audio_capture_thread(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    spawn_audio_capture_thread_with_preroll(stop_flag, samples, selected_device_name, None)
}

fn resample_preroll_to_rate(preroll: Vec<i16>, from_rate: u32, to_rate: u32) -> Vec<i16> {
    if preroll.is_empty() || from_rate == to_rate || from_rate == 0 || to_rate == 0 {
        return preroll;
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let out_len = ((preroll.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for idx in 0..out_len {
        let src = (idx as f64) / ratio;
        let i0 = (src.floor() as usize).min(preroll.len().saturating_sub(1));
        let i1 = (i0 + 1).min(preroll.len().saturating_sub(1));
        let frac = src - (i0 as f64);
        let sample = (preroll[i0] as f64) * (1.0 - frac) + (preroll[i1] as f64) * frac;
        out.push(sample.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
    }
    out
}

pub fn spawn_audio_capture_thread_with_preroll(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<parking_lot::Mutex<Vec<i16>>>,
    selected_device_name: Option<String>,
    preroll: Option<(Vec<i16>, u32)>,
) -> Result<(std::thread::JoinHandle<()>, u32), AppError> {
    // 每个新录音会话重置警告 latch；否则进程级一次警告之后，后续会话即便
    // 再次撞上限也不会写日志，丢失诊断信息。
    RECORD_CAP_WARNED.store(false, Ordering::Relaxed);
    // Quiet-speech gain is read once per session so the capture callback never
    // touches the settings file.
    set_whisper_gain(crate::utils::paths::read_whisper_mode());

    let (rate_tx, rate_rx) = std::sync::mpsc::sync_channel::<Result<u32, String>>(1);
    let stop = stop_flag.clone();

    let handle = std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            use cpal::traits::StreamTrait;

            let preroll_seed = preroll;

            #[cfg(target_os = "windows")]
            {
                if let Some((ref pre_samples, pre_rate)) = preroll_seed {
                    if !pre_samples.is_empty() {
                        let resampled = resample_preroll_to_rate(
                            pre_samples.clone(),
                            pre_rate,
                            TARGET_SAMPLE_RATE,
                        );
                        let mut locked = samples.lock();
                        let remaining = MAX_RECORD_SAMPLES.saturating_sub(locked.len());
                        locked.extend(resampled.into_iter().take(remaining));
                    }
                }
                if run_windows_native_capture(
                    stop.clone(),
                    samples.clone(),
                    selected_device_name.clone(),
                    &rate_tx,
                ) {
                    return;
                }
                if preroll_seed.is_some() {
                    // Clear 16k seed if native capture fell back to CPAL, so CPAL seeds at native sample_rate.
                    samples.lock().clear();
                }
            }

            let (device, device_name) = match resolve_input_device(selected_device_name.as_deref())
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = rate_tx.send(Err(e.to_string()));
                    return;
                }
            };
            log::info!("Using audio input device: {}", device_name);

            let config = match load_best_input_config(&device) {
                Ok(c) => c,
                Err(e) => {
                    let _ = rate_tx.send(Err(e.to_string()));
                    return;
                }
            };

            let sample_rate = config.sample_rate().0;
            let channels = config.channels() as usize;
            let sample_format = config.sample_format();
            log::info!(
                "Audio configuration: {}Hz, {}ch, {:?}",
                sample_rate,
                channels,
                sample_format
            );

            if let Some((pre_samples, pre_rate)) = preroll_seed {
                if !pre_samples.is_empty() {
                    let resampled = resample_preroll_to_rate(pre_samples, pre_rate, sample_rate);
                    let mut locked = samples.lock();
                    let remaining = MAX_RECORD_SAMPLES.saturating_sub(locked.len());
                    locked.extend(resampled.into_iter().take(remaining));
                }
            }

            let stop_for_err = stop.clone();
            let err_cb = move |e: cpal::StreamError| {
                log::error!("Audio stream error: {}", e);
                stop_for_err.store(true, Ordering::Relaxed);
            };
            let stop_cb = stop.clone();

            let mk_i16 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut locked = buf.lock();
                    warn_if_record_cap_reached(locked.len());
                    mix_to_mono_capped_i16(data, channels, &mut locked, MAX_RECORD_SAMPLES);
                }
            };
            let mk_f32 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut locked = buf.lock();
                    warn_if_record_cap_reached(locked.len());
                    mix_to_mono_capped_f32(data, channels, &mut locked, MAX_RECORD_SAMPLES);
                }
            };
            let mk_u16 = {
                let buf = samples.clone();
                let stop = stop_cb.clone();
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let mut locked = buf.lock();
                    warn_if_record_cap_reached(locked.len());
                    mix_to_mono_capped_u16(data, channels, &mut locked, MAX_RECORD_SAMPLES);
                }
            };

            let stream = match build_input_stream_dispatch!(
                device,
                config,
                sample_format,
                err_cb,
                mk_i16,
                mk_f32,
                mk_u16
            ) {
                Ok(s) => s,
                Err(e) => {
                    let _ = rate_tx.send(Err(format!("Failed to create audio stream: {}", e)));
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = rate_tx.send(Err(format!("Failed to start audio stream: {}", e)));
                return;
            }
            let _ = rate_tx.send(Ok(sample_rate));

            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            drop(stream);
            log::info!("Audio capture stopped");
        })
        .map_err(|e| AppError::Audio(format!("Failed to create recording thread: {}", e)))?;

    let sample_rate = match rate_rx.recv_timeout(std::time::Duration::from_secs(
        AUDIO_CAPTURE_INIT_TIMEOUT_SECS,
    )) {
        Ok(r) => r.map_err(AppError::Audio)?,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            stop_flag.store(true, Ordering::Relaxed);
            confirm_audio_thread_exit(
                handle,
                std::time::Duration::from_millis(AUDIO_CAPTURE_TIMEOUT_JOIN_MS),
            );
            return Err(AppError::Audio(format!(
                "Recording thread startup timed out ({} seconds)",
                AUDIO_CAPTURE_INIT_TIMEOUT_SECS
            )));
        }
        Err(_) => {
            return Err(AppError::Audio(
                "Recording thread returned no result after startup".into(),
            ))
        }
    };

    Ok((handle, sample_rate))
}

#[cfg(test)]
mod cap_tests {
    //! Tests for the capped mix-to-mono helpers and the buffer hard cap.
    //!
    //! Contract:
    //!   - `mix_to_mono_capped_<T>(data, channels, out, cap)` mixes multi-channel
    //!     audio down to mono i16 (averaging across channels per frame),
    //!     same as the existing private `mix_to_mono_<T>` helpers, **but**
    //!     `out.len()` MUST NOT exceed `cap` after the call.
    //!   - If `out.len() >= cap` already, append zero.
    //!   - If a partial frame fits, append only the frames that fit.
    //!
    //!   - `MAX_RECORD_SAMPLES` is 30 minutes of 48 kHz mono and must be
    //!     large enough to cover at least an hour of 16 kHz mono.
    use super::{
        apply_gain, mix_to_mono_capped_f32, mix_to_mono_capped_i16, mix_to_mono_capped_u16,
        MAX_RECORD_SAMPLES,
    };
    #[cfg(target_os = "windows")]
    use super::{
        handoff_native_capture_samples, spawn_audio_capture_thread,
        wait_for_windows_native_capture, WindowsNativeCaptureStartup,
    };

    const MIN_ONE_HOUR_16K_SAMPLES: usize = 60 * 60 * 16_000;
    const _: () = assert!(MAX_RECORD_SAMPLES >= MIN_ONE_HOUR_16K_SAMPLES);

    // ----- MAX_RECORD_SAMPLES constant -----------------------------------

    #[test]
    fn max_record_samples_constant_value() {
        assert_eq!(MAX_RECORD_SAMPLES, 30 * 60 * 48_000);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_watchdog_accepts_ready_startup() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        tx.send(Ok(())).unwrap();

        assert_eq!(
            wait_for_windows_native_capture(&rx, std::time::Duration::from_millis(20)),
            WindowsNativeCaptureStartup::Ready
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_watchdog_preserves_startup_error() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        tx.send(Err("device unavailable".into())).unwrap();

        assert_eq!(
            wait_for_windows_native_capture(&rx, std::time::Duration::from_millis(20)),
            WindowsNativeCaptureStartup::Unavailable("device unavailable".into())
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_watchdog_times_out_without_waiting_for_worker() {
        let (_tx, rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        let started = std::time::Instant::now();

        assert_eq!(
            wait_for_windows_native_capture(&rx, std::time::Duration::from_millis(20)),
            WindowsNativeCaptureStartup::TimedOut
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(200));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_handoff_puts_priming_before_live_samples_once() {
        let probe_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![101, 102, 103]));
        let captured_samples = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (accept_tx, accept_rx) = std::sync::mpsc::sync_channel(1);
        accept_tx.send(true).unwrap();

        assert!(handoff_native_capture_samples(
            &accept_rx,
            &probe_samples,
            &captured_samples,
            8,
        ));

        // `run` appends live packets after the handoff. The probe must already
        // be at the front of the shared stream and must be consumed exactly once.
        captured_samples.lock().extend([201, 202]);
        assert_eq!(*captured_samples.lock(), vec![101, 102, 103, 201, 202]);
        assert!(probe_samples.lock().is_empty());

        let (second_accept_tx, second_accept_rx) = std::sync::mpsc::sync_channel(1);
        second_accept_tx.send(true).unwrap();
        assert!(handoff_native_capture_samples(
            &second_accept_rx,
            &probe_samples,
            &captured_samples,
            8,
        ));
        assert_eq!(*captured_samples.lock(), vec![101, 102, 103, 201, 202]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_handoff_honors_cap_after_existing_samples() {
        let probe_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![10, 11, 12, 13]));
        let captured_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![1, 2, 3]));
        let (accept_tx, accept_rx) = std::sync::mpsc::sync_channel(1);
        accept_tx.send(true).unwrap();

        assert!(handoff_native_capture_samples(
            &accept_rx,
            &probe_samples,
            &captured_samples,
            5,
        ));

        assert_eq!(*captured_samples.lock(), vec![1, 2, 3, 10, 11]);
        assert_eq!(captured_samples.lock().len(), 5);
        assert!(probe_samples.lock().is_empty());
    }

    #[test]
    fn pre_roll_buffer_maintains_fixed_duration_and_retains_latest_samples() {
        let mut preroll = super::PreRollBuffer::new(16_000, 300); // 4800 samples capacity
        assert_eq!(preroll.capacity, 4800);
        assert_eq!(preroll.len(), 0);

        preroll.push_samples(&[1, 2, 3]);
        assert_eq!(preroll.len(), 3);

        // Push more samples than capacity: it should keep only the latest 4800
        let big_chunk: Vec<i16> = (0..5000).map(|i| i as i16).collect();
        preroll.push_samples(&big_chunk);
        assert_eq!(preroll.len(), 4800);
        assert_eq!(preroll.take()[0], 200); // 5000 - 4800 = 200
        assert_eq!(preroll.len(), 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_handoff_rejection_leaves_fallback_untouched() {
        let probe_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![101, 102]));
        let captured_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![901, 902]));
        let (accept_tx, accept_rx) = std::sync::mpsc::sync_channel(1);
        accept_tx.send(false).unwrap();

        assert!(!handoff_native_capture_samples(
            &accept_rx,
            &probe_samples,
            &captured_samples,
            8,
        ));

        assert_eq!(*captured_samples.lock(), vec![901, 902]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_handoff_cancellation_leaves_fallback_untouched() {
        let probe_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![101, 102]));
        let captured_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![901, 902]));
        let (accept_tx, accept_rx) = std::sync::mpsc::sync_channel(1);
        drop(accept_tx);

        assert!(!handoff_native_capture_samples(
            &accept_rx,
            &probe_samples,
            &captured_samples,
            8,
        ));

        assert_eq!(*captured_samples.lock(), vec![901, 902]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_capture_handoff_watchdog_timeout_does_not_leak_late_probe() {
        let (_startup_tx, startup_rx) = std::sync::mpsc::sync_channel::<Result<(), String>>(1);
        assert_eq!(
            wait_for_windows_native_capture(&startup_rx, std::time::Duration::ZERO),
            WindowsNativeCaptureStartup::TimedOut
        );

        let probe_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![101, 102]));
        let captured_samples = std::sync::Arc::new(parking_lot::Mutex::new(vec![901, 902]));
        let (accept_tx, accept_rx) = std::sync::mpsc::sync_channel(1);
        // The timeout branch drops its sender; a late native worker observes
        // this disconnect and must not write its already-primed samples.
        drop(accept_tx);

        assert!(!handoff_native_capture_samples(
            &accept_rx,
            &probe_samples,
            &captured_samples,
            8,
        ));
        assert_eq!(*captured_samples.lock(), vec![901, 902]);
    }

    // ----- mix_to_mono_capped_i16 ----------------------------------------

    #[test]
    fn whisper_gain_doubles_and_saturates() {
        assert_eq!(apply_gain(1000, false), 1000);
        assert_eq!(apply_gain(1000, true), 2000);
        assert_eq!(apply_gain(-1000, true), -2000);
        // Loud speech clips at the ceiling instead of wrapping around.
        assert_eq!(apply_gain(20_000, true), i16::MAX);
        assert_eq!(apply_gain(-20_000, true), i16::MIN);
    }

    #[test]
    fn mix_to_mono_capped_i16_under_cap_appends_all() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_i16_at_cap_appends_zero() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_i16_partial_fit_truncates() {
        let data = vec![2i16; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_i16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_i16_stereo_downmix() {
        // 10 stereo frames of (1, 2) -> 10 mono samples after mixing
        let data: Vec<i16> = (0..10).flat_map(|_| [1i16, 2i16]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_i16_stereo_partial_fit() {
        // 10 stereo frames -> at most 10 mono frames; only 5 should fit.
        let data: Vec<i16> = (0..10).flat_map(|_| [1i16, 2i16]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_i16(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_i16_zero_cap_appends_zero() {
        let data = vec![1i16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_i16(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_i16(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }

    // ----- mix_to_mono_capped_f32 ----------------------------------------

    #[test]
    fn mix_to_mono_capped_f32_under_cap_appends_all() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_f32_at_cap_appends_zero() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_f32_partial_fit_truncates() {
        let data = vec![-0.5f32; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_f32(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_f32_stereo_downmix() {
        let data: Vec<f32> = (0..10).flat_map(|_| [0.5f32, -0.5f32]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_f32_stereo_partial_fit() {
        let data: Vec<f32> = (0..10).flat_map(|_| [0.5f32, -0.5f32]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_f32(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_f32_zero_cap_appends_zero() {
        let data = vec![0.5f32; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_f32(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_f32(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }

    // ----- mix_to_mono_capped_u16 ----------------------------------------

    #[test]
    fn mix_to_mono_capped_u16_under_cap_appends_all() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn mix_to_mono_capped_u16_at_cap_appends_zero() {
        let data = vec![40000u16; 50];
        let mut out: Vec<i16> = vec![0; 100];
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn mix_to_mono_capped_u16_partial_fit_truncates() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = vec![0; 90];
        mix_to_mono_capped_u16(&data, 1, &mut out, 100);
        assert_eq!(out.len(), 100, "should fill exactly up to cap, no overflow");
    }

    #[test]
    fn mix_to_mono_capped_u16_stereo_downmix() {
        let data: Vec<u16> = (0..10).flat_map(|_| [32768u16, 40000u16]).collect();
        assert_eq!(data.len(), 20);
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 2, &mut out, 100);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn mix_to_mono_capped_u16_stereo_partial_fit() {
        let data: Vec<u16> = (0..10).flat_map(|_| [32768u16, 40000u16]).collect();
        let mut out: Vec<i16> = vec![0; 95];
        mix_to_mono_capped_u16(&data, 2, &mut out, 100);
        assert_eq!(
            out.len(),
            100,
            "5 frames fit into the remaining 5 slots, no overflow"
        );
    }

    #[test]
    fn mix_to_mono_capped_u16_zero_cap_appends_zero() {
        let data = vec![32768u16; 50];
        let mut out: Vec<i16> = Vec::new();
        mix_to_mono_capped_u16(&data, 1, &mut out, 0);
        assert_eq!(out.len(), 0);

        let mut out2: Vec<i16> = vec![0; 30];
        mix_to_mono_capped_u16(&data, 1, &mut out2, 0);
        assert_eq!(out2.len(), 30, "cap=0 must never shrink existing buffer");
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires an available Windows microphone"]
    fn capture_entrypoint_uses_16k_native_stream() {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let samples = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let (handle, sample_rate) =
            spawn_audio_capture_thread(stop.clone(), samples.clone(), None).expect("start capture");
        std::thread::sleep(std::time::Duration::from_millis(500));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        handle.join().expect("join capture thread");

        let frame_count = samples.lock().len();
        eprintln!("entrypoint_sample_rate={sample_rate} entrypoint_frames={frame_count}");
        assert_eq!(sample_rate, super::TARGET_SAMPLE_RATE);
        assert!(frame_count >= sample_rate as usize / 4);
    }
}
