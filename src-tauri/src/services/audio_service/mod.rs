use serde::Serialize;

// ---------- 常量 ----------

pub(crate) const TARGET_SAMPLE_RATE: u32 = 16000;
/// finalize_recording 的下限：低于这个时长整段录音直接跳过（视为误按）
pub(crate) const MIN_AUDIO_DURATION_SEC: f64 = 0.5;
/// interim 的下限：首个 tick 积到这个时长就开始送 Python 推理，不再等到 0.5s。
/// 短于 0.5s 的部分会在 funasr_service::transcribe_pcm16 里尾部补零对齐 Python VAD。
pub(crate) const MIN_INTERIM_DURATION_SEC: f64 = 0.2;
pub(crate) const MIN_SAMPLES_GROWTH: usize = 1024;

pub(crate) const INTERIM_INTERVAL_MIN_MS: u64 = 140;
pub(crate) const INTERIM_INTERVAL_BASE_MS: u64 = 220;
pub(crate) const INTERIM_INTERVAL_MAX_MS: u64 = 460;
pub(crate) const INTERIM_INTERVAL_DOWN_STEP_MS: u64 = 24;
pub(crate) const INTERIM_INTERVAL_UP_STEP_MS: u64 = 42;
pub(crate) const INTERIM_HEAVY_COST_MS: u64 = 420;
pub(crate) const INTERIM_LIGHT_COST_MS: u64 = 180;
pub(crate) const INTERIM_MAX_AUDIO_WINDOW_SEC: f64 = 12.0;

pub(crate) const RESULT_HIDE_DELAY_MS: u64 = 2500;
pub(crate) const EMPTY_RESULT_HIDE_DELAY_MS: u64 = 360;
/// Brief result-preview delay; target-window checks, not timing, protect paste focus.
pub(crate) const PASTE_DELAY_MS: u64 = 60;
pub(crate) const AUDIO_CAPTURE_INIT_TIMEOUT_SECS: u64 = 8;
/// 20 Hz, matching the dictation meter cadence in the web and desktop clients.
pub(crate) const MICROPHONE_LEVEL_EMIT_INTERVAL_MS: u64 = 50;
/// finalize 阶段等待并行抓取选中文本的最大时长。超时就按普通听写处理。
pub(crate) const EDIT_GRAB_WAIT_MS: u64 = 650;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceInfo {
    pub name: String,
    pub is_default: bool,
    #[serde(default)]
    pub is_bluetooth: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceListPayload {
    pub devices: Vec<InputDeviceInfo>,
    pub selected_device_name: Option<String>,
}

// ---------- 统一的多格式音频流构建宏 ----------

/// 为三种采样格式（I16/F32/U16）构建 cpal 输入流，消除重复代码。
macro_rules! build_input_stream_dispatch {
    ($device:expr, $config:expr, $sample_format:expr, $err_cb:expr, $callback_i16:expr, $callback_f32:expr, $callback_u16:expr) => {
        match $sample_format {
            cpal::SampleFormat::I16 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_i16, $err_cb, None)
            }
            cpal::SampleFormat::F32 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_f32, $err_cb, None)
            }
            cpal::SampleFormat::U16 => {
                use cpal::traits::DeviceTrait;
                $device.build_input_stream(&$config.into(), $callback_u16, $err_cb, None)
            }
            _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        }
    };
}

// ---------- 子模块 ----------

pub(crate) mod audio_ducking;
mod capture;
mod finalize;
mod interim;
mod monitor;
mod native_capture;
#[cfg(test)]
mod native_capture_tests;
#[cfg(test)]
mod native_integration_tests;
mod native_recording;
pub(crate) mod output;
#[cfg(test)]
mod output_tests;
mod resample;
mod wav;
#[cfg(target_os = "windows")]
mod windows_capture;

// ---------- 外部 API 再导出 ----------
//
// 保持外部引用点零改动：
// `use crate::services::audio_service::X` 在拆分前后语义相同。

#[allow(unused_imports)]
pub use audio_ducking::{
    duck_audio_with_config, restore_audio, update_audio_behavior, PreventSleep,
};
#[allow(unused_imports)]
pub use capture::{
    is_bluetooth_mic_name, list_input_devices_sync, set_avoid_bluetooth_microphone,
    spawn_audio_capture_thread, spawn_audio_capture_thread_with_preroll, spawn_waveform_emitter,
    start_preroll_if_eligible, stop_preroll, stop_preroll_and_take,
};
pub(crate) use finalize::flush_pending_paste;
pub(crate) use finalize::restart_idle_preroll_if_eligible;
pub use finalize::{discard_recording, finalize_recording};
pub use interim::spawn_interim_loop;
pub use monitor::{
    record_test_clip_sync, start_microphone_level_monitor, stop_microphone_level_monitor,
    test_microphone_sync,
};
pub(crate) use native_recording::spawn_native_recording;
pub use native_recording::NativeRecording;
pub use wav::encode_wav;

/// Resamples arbitrary-rate mono PCM to the engine rate.
///
/// Exposed for file transcription, which decodes at the file's own rate.
pub(crate) fn resample_for_file(samples: &[i16], input_rate: u32) -> Result<Vec<i16>, String> {
    resample::resample_to_16k(samples, input_rate).map(|value| value.into_owned())
}
