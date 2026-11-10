//! Background audio ducking, session muting, media auto-pause, and idle-sleep
//! prevention during active dictation and meetings.
//!
//! Uses a dedicated MTA COM/WinRT worker thread so recording startup and finish
//! never block on Windows WASAPI/GSMTC calls.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::state::user_profile::{AudioBehaviorConfig, AudioDuckingMode};

const TICK: Duration = Duration::from_millis(16);
const EPSILON: f32 = 0.002;
const LOWER_DURATION: Duration = Duration::from_millis(120);
const RESTORE_DURATION: Duration = Duration::from_secs(1);
const SETTLE_DURATION: Duration = Duration::from_millis(500);

static SLEEP_HOLD_COUNT: AtomicUsize = AtomicUsize::new(0);
static BEHAVIOR_SNAPSHOT: Mutex<Option<AudioBehaviorConfig>> = Mutex::new(None);

/// Syncs the latest user audio behavior configuration into the audio ducking subsystem.
pub fn update_audio_behavior(config: AudioBehaviorConfig) {
    *BEHAVIOR_SNAPSHOT.lock() = Some(config);
}

#[cfg(test)]
fn current_audio_behavior() -> AudioBehaviorConfig {
    BEHAVIOR_SNAPSHOT.lock().clone().unwrap_or_default()
}

/// Prevents Windows idle system sleep while held (for dictations and meetings).
pub struct PreventSleep {
    #[cfg(target_os = "windows")]
    active: bool,
}

impl PreventSleep {
    pub fn acquire() -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            use windows_sys::Win32::System::Power::{
                SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
            };
            // SAFETY: SetThreadExecutionState takes only bitflags and returns the previous state.
            let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            let active = prev != 0;
            if active {
                SLEEP_HOLD_COUNT.fetch_add(1, Ordering::AcqRel);
            }
            Some(Self { active })
        }
        #[cfg(not(target_os = "windows"))]
        {
            SLEEP_HOLD_COUNT.fetch_add(1, Ordering::AcqRel);
            Some(Self {})
        }
    }
}

impl Drop for PreventSleep {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        if self.active {
            let remaining = SLEEP_HOLD_COUNT.fetch_sub(1, Ordering::AcqRel) - 1;
            if remaining == 0 {
                use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};
                // SAFETY: Clearing ES_SYSTEM_REQUIRED while keeping ES_CONTINUOUS restores normal sleep idle tracking.
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS);
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            SLEEP_HOLD_COUNT.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

// ---------- Volume ramp engine (adapted cleanly for WASAPI ISimpleAudioVolume) ----------

pub(crate) trait VolumeSet {
    fn read(&self) -> Option<Vec<f32>>;
    fn write(&mut self, values: &[f32]) -> bool;
}

pub(crate) struct VolumeRampGuard {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl VolumeRampGuard {
    pub(crate) fn start<V: VolumeSet>(
        factor: f32,
        open: impl FnOnce() -> Option<V> + Send + 'static,
    ) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("output-ducking".into())
            .spawn(move || {
                if let Some(output) = open() {
                    if let Some(mut ramp) = Ramp::new(output) {
                        run_ramp(&mut ramp, factor, receiver);
                    }
                }
            })
            .ok()?;
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }

    pub(crate) fn restore(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for VolumeRampGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn run_ramp<V: VolumeSet>(ramp: &mut Ramp<V>, factor: f32, receiver: Receiver<()>) {
    ramp.retarget(Some(factor));
    let started = Instant::now();
    loop {
        match receiver.recv_timeout(TICK) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => ramp.tick(started.elapsed().as_secs_f32()),
        }
        if ramp.finished {
            return;
        }
    }
    ramp.retarget(None);
    let started = Instant::now();
    while !ramp.finished {
        ramp.tick(started.elapsed().as_secs_f32());
        if !ramp.finished {
            thread::sleep(TICK);
        }
    }
}

struct Ramp<V: VolumeSet> {
    output: V,
    original: Vec<f32>,
    accepted: Vec<f32>,
    from: Vec<f32>,
    target: Vec<f32>,
    restoring: bool,
    settled: bool,
    finished: bool,
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < EPSILON)
}

impl<V: VolumeSet> Ramp<V> {
    fn new(output: V) -> Option<Self> {
        let original = output.read().filter(|values| !values.is_empty())?;
        Some(Self {
            output,
            accepted: original.clone(),
            from: original.clone(),
            target: original.clone(),
            original,
            restoring: false,
            settled: false,
            finished: false,
        })
    }

    fn retarget(&mut self, factor: Option<f32>) {
        if self.finished {
            return;
        }
        if self.settled
            && !self
                .output
                .read()
                .is_some_and(|v| close(&v, &self.accepted))
        {
            // Respect manual user volume adjustments made while ducked.
            self.finished = true;
            return;
        }
        self.from = self.output.read().unwrap_or_else(|| self.accepted.clone());
        self.target = self
            .original
            .iter()
            .map(|v| (v * factor.unwrap_or(1.0)).clamp(0.0, 1.0))
            .collect();
        self.restoring = factor.is_none();
        self.settled = false;
    }

    fn duration(&self) -> Duration {
        if self.restoring {
            RESTORE_DURATION
        } else {
            LOWER_DURATION
        }
    }

    fn tick(&mut self, seconds: f32) {
        if self.finished || self.settled {
            return;
        }
        let duration = self.duration().as_secs_f32();
        let t = (seconds / duration).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let values: Vec<f32> = self
            .from
            .iter()
            .zip(&self.target)
            .map(|(a, b)| a + (b - a) * eased)
            .collect();
        if !self.output.write(&values) {
            self.rollback();
            return;
        }
        let Some(actual) = self.output.read() else {
            self.rollback();
            return;
        };
        self.accepted = actual;
        if t >= 1.0
            && (close(&self.accepted, &self.target)
                || seconds >= duration + SETTLE_DURATION.as_secs_f32())
        {
            self.settled = true;
            self.finished = self.restoring;
        }
    }

    fn rollback(&mut self) {
        self.output.write(&self.original);
        self.finished = true;
    }
}

// ---------- Output route classification (speakers vs headphones) ----------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputRoute {
    Speakers,
    Headphones,
    Unknown,
}

pub(crate) fn output_route_applies(route: OutputRoute) -> bool {
    route != OutputRoute::Headphones
}

pub(crate) fn classify_endpoint_name_as_route(name: &str) -> Option<OutputRoute> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("headphone")
        || lower.contains("headset")
        || lower.contains("earbud")
        || lower.contains("airpods")
        || lower.contains("earpiece")
    {
        Some(OutputRoute::Headphones)
    } else if lower.contains("speaker") || lower.contains("soundbar") {
        Some(OutputRoute::Speakers)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn default_output_route_windows() -> OutputRoute {
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, Handset, Headphones, Headset, IMMDeviceEnumerator,
        MMDeviceEnumerator, PKEY_AudioEndpoint_FormFactor, Speakers,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToUInt32;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

    let result = (|| -> windows::core::Result<OutputRoute> {
        // SAFETY: MMDeviceEnumerator is a standard system COM class; COM is initialized on the worker.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        // SAFETY: `enumerator` is a valid COM pointer obtained above.
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
        // SAFETY: `device` is a valid IMMDevice COM object opened for read-only properties.
        let store = unsafe { device.OpenPropertyStore(STGM_READ)? };
        // SAFETY: `store` is a valid IPropertyStore opened above.
        if let Ok(prop) = unsafe { store.GetValue(&PKEY_AudioEndpoint_FormFactor) } {
            // SAFETY: `prop` is an initialized PROPVARIANT returned by `GetValue`.
            if let Ok(form_factor) = unsafe { PropVariantToUInt32(&prop) } {
                let ff = form_factor as i32;
                if ff == Headphones.0 || ff == Headset.0 || ff == Handset.0 {
                    return Ok(OutputRoute::Headphones);
                }
                if ff == Speakers.0 {
                    return Ok(OutputRoute::Speakers);
                }
            }
        }
        // SAFETY: `store` is a valid IPropertyStore opened above.
        if let Ok(prop) = unsafe { store.GetValue(&PKEY_Device_FriendlyName) } {
            let name = prop.to_string();
            if let Some(route) = classify_endpoint_name_as_route(&name) {
                return Ok(route);
            }
        }
        Ok(OutputRoute::Unknown)
    })();

    match result {
        Ok(route) => route,
        Err(error) => {
            log::debug!("Unable to read Windows default audio output route: {error}");
            OutputRoute::Unknown
        }
    }
}

// ---------- Windows COM / WinRT suppression backend ----------

#[cfg(target_os = "windows")]
mod win_backend {
    use super::*;
    use windows::core::Interface;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    fn wait_async<T: windows::core::RuntimeType>(
        operation: windows_future::IAsyncOperation<T>,
    ) -> windows::core::Result<T> {
        let deadline = Instant::now() + Duration::from_secs(3);
        while operation.Status()? == windows_future::AsyncStatus::Started {
            if Instant::now() >= deadline {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                    -2147023436i32, // ERROR_TIMEOUT
                )));
            }
            thread::sleep(Duration::from_millis(2));
        }
        operation.GetResults()
    }

    enum Suppressed {
        MutedSessions(Vec<ISimpleAudioVolume>),
        Ducked(VolumeRampGuard),
        PausedMedia(Vec<GlobalSystemMediaTransportControlsSession>),
    }

    struct Held {
        suppressed: Option<Suppressed>,
        _sleep: Option<PreventSleep>,
    }

    pub enum Command {
        Suppress(AudioBehaviorConfig),
        Restore,
    }

    struct WindowsSessions(Vec<ISimpleAudioVolume>);

    impl VolumeSet for WindowsSessions {
        fn read(&self) -> Option<Vec<f32>> {
            self.0
                .iter()
                // SAFETY: Each `ISimpleAudioVolume` is a live COM interface pointer owned by `self.0`.
                .map(|volume| unsafe { volume.GetMasterVolume() }.ok())
                .collect()
        }

        fn write(&mut self, values: &[f32]) -> bool {
            self.0.iter().zip(values).fold(true, |ok, (vol, &val)| {
                // SAFETY: `vol` is a live `ISimpleAudioVolume` COM pointer and `event_context` may be null.
                let res = unsafe { vol.SetMasterVolume(val.clamp(0.0, 1.0), std::ptr::null()) };
                ok & res.is_ok()
            })
        }
    }

    fn other_render_sessions() -> windows::core::Result<Vec<ISimpleAudioVolume>> {
        // SAFETY: COM is initialized on the calling worker thread; MMDeviceEnumerator is a system COM class.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        // SAFETY: `enumerator` is a live COM interface obtained above.
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
        // SAFETY: `device` is a live IMMDevice interface.
        let manager: IAudioSessionManager2 = unsafe { device.Activate(CLSCTX_ALL, None)? };
        // SAFETY: `manager` is a live IAudioSessionManager2 interface.
        let sessions = unsafe { manager.GetSessionEnumerator()? };
        // SAFETY: `sessions` is a live IAudioSessionEnumerator interface.
        let count = unsafe { sessions.GetCount()? };
        let own_pid = std::process::id();
        let mut volumes = Vec::new();
        for index in 0..count {
            // SAFETY: `index` is strictly below `count` from `GetCount()`.
            let Ok(control) = (unsafe { sessions.GetSession(index) }) else {
                continue;
            };
            if let Ok(control2) = control.cast::<IAudioSessionControl2>() {
                // SAFETY: `control2` is a live IAudioSessionControl2 interface.
                if let Ok(pid) = unsafe { control2.GetProcessId() } {
                    if pid == own_pid {
                        continue;
                    }
                }
            }
            if let Ok(volume) = control.cast::<ISimpleAudioVolume>() {
                volumes.push(volume);
            }
        }
        Ok(volumes)
    }

    fn mute_other_sessions() -> Option<Suppressed> {
        let muted = other_render_sessions().map(|sessions| {
            sessions
                .into_iter()
                .filter(|vol| {
                    // SAFETY: `vol` is a live ISimpleAudioVolume interface pointer.
                    let already_muted =
                        unsafe { vol.GetMute() }.map_or(true, |muted| muted.as_bool());
                    // SAFETY: `vol` is a live ISimpleAudioVolume interface and event context is optional.
                    !already_muted && unsafe { vol.SetMute(true, std::ptr::null()) }.is_ok()
                })
                .collect::<Vec<_>>()
        });
        match muted {
            Ok(list) if !list.is_empty() => Some(Suppressed::MutedSessions(list)),
            Ok(_) => None,
            Err(error) => {
                log::warn!("Failed to mute background Windows audio sessions: {error}");
                None
            }
        }
    }

    fn duck_other_sessions(factor: f32) -> Option<Suppressed> {
        VolumeRampGuard::start(factor.clamp(0.0, 1.0), || {
            // SAFETY: Initializes MTA COM on this dedicated ramp worker thread if not already initialized.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            match other_render_sessions() {
                Ok(sessions) if !sessions.is_empty() => Some(WindowsSessions(sessions)),
                Ok(_) => None,
                Err(error) => {
                    log::warn!("Failed to enumerate Windows audio sessions for ducking: {error}");
                    None
                }
            }
        })
        .map(Suppressed::Ducked)
    }

    fn pause_playing_media() -> Option<Suppressed> {
        let paused =
            (|| -> windows::core::Result<Vec<GlobalSystemMediaTransportControlsSession>> {
                let manager =
                    wait_async(GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?)?;
                let mut paused = Vec::new();
                for session in manager.GetSessions()? {
                    let playing = session
                        .GetPlaybackInfo()
                        .and_then(|info| info.PlaybackStatus())
                        == Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
                    if playing
                        && session
                            .TryPauseAsync()
                            .and_then(wait_async)
                            .unwrap_or(false)
                    {
                        paused.push(session);
                    }
                }
                Ok(paused)
            })();
        match paused {
            Ok(list) if !list.is_empty() => Some(Suppressed::PausedMedia(list)),
            Ok(_) => None,
            Err(error) => {
                log::warn!("Failed to pause Windows GSMTC media playback: {error}");
                None
            }
        }
    }

    fn apply_suppression(cfg: &AudioBehaviorConfig) -> Option<Suppressed> {
        if cfg.ducking_mode == AudioDuckingMode::Off {
            return None;
        }
        if cfg.speakers_only && !output_route_applies(default_output_route_windows()) {
            log::info!("Skipped audio ducking because output route is headphones/headset");
            return None;
        }
        match cfg.ducking_mode {
            AudioDuckingMode::Off => None,
            AudioDuckingMode::Duck => duck_other_sessions(cfg.duck_volume),
            AudioDuckingMode::Mute => mute_other_sessions(),
            AudioDuckingMode::PauseMedia => pause_playing_media(),
        }
    }

    fn restore_held(held: Option<Held>) {
        let Some(mut held) = held else {
            return;
        };
        if let Some(suppressed) = held.suppressed.take() {
            match suppressed {
                Suppressed::MutedSessions(sessions) => {
                    for vol in sessions {
                        // SAFETY: `vol` is a live ISimpleAudioVolume interface captured earlier.
                        let _ = unsafe { vol.SetMute(false, std::ptr::null()) };
                    }
                }
                Suppressed::Ducked(mut guard) => {
                    guard.restore();
                }
                Suppressed::PausedMedia(sessions) => {
                    for session in sessions {
                        if let Ok(op) = session.TryPlayAsync() {
                            let _ = wait_async(op);
                        }
                    }
                }
            }
        }
    }

    pub fn spawn_worker() -> Sender<Command> {
        let (tx, rx) = mpsc::channel::<Command>();
        let _ = thread::Builder::new()
            .name("windows-audio-suppressor".into())
            .spawn(move || {
                // SAFETY: Initializes MTA COM on this dedicated background worker thread.
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let mut held: Option<Held> = None;
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Command::Suppress(cfg) => {
                            if held.is_none() {
                                let suppressed = apply_suppression(&cfg);
                                let _sleep = PreventSleep::acquire();
                                held = Some(Held { suppressed, _sleep });
                            }
                        }
                        Command::Restore => {
                            restore_held(held.take());
                        }
                    }
                }
                restore_held(held.take());
            });
        tx
    }
}

#[cfg(target_os = "windows")]
static WORKER_TX: std::sync::LazyLock<Sender<win_backend::Command>> =
    std::sync::LazyLock::new(win_backend::spawn_worker);

/// Manages temporary audio ducking, media pause, and sleep prevention for active dictation.
#[derive(Debug, Default)]
pub struct AudioDuckingSession {
    active: AtomicBool,
}

impl AudioDuckingSession {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
        }
    }

    /// Suppress background audio using the provided configuration when recording begins.
    pub fn start_with_config(&self, config: &AudioBehaviorConfig) {
        if self.active.swap(true, Ordering::SeqCst) {
            return;
        }
        #[cfg(target_os = "windows")]
        {
            let _ = WORKER_TX.send(win_backend::Command::Suppress(config.clone()));
            log::info!(
                "Audio suppression engaged (mode={:?}, duck_volume={:.2}, speakers_only={})",
                config.ducking_mode,
                config.duck_volume,
                config.speakers_only
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = config;
        }
    }

    #[cfg(test)]
    pub fn start_ducking(&self) {
        let cfg = current_audio_behavior();
        self.start_with_config(&cfg);
    }

    /// Restore background audio or resume media when recording concludes.
    pub fn restore(&self) {
        if !self.active.swap(false, Ordering::SeqCst) {
            return;
        }
        #[cfg(target_os = "windows")]
        {
            let _ = WORKER_TX.send(win_backend::Command::Restore);
            log::info!("Audio suppression restore requested");
        }
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
}

/// Global audio ducking controller shared across recording sessions.
pub static AUDIO_DUCKING: AudioDuckingSession = AudioDuckingSession::new();

pub fn duck_audio_with_config(config: &AudioBehaviorConfig) {
    update_audio_behavior(config.clone());
    AUDIO_DUCKING.start_with_config(config);
}

pub fn restore_audio() {
    AUDIO_DUCKING.restore();
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeVolumes {
        values: Vec<f32>,
        fail: bool,
    }

    impl VolumeSet for FakeVolumes {
        fn read(&self) -> Option<Vec<f32>> {
            Some(self.values.clone())
        }

        fn write(&mut self, values: &[f32]) -> bool {
            if std::mem::take(&mut self.fail) {
                return false;
            }
            self.values = values
                .iter()
                .map(|v| (v * 1000.0).round() / 1000.0)
                .collect();
            true
        }
    }

    #[test]
    fn test_ducking_session_lifecycle() {
        let session = AudioDuckingSession::new();
        assert!(!session.is_active());

        session.start_with_config(&AudioBehaviorConfig {
            ducking_mode: AudioDuckingMode::Off,
            ..Default::default()
        });
        assert!(session.is_active());

        // Repeated call is idempotent
        session.start_ducking();
        assert!(session.is_active());

        session.restore();
        assert!(!session.is_active());

        // Repeated restore is idempotent
        session.restore();
        assert!(!session.is_active());
    }

    #[test]
    fn volume_ramp_lowers_and_restores_preserving_balance() {
        let mut ramp = Ramp::new(FakeVolumes {
            values: vec![0.4, 0.8],
            fail: false,
        })
        .unwrap();
        ramp.retarget(Some(0.25));
        ramp.tick(LOWER_DURATION.as_secs_f32());
        assert!(close(&ramp.output.values, &[0.1, 0.2]));
        assert!(ramp.settled);

        ramp.retarget(None);
        ramp.tick(RESTORE_DURATION.as_secs_f32());
        assert!(close(&ramp.output.values, &[0.4, 0.8]));
        assert!(ramp.finished);
    }

    #[test]
    fn volume_ramp_respects_manual_override_and_rolls_back_on_error() {
        let mut ramp = Ramp::new(FakeVolumes {
            values: vec![0.5, 0.5],
            fail: false,
        })
        .unwrap();
        ramp.retarget(Some(0.2));
        ramp.tick(LOWER_DURATION.as_secs_f32());
        // Simulate user manually dragging the volume slider during dictation.
        ramp.output.values = vec![0.75, 0.75];
        ramp.retarget(None);
        ramp.tick(RESTORE_DURATION.as_secs_f32());
        assert_eq!(ramp.output.values, vec![0.75, 0.75]);

        let mut fail_ramp = Ramp::new(FakeVolumes {
            values: vec![0.6, 0.9],
            fail: true,
        })
        .unwrap();
        fail_ramp.retarget(Some(0.2));
        fail_ramp.tick(LOWER_DURATION.as_secs_f32() / 2.0);
        assert!(fail_ramp.finished);
        assert!(close(&fail_ramp.output.values, &[0.6, 0.9]));
    }

    #[test]
    fn speakers_only_classifies_routes_cleanly() {
        assert!(output_route_applies(OutputRoute::Speakers));
        assert!(output_route_applies(OutputRoute::Unknown));
        assert!(!output_route_applies(OutputRoute::Headphones));
        assert_eq!(
            classify_endpoint_name_as_route("AirPods Pro Hands-Free"),
            Some(OutputRoute::Headphones)
        );
        assert_eq!(
            classify_endpoint_name_as_route("Realtek Speakers"),
            Some(OutputRoute::Speakers)
        );
    }
}
