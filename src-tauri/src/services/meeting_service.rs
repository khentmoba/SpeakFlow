//! Dual-track Windows meeting recorder (microphone + WASAPI loopback),
//! call-app detection, speaker-diarized transcription, LLM notes/summary,
//! Markdown/SRT/VTT/JSON export, and audio expiration cleanup.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use crate::services::audio_service::{
    encode_wav, resample_for_file, spawn_audio_capture_thread, PreventSleep, TARGET_SAMPLE_RATE,
};
use crate::services::funasr_service;
use crate::services::llm_client::{self, LlmRequestOptions, LlmUserInput};
use crate::services::llm_provider;
use crate::services::local_only;
use crate::state::user_profile::{MeetingAppPolicy, MeetingsConfig};
use crate::state::AppState;
use crate::utils::paths;

const MEETINGS_DIR: &str = "meetings";
const MEETINGS_INDEX_FILE: &str = "meetings.json";
const MAX_MEETINGS_STORED: usize = 500;
const MAX_MEETING_SAMPLES: usize = 2 * 60 * 60 * 48_000;
const LOOPBACK_POLL_MS: u64 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSegment {
    pub id: String,
    pub speaker: String,
    pub track: String, // "mic" | "loopback"
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingRecord {
    pub id: String,
    pub title: String,
    pub app_name: Option<String>,
    pub status: String, // "completed" | "failed"
    pub started_at: u64,
    pub ended_at: u64,
    pub duration_sec: f64,
    pub mic_audio_path: Option<String>,
    pub loopback_audio_path: Option<String>,
    pub markdown_path: Option<String>,
    pub summary: Option<String>,
    pub transcript_text: String,
    pub segments: Vec<MeetingSegment>,
    pub word_count: usize,
    pub speaker_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStatusPayload {
    pub recording: bool,
    pub processing: bool,
    pub active_id: Option<String>,
    pub title: Option<String>,
    pub app_name: Option<String>,
    pub started_at: Option<u64>,
    pub duration_sec: u64,
    pub mic_samples: usize,
    pub loopback_samples: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CallDetectionResult {
    pub active: bool,
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub policy: MeetingAppPolicy,
}

struct ActiveMeetingSession {
    id: String,
    title: String,
    app_name: Option<String>,
    started_at: u64,
    stop_flag: Arc<AtomicBool>,
    mic_samples: Arc<Mutex<Vec<i16>>>,
    mic_sample_rate: u32,
    mic_thread: Option<JoinHandle<()>>,
    loopback_samples: Arc<Mutex<Vec<i16>>>,
    loopback_sample_rate: u32,
    loopback_thread: Option<JoinHandle<()>>,
    sleep_thread: Option<JoinHandle<()>>,
}

static ACTIVE_MEETING: Mutex<Option<ActiveMeetingSession>> = Mutex::new(None);
static MEETING_PROCESSING: AtomicBool = AtomicBool::new(false);
static MEETINGS_IO_LOCK: Mutex<()> = Mutex::new(());

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn meetings_dir() -> PathBuf {
    paths::get_data_dir().join(MEETINGS_DIR)
}

fn meetings_index_path() -> PathBuf {
    meetings_dir().join(MEETINGS_INDEX_FILE)
}

pub fn load_meetings() -> Vec<MeetingRecord> {
    let _guard = MEETINGS_IO_LOCK.lock();
    let path = meetings_index_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<MeetingRecord>>(&raw).unwrap_or_default()
}

fn save_meetings(records: &[MeetingRecord]) -> Result<(), String> {
    let _guard = MEETINGS_IO_LOCK.lock();
    let dir = meetings_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create meetings directory: {e}"))?;
    let json = serde_json::to_string_pretty(records)
        .map_err(|e| format!("Failed to serialize meetings: {e}"))?;
    paths::atomic_write(&meetings_index_path(), json.as_bytes())
        .map_err(|e| format!("Failed to persist meetings index: {e}"))
}

pub fn get_meeting_by_id(id: &str) -> Option<MeetingRecord> {
    load_meetings().into_iter().find(|m| m.id == id)
}

pub fn is_meeting_recording() -> bool {
    ACTIVE_MEETING.lock().is_some()
}

pub fn current_status() -> MeetingStatusPayload {
    let processing = MEETING_PROCESSING.load(Ordering::Acquire);
    let guard = ACTIVE_MEETING.lock();
    match guard.as_ref() {
        Some(active) => {
            let elapsed = now_secs().saturating_sub(active.started_at);
            MeetingStatusPayload {
                recording: true,
                processing,
                active_id: Some(active.id.clone()),
                title: Some(active.title.clone()),
                app_name: active.app_name.clone(),
                started_at: Some(active.started_at),
                duration_sec: elapsed,
                mic_samples: active.mic_samples.lock().len(),
                loopback_samples: active.loopback_samples.lock().len(),
            }
        }
        None => MeetingStatusPayload {
            recording: false,
            processing,
            active_id: None,
            title: None,
            app_name: None,
            started_at: None,
            duration_sec: 0,
            mic_samples: 0,
            loopback_samples: 0,
        },
    }
}

fn core_call_app_match(process_lower: &str, title_lower: &str) -> Option<&'static str> {
    if process_lower == "zoom.exe" || title_lower.contains("zoom meeting") {
        return Some("Zoom");
    }
    if process_lower == "teams.exe"
        || process_lower == "ms-teams.exe"
        || title_lower.contains("microsoft teams")
    {
        return Some("Microsoft Teams");
    }
    if process_lower == "discord.exe" || title_lower.contains("discord |") {
        return Some("Discord");
    }
    if process_lower == "slack.exe" && title_lower.contains("huddle") {
        return Some("Slack Huddle");
    }
    if title_lower.contains("meet - ") || title_lower.contains("google meet") {
        return Some("Google Meet");
    }
    if process_lower.contains("webex") || title_lower.contains("webex") {
        return Some("Webex");
    }
    None
}

fn extended_call_app_match(process_lower: &str, title_lower: &str) -> Option<&'static str> {
    if let Some(core) = core_call_app_match(process_lower, title_lower) {
        return Some(core);
    }
    if process_lower == "skype.exe" || title_lower.contains("skype") {
        return Some("Skype");
    }
    if process_lower == "loom.exe" || title_lower.contains("loom") {
        return Some("Loom");
    }
    if process_lower.contains("chime") || title_lower.contains("amazon chime") {
        return Some("Amazon Chime");
    }
    if process_lower.contains("ringcentral") || title_lower.contains("ringcentral") {
        return Some("RingCentral");
    }
    if process_lower.contains("gather") || title_lower.contains("gather.town") {
        return Some("Gather");
    }
    if title_lower.contains("around") || title_lower.contains("whereby") {
        return Some("Web Call");
    }
    if process_lower == "slack.exe" {
        return Some("Slack");
    }
    None
}

pub fn match_call_window(
    process_name: &str,
    window_title: &str,
    config: &MeetingsConfig,
) -> Option<CallDetectionResult> {
    let process_lower = process_name.trim().to_ascii_lowercase();
    let title_lower = window_title.trim().to_ascii_lowercase();
    if process_lower.is_empty() && title_lower.is_empty() {
        return None;
    }
    let app_name = if config.more_call_apps {
        extended_call_app_match(&process_lower, &title_lower)
    } else {
        core_call_app_match(&process_lower, &title_lower)
    }?;
    let policy = config
        .app_policies
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(app_name) || k.eq_ignore_ascii_case(process_name))
        .map(|(_, p)| *p)
        .unwrap_or(MeetingAppPolicy::Ask);
    if policy == MeetingAppPolicy::Never {
        return None;
    }
    Some(CallDetectionResult {
        active: true,
        app_name: Some(app_name.to_string()),
        process_name: Some(process_name.to_string()),
        window_title: Some(window_title.to_string()),
        policy,
    })
}

pub fn detect_active_call(config: &MeetingsConfig) -> CallDetectionResult {
    if !config.call_detection_enabled {
        return CallDetectionResult {
            active: false,
            app_name: None,
            process_name: None,
            window_title: None,
            policy: MeetingAppPolicy::Ask,
        };
    }

    #[cfg(target_os = "windows")]
    {
        for (proc_name, title) in enumerate_visible_windows_windows() {
            if let Some(hit) = match_call_window(&proc_name, &title, config) {
                return hit;
            }
        }
    }

    CallDetectionResult {
        active: false,
        app_name: None,
        process_name: None,
        window_title: None,
        policy: MeetingAppPolicy::Ask,
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn enumerate_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    // SAFETY: `hwnd` is a live top-level window handle passed synchronously by EnumWindows.
    if unsafe { IsWindowVisible(hwnd) } == 0 {
        return 1;
    }
    let mut title_buf = [0u16; 256];
    // SAFETY: `title_buf` is a writable stack buffer of length 256 passed to GetWindowTextW.
    let title_len = unsafe { GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32) };
    let title = if title_len > 0 {
        String::from_utf16_lossy(&title_buf[..title_len as usize])
    } else {
        String::new()
    };
    if title.trim().is_empty() {
        return 1;
    }

    let mut pid: u32 = 0;
    // SAFETY: `pid` points to stack storage for GetWindowThreadProcessId.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    let mut proc_name = String::new();
    if pid != 0 {
        // SAFETY: Opening limited query handle for discovered window PID, closed immediately after query.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !handle.is_null() {
            let mut path_buf = [0u16; 260];
            let mut size = path_buf.len() as u32;
            // SAFETY: `handle` is valid and `path_buf` has capacity `size`.
            let ok =
                unsafe { QueryFullProcessImageNameW(handle, 0, path_buf.as_mut_ptr(), &mut size) };
            // SAFETY: `handle` was opened by OpenProcess above.
            unsafe { CloseHandle(handle) };
            if ok != 0 && size > 0 {
                let full = String::from_utf16_lossy(&path_buf[..size as usize]);
                proc_name = full.rsplit('\\').next().unwrap_or("").to_string();
            }
        }
    }
    // SAFETY: `lparam` points to our local Vec<(String, String)> for the synchronous EnumWindows call.
    let list = unsafe { &mut *(lparam as *mut Vec<(String, String)>) };
    list.push((proc_name, title));
    1
}

#[cfg(target_os = "windows")]
fn enumerate_visible_windows_windows() -> Vec<(String, String)> {
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    let mut results: Vec<(String, String)> = Vec::new();
    let ptr = (&mut results as *mut Vec<(String, String)>) as LPARAM;
    // SAFETY: EnumWindows invokes enumerate_window_proc synchronously before returning, so `results` stays valid.
    unsafe {
        EnumWindows(Some(enumerate_window_proc), ptr);
    }
    results
}

/// Spawns WASAPI loopback capture on Windows (system playback stream).
fn spawn_loopback_capture(
    stop_flag: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<i16>>>,
) -> Option<(JoinHandle<()>, u32)> {
    #[cfg(target_os = "windows")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Result<u32, String>>(1);
        let handle = std::thread::Builder::new()
            .name("meeting-loopback".into())
            .spawn(move || {
                use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

                if wasapi::initialize_mta().ok().is_err() {
                    let _ = tx.send(Err("Failed to initialize COM MTA for loopback".to_string()));
                    return;
                }
                struct ComDrop;
                impl Drop for ComDrop {
                    fn drop(&mut self) {
                        wasapi::deinitialize();
                    }
                }
                let _com = ComDrop;

                let setup = (|| -> Result<_, String> {
                    let enumerator = DeviceEnumerator::new()
                        .map_err(|e| format!("Loopback enumerator failed: {e}"))?;
                    let device = enumerator
                        .get_default_device(&Direction::Render)
                        .map_err(|e| format!("Default render endpoint unavailable: {e}"))?;
                    let mut client = device
                        .get_iaudioclient()
                        .map_err(|e| format!("Loopback AudioClient failed: {e}"))?;
                    let desired = WaveFormat::new(
                        16,
                        16,
                        &SampleType::Int,
                        TARGET_SAMPLE_RATE as usize,
                        1,
                        None,
                    );
                    let (_, min_period) = client
                        .get_device_period()
                        .map_err(|e| format!("Loopback device period failed: {e}"))?;
                    let mode = StreamMode::PollingShared {
                        autoconvert: true,
                        buffer_duration_hns: min_period.max(200_000),
                    };
                    client
                        .initialize_client(&desired, &Direction::Capture, &mode)
                        .map_err(|e| format!("Loopback initialize failed: {e}"))?;
                    let buffer_frames = client
                        .get_buffer_size()
                        .map_err(|e| format!("Loopback buffer size failed: {e}"))?;
                    let capture = client
                        .get_audiocaptureclient()
                        .map_err(|e| format!("Loopback capture client failed: {e}"))?;
                    client
                        .start_stream()
                        .map_err(|e| format!("Loopback start_stream failed: {e}"))?;
                    Ok((
                        client,
                        capture,
                        buffer_frames,
                        desired.get_blockalign() as usize,
                    ))
                })();

                let (client, capture, buffer_frames, block_align) = match setup {
                    Ok(ok) => {
                        let _ = tx.send(Ok(TARGET_SAMPLE_RATE));
                        ok
                    }
                    Err(err) => {
                        let _ = tx.send(Err(err));
                        return;
                    }
                };

                let mut raw = vec![0u8; buffer_frames as usize * block_align.max(2)];
                while !stop_flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(LOOPBACK_POLL_MS));
                    loop {
                        let frames = match capture.get_next_packet_size() {
                            Ok(Some(n)) if n > 0 => n as usize,
                            _ => break,
                        };
                        let needed = frames * 2;
                        if raw.len() < needed {
                            raw.resize(needed, 0);
                        }
                        let Ok((read_frames, info)) = capture.read_from_device(&mut raw[..needed])
                        else {
                            break;
                        };
                        let count = read_frames as usize;
                        if count == 0 {
                            break;
                        }
                        let mut guard = samples.lock();
                        if guard.len() >= MAX_MEETING_SAMPLES {
                            break;
                        }
                        let room = MAX_MEETING_SAMPLES - guard.len();
                        let take = count.min(room);
                        let new_len = guard.len() + take;
                        if info.flags.silent {
                            guard.resize(new_len, 0);
                        } else {
                            guard.extend(
                                raw[..take * 2]
                                    .chunks_exact(2)
                                    .map(|b| i16::from_le_bytes([b[0], b[1]])),
                            );
                        }
                    }
                }
                let _ = client.stop_stream();
            })
            .ok()?;

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(rate)) => Some((handle, rate)),
            Ok(Err(err)) => {
                log::warn!("WASAPI loopback unavailable (recording mic-only): {err}");
                let _ = handle.join();
                None
            }
            Err(_) => {
                log::warn!("WASAPI loopback startup timed out (recording mic-only)");
                None
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (stop_flag, samples);
        None
    }
}

pub fn start_meeting(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    title: Option<String>,
    app_name: Option<String>,
) -> Result<MeetingStatusPayload, String> {
    if MEETING_PROCESSING.load(Ordering::Acquire) {
        return Err("Previous meeting is still processing".to_string());
    }
    if ACTIVE_MEETING.lock().is_some() {
        return Err("A meeting recording is already in progress".to_string());
    }
    if state.recording.recording.lock().is_some() {
        return Err("Stop active dictation before starting a meeting recording".to_string());
    }

    crate::services::audio_service::stop_preroll();

    let (selected_device, avoid_bt) = state.with_profile(|p| {
        (
            state.selected_input_device_name(),
            p.audio_behavior.avoid_bluetooth_microphone,
        )
    });
    crate::services::audio_service::set_avoid_bluetooth_microphone(avoid_bt);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let mic_samples = Arc::new(Mutex::new(Vec::with_capacity(
        TARGET_SAMPLE_RATE as usize * 60,
    )));
    let (mic_thread, mic_sample_rate) =
        match spawn_audio_capture_thread(stop_flag.clone(), mic_samples.clone(), selected_device) {
            Ok(result) => result,
            Err(error) => {
                // The idle ring was consumed above; put it back before bailing.
                crate::services::audio_service::restart_idle_preroll_if_eligible(state);
                return Err(format!(
                    "Failed to start meeting microphone capture: {error}"
                ));
            }
        };

    let loopback_samples = Arc::new(Mutex::new(Vec::with_capacity(
        TARGET_SAMPLE_RATE as usize * 60,
    )));
    let (loopback_thread, loopback_sample_rate) =
        match spawn_loopback_capture(stop_flag.clone(), loopback_samples.clone()) {
            Some((handle, rate)) => (Some(handle), rate),
            None => (None, TARGET_SAMPLE_RATE),
        };

    let sleep_stop = stop_flag.clone();
    let sleep_thread = std::thread::Builder::new()
        .name("meeting-sleep-guard".into())
        .spawn(move || {
            let _guard = PreventSleep::acquire();
            while !sleep_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .ok();

    let started_at = now_secs();
    let id = format!("mtg-{started_at}-{}", std::process::id());
    let resolved_title = title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match app_name.as_deref() {
            Some(app) if !app.trim().is_empty() => format!("{app} Meeting"),
            _ => "Meeting Recording".to_string(),
        });

    *ACTIVE_MEETING.lock() = Some(ActiveMeetingSession {
        id: id.clone(),
        title: resolved_title,
        app_name,
        started_at,
        stop_flag,
        mic_samples,
        mic_sample_rate,
        mic_thread: Some(mic_thread),
        loopback_samples,
        loopback_sample_rate,
        loopback_thread,
        sleep_thread,
    });

    let status = current_status();
    let _ = app_handle.emit("meeting-status", &status);
    crate::services::http_api_service::publish_stream_event(
        "meeting.started",
        serde_json::to_value(&status).unwrap_or_default(),
    );
    Ok(status)
}

fn non_silent_audio(samples: &[i16]) -> bool {
    if samples.len() < (TARGET_SAMPLE_RATE as usize / 4) {
        return false;
    }
    let peak = samples
        .iter()
        .step_by(4)
        .map(|&s| s.unsigned_abs())
        .max()
        .unwrap_or(0);
    peak >= 90
}

fn slice_pcm_ms(samples: &[i16], start_ms: u64, end_ms: u64) -> Vec<i16> {
    let sr = TARGET_SAMPLE_RATE as u64;
    let pad_samples = (sr * 150) / 1000;
    let raw_start = ((start_ms * sr) / 1000) as usize;
    let raw_end = (((end_ms * sr) / 1000) as usize).min(samples.len());
    let start = raw_start.saturating_sub(pad_samples as usize);
    let end = (raw_end + pad_samples as usize).min(samples.len());
    if start >= end {
        return Vec::new();
    }
    samples[start..end].to_vec()
}

pub fn merge_diarize_segments(
    raw: &[funasr_service::DiarizeSegment],
) -> Vec<funasr_service::DiarizeSegment> {
    let mut sorted = raw.to_vec();
    sorted.sort_by_key(|seg| seg.t0_ms);
    let mut merged: Vec<funasr_service::DiarizeSegment> = Vec::new();
    for seg in sorted {
        if seg.t1_ms <= seg.t0_ms {
            continue;
        }
        if let Some(last) = merged.last_mut() {
            if last.speaker_id == seg.speaker_id && seg.t0_ms.saturating_sub(last.t1_ms) <= 1200 {
                last.t1_ms = last.t1_ms.max(seg.t1_ms);
                last.p = last.p.max(seg.p);
                continue;
            }
        }
        merged.push(seg);
    }
    merged
}

pub fn format_timestamp_hms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

pub fn render_meeting_markdown(record: &MeetingRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", record.title));
    if let Some(app) = record.app_name.as_deref() {
        out.push_str(&format!("- **App:** {app}\n"));
    }
    out.push_str(&format!(
        "- **Duration:** {:.1}s\n- **Speakers:** {}\n- **Words:** {}\n\n",
        record.duration_sec, record.speaker_count, record.word_count
    ));
    if let Some(summary) = record.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## Notes & Summary\n\n");
        out.push_str(summary.trim());
        out.push_str("\n\n");
    }
    out.push_str("## Transcript\n\n");
    if record.segments.is_empty() {
        out.push_str(record.transcript_text.trim());
        out.push('\n');
    } else {
        for seg in &record.segments {
            out.push_str(&format!(
                "- **[{}] {}:** {}\n",
                format_timestamp_hms(seg.start_ms),
                seg.speaker,
                seg.text.trim()
            ));
        }
    }
    out
}

pub fn export_meeting_format(record: &MeetingRecord, format: &str) -> Result<String, String> {
    match format.trim().to_ascii_lowercase().as_str() {
        "markdown" | "md" => Ok(render_meeting_markdown(record)),
        "json" => serde_json::to_string_pretty(record)
            .map_err(|e| format!("Failed to serialize meeting JSON: {e}")),
        "srt" => {
            let mut out = String::new();
            for (idx, seg) in record.segments.iter().enumerate() {
                out.push_str(&format!(
                    "{}\n{} --> {}\n{}: {}\n\n",
                    idx + 1,
                    format_srt_time(seg.start_ms),
                    format_srt_time(seg.end_ms.max(seg.start_ms + 500)),
                    seg.speaker,
                    seg.text.trim()
                ));
            }
            if out.is_empty() && !record.transcript_text.trim().is_empty() {
                out.push_str(&format!(
                    "1\n00:00:00,000 --> {}\n{}\n",
                    format_srt_time((record.duration_sec * 1000.0) as u64),
                    record.transcript_text.trim()
                ));
            }
            Ok(out)
        }
        "vtt" => {
            let mut out = String::from("WEBVTT\n\n");
            for seg in &record.segments {
                out.push_str(&format!(
                    "{} --> {}\n<v {}>{}</v>\n\n",
                    format_vtt_time(seg.start_ms),
                    format_vtt_time(seg.end_ms.max(seg.start_ms + 500)),
                    seg.speaker,
                    seg.text.trim()
                ));
            }
            Ok(out)
        }
        other => Err(format!("Unsupported meeting export format: {other}")),
    }
}

fn format_srt_time(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let rem = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{rem:03}")
}

fn format_vtt_time(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let rem = ms % 1000;
    format!("{h:02}:{m:02}:{s:02}.{rem:03}")
}

async fn summarize_meeting_transcript(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    title: &str,
    transcript: &str,
) -> Option<String> {
    if transcript.trim().is_empty() || local_only::is_enabled() {
        return None;
    }
    let (llm_cfg, use_vocab, hot_words) = state.with_profile(|p| {
        (
            p.llm_provider.clone(),
            p.labs.meeting_notes_vocabulary,
            p.get_hot_word_texts(80),
        )
    });
    let endpoint = llm_provider::endpoint_for_config(&llm_cfg);
    let api_key = llm_provider::load_api_key_for_provider(app_handle, &endpoint.provider);
    if api_key.trim().is_empty() && endpoint.provider != "custom" && endpoint.provider != "magpie" {
        return None;
    }
    let vocab_note = if use_vocab && !hot_words.is_empty() {
        format!(
            "\nPrefer these domain spellings/terms when relevant: {}",
            hot_words.join(", ")
        )
    } else {
        String::new()
    };
    let system_prompt = format!(
        "You summarize meeting transcripts cleanly in Markdown. Produce concise sections:\n\
         ### Summary\n(2-4 bullet points)\n\n\
         ### Decisions\n(bullet points or 'None noted')\n\n\
         ### Action Items\n(checkbox bullets `- [ ] owner: task`){vocab_note}"
    );
    let user_prompt = format!("Meeting title: {title}\n\nTranscript:\n{transcript}");
    let options = LlmRequestOptions::default();
    let user_input = LlmUserInput {
        text: user_prompt,
        images: Vec::new(),
    };
    let body = llm_client::build_llm_body(&endpoint, &system_prompt, &user_input, options);
    let http_client = &state.http_client;
    match llm_client::send_llm_request(
        http_client,
        &endpoint,
        &api_key,
        &body,
        transcript.len(),
        Some(app_handle),
        options,
    )
    .await
    {
        Ok(summary) if !summary.trim().is_empty() => Some(summary.trim().to_string()),
        Ok(_) => None,
        Err(err) => {
            log::warn!("Meeting summary generation skipped: {err}");
            None
        }
    }
}

pub async fn stop_meeting(app_handle: &tauri::AppHandle) -> Result<MeetingRecord, String> {
    let mut session = {
        let mut guard = ACTIVE_MEETING.lock();
        guard
            .take()
            .ok_or_else(|| "No active meeting recording to stop".to_string())?
    };

    MEETING_PROCESSING.store(true, Ordering::Release);
    session.stop_flag.store(true, Ordering::Release);
    let _ = app_handle.emit("meeting-status", current_status());

    let mic_handle = session.mic_thread.take();
    let loopback_handle = session.loopback_thread.take();
    let sleep_handle = session.sleep_thread.take();

    let _ = tokio::task::spawn_blocking(move || {
        if let Some(h) = mic_handle {
            let _ = h.join();
        }
        if let Some(h) = loopback_handle {
            let _ = h.join();
        }
        if let Some(h) = sleep_handle {
            let _ = h.join();
        }
    })
    .await;

    let ended_at = now_secs();
    let raw_mic = std::mem::take(&mut *session.mic_samples.lock());
    let raw_loopback = std::mem::take(&mut *session.loopback_samples.lock());

    let mic_16k = if session.mic_sample_rate == TARGET_SAMPLE_RATE {
        raw_mic
    } else {
        resample_for_file(&raw_mic, session.mic_sample_rate).unwrap_or(raw_mic)
    };
    let loopback_16k = if session.loopback_sample_rate == TARGET_SAMPLE_RATE {
        raw_loopback
    } else {
        resample_for_file(&raw_loopback, session.loopback_sample_rate).unwrap_or(raw_loopback)
    };

    let duration_sec = ((mic_16k.len().max(loopback_16k.len()) as f64)
        / f64::from(TARGET_SAMPLE_RATE))
    .max(ended_at.saturating_sub(session.started_at) as f64);

    let state = app_handle.state::<AppState>();
    let (meetings_cfg, expire_audio_flag, emit_finished_event) = state.with_profile(|p| {
        (
            p.meetings.clone(),
            p.labs.expire_meeting_audio,
            p.labs.meeting_finished_event,
        )
    });

    let dir = meetings_dir();
    let _ = std::fs::create_dir_all(&dir);
    let keep_audio = !expire_audio_flag || meetings_cfg.keep_audio_days > 0;

    let mut mic_audio_path = None;
    let mut loopback_audio_path = None;

    let mic_wav = if non_silent_audio(&mic_16k) {
        encode_wav(&mic_16k, TARGET_SAMPLE_RATE).ok()
    } else {
        None
    };
    let loopback_wav = if non_silent_audio(&loopback_16k) {
        encode_wav(&loopback_16k, TARGET_SAMPLE_RATE).ok()
    } else {
        None
    };

    if keep_audio {
        if let Some(bytes) = mic_wav.as_ref() {
            let path = dir.join(format!("{}_mic.wav", session.id));
            if std::fs::write(&path, bytes).is_ok() {
                mic_audio_path = Some(path.to_string_lossy().to_string());
            }
        }
        if let Some(bytes) = loopback_wav.as_ref() {
            let path = dir.join(format!("{}_loopback.wav", session.id));
            if std::fs::write(&path, bytes).is_ok() {
                loopback_audio_path = Some(path.to_string_lossy().to_string());
            }
        }
    }

    let mut segments: Vec<MeetingSegment> = Vec::new();
    let mut seg_seq = 1usize;

    let mut transcription_errors: Vec<String> = Vec::new();

    if let Some(bytes) = mic_wav {
        match funasr_service::transcribe(state.inner(), bytes, app_handle).await {
            Ok(res) => {
                let text = res.text.trim().to_string();
                if !text.is_empty() {
                    segments.push(MeetingSegment {
                        id: format!("seg-{seg_seq}"),
                        speaker: "You".to_string(),
                        track: "mic".to_string(),
                        start_ms: 0,
                        end_ms: (duration_sec * 1000.0) as u64,
                        text,
                    });
                    seg_seq += 1;
                }
            }
            Err(error) => transcription_errors.push(format!("mic: {error}")),
        }
    }

    if let Some(bytes) = loopback_wav {
        let diarized = if state.engine.supports_diarization.load(Ordering::Acquire) {
            funasr_service::diarize_wav_bytes(state.inner(), bytes.clone(), app_handle)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let merged = merge_diarize_segments(&diarized);
        if !merged.is_empty() && merged.len() <= 40 {
            for turn in merged {
                let start_ms = turn.t0_ms;
                let end_ms = turn.t1_ms.max(turn.t0_ms + 400);
                let slice = slice_pcm_ms(&loopback_16k, start_ms, end_ms);
                if slice.len() < (TARGET_SAMPLE_RATE as usize / 4) {
                    continue;
                }
                if let Ok(slice_wav) = encode_wav(&slice, TARGET_SAMPLE_RATE) {
                    match funasr_service::transcribe(state.inner(), slice_wav, app_handle).await {
                        Ok(res) => {
                            let text = res.text.trim().to_string();
                            if !text.is_empty() {
                                segments.push(MeetingSegment {
                                    id: format!("seg-{seg_seq}"),
                                    speaker: format!("Speaker {}", turn.speaker_id + 1),
                                    track: "loopback".to_string(),
                                    start_ms,
                                    end_ms,
                                    text,
                                });
                                seg_seq += 1;
                            }
                        }
                        Err(error) => transcription_errors
                            .push(format!("speaker {}: {error}", turn.speaker_id + 1)),
                    }
                }
            }
        } else {
            match funasr_service::transcribe(state.inner(), bytes, app_handle).await {
                Ok(res) => {
                    let text = res.text.trim().to_string();
                    if !text.is_empty() {
                        segments.push(MeetingSegment {
                            id: format!("seg-{seg_seq}"),
                            speaker: "Remote".to_string(),
                            track: "loopback".to_string(),
                            start_ms: 0,
                            end_ms: (duration_sec * 1000.0) as u64,
                            text,
                        });
                    }
                }
                Err(error) => transcription_errors.push(format!("remote: {error}")),
            }
        }
    }

    segments.sort_by_key(|s| s.start_ms);

    let transcript_text = segments
        .iter()
        .map(|s| {
            format!(
                "[{}] {}: {}",
                format_timestamp_hms(s.start_ms),
                s.speaker,
                s.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let summary = if meetings_cfg.auto_summarize {
        summarize_meeting_transcript(app_handle, state.inner(), &session.title, &transcript_text)
            .await
    } else {
        None
    };

    let speaker_count = {
        let mut set = std::collections::HashSet::new();
        for s in &segments {
            set.insert(s.speaker.as_str());
        }
        set.len()
    };
    let word_count = transcript_text.split_whitespace().count();

    // A meeting whose every transcription attempt failed is not "completed":
    // surface the failure instead of storing an empty transcript as success.
    let status = if segments.is_empty() && !transcription_errors.is_empty() {
        "failed"
    } else {
        "completed"
    };
    let error = if transcription_errors.is_empty() {
        None
    } else {
        Some(transcription_errors.join("; "))
    };

    let mut record = MeetingRecord {
        id: session.id.clone(),
        title: session.title,
        app_name: session.app_name,
        status: status.to_string(),
        started_at: session.started_at,
        ended_at,
        duration_sec,
        mic_audio_path,
        loopback_audio_path,
        markdown_path: None,
        summary,
        transcript_text,
        segments,
        word_count,
        speaker_count,
        error,
    };

    if meetings_cfg.save_markdown_transcripts {
        let md_dir = meetings_cfg
            .markdown_output_dir
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.clone());
        if std::fs::create_dir_all(&md_dir).is_ok() {
            let md_path = md_dir.join(format!("{}.md", record.id));
            let rendered = render_meeting_markdown(&record);
            if paths::atomic_write(&md_path, rendered.as_bytes()).is_ok() {
                record.markdown_path = Some(md_path.to_string_lossy().to_string());
            }
        }
    }

    let mut all = load_meetings();
    all.insert(0, record.clone());
    if all.len() > MAX_MEETINGS_STORED {
        all.truncate(MAX_MEETINGS_STORED);
    }
    let _ = save_meetings(&all);

    cleanup_expired_meeting_audio(&meetings_cfg, expire_audio_flag);

    MEETING_PROCESSING.store(false, Ordering::Release);
    let _ = app_handle.emit("meeting-status", current_status());
    let _ = app_handle.emit("meeting-finished", &record);
    if emit_finished_event {
        crate::services::http_api_service::publish_stream_event(
            "meeting.finished",
            serde_json::to_value(&record).unwrap_or_default(),
        );
    }

    let (audio_behavior, labs, selected) = state.with_profile(|p| {
        (
            p.audio_behavior.clone(),
            p.labs.clone(),
            state.selected_input_device_name(),
        )
    });
    crate::services::audio_service::start_preroll_if_eligible(
        selected,
        &audio_behavior,
        &labs,
        false,
    );

    Ok(record)
}

pub fn delete_meeting_by_id(id: &str) -> Result<bool, String> {
    let mut records = load_meetings();
    let idx = match records.iter().position(|r| r.id == id) {
        Some(i) => i,
        None => return Ok(false),
    };
    let removed = records.remove(idx);
    for path in [
        removed.mic_audio_path.as_deref(),
        removed.loopback_audio_path.as_deref(),
        removed.markdown_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let p = Path::new(path);
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }
    save_meetings(&records)?;
    Ok(true)
}

pub async fn resummarize_meeting(
    app_handle: &tauri::AppHandle,
    id: &str,
) -> Result<MeetingRecord, String> {
    let mut records = load_meetings();
    let pos = records
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| format!("Meeting {id} not found"))?;
    let state = app_handle.state::<AppState>();
    let summary = summarize_meeting_transcript(
        app_handle,
        state.inner(),
        &records[pos].title,
        &records[pos].transcript_text,
    )
    .await
    .ok_or_else(|| {
        "Unable to generate AI summary (check LLM key or local-only mode)".to_string()
    })?;
    records[pos].summary = Some(summary);
    let updated = records[pos].clone();
    save_meetings(&records)?;
    Ok(updated)
}

pub fn cleanup_expired_meeting_audio(config: &MeetingsConfig, expire_enabled: bool) {
    if !expire_enabled || config.keep_audio_days == 0 {
        return;
    }
    let cutoff = now_secs().saturating_sub(u64::from(config.keep_audio_days) * 86_400);
    let mut records = load_meetings();
    let mut changed = false;
    for rec in &mut records {
        if rec.ended_at > 0 && rec.ended_at < cutoff {
            if let Some(p) = rec.mic_audio_path.take() {
                let _ = std::fs::remove_file(p);
                changed = true;
            }
            if let Some(p) = rec.loopback_audio_path.take() {
                let _ = std::fs::remove_file(p);
                changed = true;
            }
        }
    }
    if changed {
        let _ = save_meetings(&records);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_adjacent_same_speaker_diarize_segments() {
        let raw = vec![
            funasr_service::DiarizeSegment {
                speaker_id: 0,
                t0_ms: 0,
                t1_ms: 2000,
                p: 0.9,
            },
            funasr_service::DiarizeSegment {
                speaker_id: 0,
                t0_ms: 2500,
                t1_ms: 4500,
                p: 0.95,
            },
            funasr_service::DiarizeSegment {
                speaker_id: 1,
                t0_ms: 4800,
                t1_ms: 7000,
                p: 0.88,
            },
        ];
        let merged = merge_diarize_segments(&raw);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].speaker_id, 0);
        assert_eq!(merged[0].t1_ms, 4500);
        assert_eq!(merged[1].speaker_id, 1);
    }

    #[test]
    fn call_detection_respects_more_apps_and_never_policy() {
        let mut cfg = MeetingsConfig::default();
        assert!(match_call_window("Zoom.exe", "Zoom Meeting", &cfg).is_some());
        assert!(match_call_window("Loom.exe", "Loom Recorder", &cfg).is_none());

        cfg.more_call_apps = true;
        assert!(match_call_window("Loom.exe", "Loom Recorder", &cfg).is_some());

        cfg.app_policies
            .insert("Zoom".to_string(), MeetingAppPolicy::Never);
        assert!(match_call_window("Zoom.exe", "Zoom Meeting", &cfg).is_none());
    }

    #[test]
    fn exports_markdown_srt_and_vtt_cleanly() {
        let rec = MeetingRecord {
            id: "mtg-1".into(),
            title: "Design Review".into(),
            app_name: Some("Zoom".into()),
            status: "completed".into(),
            started_at: 100,
            ended_at: 160,
            duration_sec: 60.0,
            mic_audio_path: None,
            loopback_audio_path: None,
            markdown_path: None,
            summary: Some("### Summary\n- Shipped WASAPI capture".into()),
            transcript_text: "[00:00] You: Hello\n[00:05] Remote: Ready".into(),
            segments: vec![
                MeetingSegment {
                    id: "1".into(),
                    speaker: "You".into(),
                    track: "mic".into(),
                    start_ms: 0,
                    end_ms: 4000,
                    text: "Hello".into(),
                },
                MeetingSegment {
                    id: "2".into(),
                    speaker: "Remote".into(),
                    track: "loopback".into(),
                    start_ms: 5000,
                    end_ms: 9000,
                    text: "Ready".into(),
                },
            ],
            word_count: 8,
            speaker_count: 2,
            error: None,
        };

        let md = export_meeting_format(&rec, "markdown").unwrap();
        assert!(md.contains("# Design Review"));
        assert!(md.contains("Shipped WASAPI capture"));

        let srt = export_meeting_format(&rec, "srt").unwrap();
        assert!(srt.contains("00:00:00,000 --> 00:00:04,000"));
        assert!(srt.contains("You: Hello"));

        let vtt = export_meeting_format(&rec, "vtt").unwrap();
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("<v Remote>Ready</v>"));
    }
}
