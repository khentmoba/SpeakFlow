#[cfg(test)]
use crate::commands::audio::{
    RECORDING_ALREADY_ACTIVE_ERROR, RECORDING_NOT_READY_ERROR, RECORDING_START_CANCELLED_ERROR,
};
use crate::state::{AppState, RecordingSlot, RecordingTrigger};
use crate::utils::AppError;
#[cfg(target_os = "windows")]
use parking_lot::Mutex;
#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicU32;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, OnceLock,
};
#[cfg(target_os = "windows")]
use std::thread::JoinHandle;
use tauri::{Emitter, Manager};
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
    VK_F1, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_SPACE, VK_TAB, VK_UP,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_NULL, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN,
    WM_XBUTTONUP,
};

mod dispatch;
mod monitor;
mod reg_backend;
mod spec;
mod start_owner;
mod unified_hook;

#[cfg(test)]
use dispatch::is_ignorable_start_audio_error;
use dispatch::{
    dispatch_hotkey_press, dispatch_hotkey_release, handle_hotkey_stop,
    handle_meeting_toggle_action, handle_terminal_paste_action, handle_typed_text_action,
};
use monitor::*;
use reg_backend::*;
use spec::*;
pub(crate) use start_owner::HotkeyStartOwner;
use unified_hook::*;

const HOTKEY_REPRESS_DEBOUNCE_MS: u64 = 180;

/// RegisterHotKey probe uses this atom ID
#[cfg(target_os = "windows")]
const PROBE_HOTKEY_ID: i32 = 0x4C57; // "LW"

/// Counter for injected events ignored by the LLKH filter
#[cfg(target_os = "windows")]
static IGNORED_INJECTED_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// HotkeyBackend — selects between RegisterHotKey and low-level hook
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HotkeyBackend {
    /// Standard Windows RegisterHotKey — most stable, toggle-only
    RegisterHotKey,
    /// WH_KEYBOARD_LL — for hold mode, modifier-only, Win combos
    LowLevelHook,
}

fn classify_backend(kind: HotkeyKind, spec: &HotkeySpec) -> HotkeyBackend {
    if spec.force_low_level_hook() {
        return HotkeyBackend::LowLevelHook;
    }
    if is_share_dictation_shortcut() && kind.is_dictation_passthrough_eligible() {
        return HotkeyBackend::LowLevelHook;
    }
    if is_toggle_mode() || !kind.is_recording_trigger() {
        HotkeyBackend::RegisterHotKey
    } else {
        // Hold / tap-or-hold needs LLKH for key-up detection
        HotkeyBackend::LowLevelHook
    }
}

// ---------------------------------------------------------------------------
// Dispatch channel — moves heavy work off the hook thread
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
enum DispatchEvent {
    Press(Arc<UnifiedHookState>, String),
    Release(Arc<UnifiedHookState>, String),
    Cancel(Arc<UnifiedHookState>, String),
    PasteLast(Arc<UnifiedHookState>),
    TerminalPaste(tauri::AppHandle),
    TypedText(tauri::AppHandle),
    MeetingToggle(tauri::AppHandle),
}

#[cfg(target_os = "windows")]
fn dispatch_channel() -> &'static std::sync::mpsc::Sender<DispatchEvent> {
    static TX: OnceLock<std::sync::mpsc::Sender<DispatchEvent>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<DispatchEvent>();
        std::thread::Builder::new()
            .name("hotkey-dispatch".into())
            .spawn(move || {
                for event in rx {
                    match event {
                        DispatchEvent::Press(state, msg) => {
                            dispatch_hotkey_press(
                                &state.app_handle,
                                &state.gate,
                                state.trigger,
                                &msg,
                                state.spec.label(),
                            );
                        }
                        DispatchEvent::Release(state, msg) => {
                            dispatch_hotkey_release(
                                &state.app_handle,
                                &state.gate,
                                state.trigger,
                                &msg,
                                state.spec.label(),
                            );
                        }
                        DispatchEvent::Cancel(state, msg) => {
                            log::info!("{msg}");
                            let app = state.app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let app_state = app.state::<AppState>();
                                let recording = app_state.recording.recording.lock().take();
                                if let Some(slot) = recording {
                                    match slot {
                                        RecordingSlot::Starting(pending) => {
                                            pending.stop_flag.store(true, Ordering::Release);
                                            pending.stop_notify.notify_waiters();
                                        }
                                        RecordingSlot::Active(session) => {
                                            session.stop_flag.store(true, Ordering::Release);
                                            crate::services::audio_service::discard_recording(
                                                session,
                                            )
                                            .await;
                                        }
                                    }
                                }
                                crate::services::audio_service::restore_audio();
                                // The ring was consumed by this attempt; arm it again.
                                crate::services::audio_service::restart_idle_preroll_if_eligible(
                                    &app.state::<crate::state::AppState>(),
                                );
                                crate::commands::window::hide_subtitle_window_inner(&app).ok();
                            });
                        }
                        DispatchEvent::PasteLast(state) => {
                            let app = state.app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = crate::commands::clipboard::paste_last_dictation(app).await;
                            });
                        }
                        DispatchEvent::TerminalPaste(app) => {
                            handle_terminal_paste_action(app);
                        }
                        DispatchEvent::TypedText(app) => {
                            handle_typed_text_action(app);
                        }
                        DispatchEvent::MeetingToggle(app) => {
                            handle_meeting_toggle_action(app);
                        }
                    }
                }
            })
            .expect("failed to start hotkey dispatch worker");
        tx
    })
}

/// Send dispatch event from hook callback — never blocks.
#[cfg(target_os = "windows")]
fn send_dispatch(event: DispatchEvent) {
    let _ = dispatch_channel().send(event);
}

/// Probe whether a hotkey is already registered system-wide via RegisterHotKey.
/// Returns a human-readable warning if conflicting, None otherwise.
#[cfg(target_os = "windows")]
fn probe_system_hotkey_conflict(spec: &HotkeySpec) -> Option<String> {
    let (win_mods, vk) = match spec {
        HotkeySpec::Standard {
            modifiers, main_vk, ..
        } if !spec.force_low_level_hook() => {
            let mut m = 0u32;
            if modifiers.has_ctrl() {
                m |= MOD_CONTROL;
            }
            if modifiers.has_alt() {
                m |= MOD_ALT;
            }
            if modifiers.has_shift() {
                m |= MOD_SHIFT;
            }
            if modifiers.has_super() {
                m |= MOD_WIN;
            }
            (m, *main_vk as u32)
        }
        // Modifier-only, side-specific, and consumed lock keys require LLKH.
        _ => return None,
    };

    // Attempt to register — success means no conflict
    // SAFETY: RegisterHotKey takes only integer arguments; a null window binds the probe hotkey to this thread's queue.
    let ok = unsafe { RegisterHotKey(std::ptr::null_mut(), PROBE_HOTKEY_ID, win_mods, vk) };
    if ok != 0 {
        // SAFETY: Runs only after the probe RegisterHotKey succeeded on this thread with the same id, so this releases that registration.
        unsafe { UnregisterHotKey(std::ptr::null_mut(), PROBE_HOTKEY_ID) };
        None
    } else {
        Some(format!(
            "Shortcut {} may already be used by another application and could trigger both actions",
            spec.label()
        ))
    }
}

// ---------------------------------------------------------------------------

struct HotkeyEventGate {
    is_pressed: AtomicBool,
    last_release_ms: AtomicU64,
    pressed_at_ms: AtomicU64,
    tap_locked: AtomicBool,
    toggle_active: AtomicBool,
    start_owner: Arc<HotkeyStartOwner>,
    transition: parking_lot::Mutex<()>,
    registered: AtomicBool,
}

impl Default for HotkeyEventGate {
    fn default() -> Self {
        Self {
            is_pressed: AtomicBool::new(false),
            last_release_ms: AtomicU64::new(0),
            pressed_at_ms: AtomicU64::new(0),
            tap_locked: AtomicBool::new(false),
            toggle_active: AtomicBool::new(false),
            start_owner: Arc::new(HotkeyStartOwner::default()),
            transition: parking_lot::Mutex::new(()),
            registered: AtomicBool::new(true),
        }
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn reset_hotkey_event_gate(gate: &HotkeyEventGate) {
    gate.start_owner.release();
    gate.is_pressed.store(false, Ordering::Release);
    gate.last_release_ms.store(0, Ordering::Release);
    gate.pressed_at_ms.store(0, Ordering::Release);
    gate.tap_locked.store(false, Ordering::Release);
    gate.toggle_active.store(false, Ordering::Release);
}

#[cfg(target_os = "windows")]
fn hotkey_gate_is_current(trigger: RecordingTrigger, gate: &Arc<HotkeyEventGate>) -> bool {
    let guard = unified_hook_state_snapshot().load_full();
    match trigger {
        RecordingTrigger::DictationOriginal => {
            guard
                .dictation
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(&s.gate, gate))
                || guard
                    .second_dictation
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(&s.gate, gate))
        }
        RecordingTrigger::DictationTranslated => guard
            .translation
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.gate, gate)),
        RecordingTrigger::Assistant => guard
            .assistant
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.gate, gate)),
    }
}

#[cfg(not(target_os = "windows"))]
fn hotkey_gate_is_current(_trigger: RecordingTrigger, _gate: &Arc<HotkeyEventGate>) -> bool {
    true
}

// Configuration publication and dispatch must see one mode/registration pair.
fn hotkey_configuration_lock() -> &'static parking_lot::Mutex<()> {
    static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    &LOCK
}

// ---------------------------------------------------------------------------
// Toggle mode — persisted as a static AtomicBool
// ---------------------------------------------------------------------------

fn toggle_mode_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn is_toggle_mode() -> bool {
    toggle_mode_flag().load(Ordering::Acquire)
}

static SHARE_DICTATION_SHORTCUT: AtomicBool = AtomicBool::new(false);
static TAP_OR_HOLD_ENABLED: AtomicBool = AtomicBool::new(false);
static STANDALONE_SPECIAL_KEYS: AtomicBool = AtomicBool::new(false);

pub(super) fn is_share_dictation_shortcut() -> bool {
    SHARE_DICTATION_SHORTCUT.load(Ordering::Acquire)
}

pub(super) fn is_tap_or_hold_enabled() -> bool {
    TAP_OR_HOLD_ENABLED.load(Ordering::Acquire)
}

pub(super) fn is_standalone_special_keys_enabled() -> bool {
    STANDALONE_SPECIAL_KEYS.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Diagnostic & error helpers
// ---------------------------------------------------------------------------

fn emit_recording_error(app_handle: &tauri::AppHandle, message: &str) {
    let _ = app_handle.emit(
        "recording-error",
        serde_json::json!({
            "message": message,
        }),
    );
}

fn hotkey_warning_message(shortcut_label: &str) -> Option<String> {
    if shortcut_label.contains("Win") {
        return Some(
            "Some Win key combinations may be reserved by Windows or other applications."
                .to_string(),
        );
    }
    if shortcut_label == "Alt" {
        return Some("Using Alt alone prevents menu bar activation.".to_string());
    }
    None
}

fn hotkey_kind_label(kind: HotkeyKind) -> &'static str {
    match kind {
        HotkeyKind::Dictation => "dictation",
        HotkeyKind::SecondDictation => "secondary dictation",
        HotkeyKind::Translation => "translation",
        HotkeyKind::Assistant => "assistant",
        HotkeyKind::TerminalPaste => "terminal paste",
        HotkeyKind::TypedText => "typed text",
        HotkeyKind::MeetingToggle => "meeting toggle",
    }
}

fn ensure_hotkey_not_conflicting(
    _app_handle: &tauri::AppHandle,
    kind: HotkeyKind,
    candidate_label: &str,
) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        let guard = unified_hook_state_slot().lock();
        for (other_kind, state) in [
            (HotkeyKind::Dictation, guard.dictation.as_ref()),
            (HotkeyKind::SecondDictation, guard.second_dictation.as_ref()),
            (HotkeyKind::Translation, guard.translation.as_ref()),
            (HotkeyKind::Assistant, guard.assistant.as_ref()),
            (HotkeyKind::TerminalPaste, guard.terminal_paste.as_ref()),
            (HotkeyKind::TypedText, guard.typed_text.as_ref()),
            (HotkeyKind::MeetingToggle, guard.meeting_toggle.as_ref()),
        ] {
            if other_kind == kind {
                continue;
            }
            if let Some(state) = state {
                if state.spec.label() == candidate_label {
                    return Err(AppError::Other(format!(
                        "Shortcut {} is already assigned to the {} hotkey; please choose a different combination",
                        candidate_label,
                        hotkey_kind_label(other_kind)
                    )));
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = kind;
        let _ = candidate_label;
    }

    Ok(())
}

fn update_hotkey_diagnostic<F>(app_handle: &tauri::AppHandle, update: F)
where
    F: FnOnce(&mut crate::state::HotkeyDiagnosticState),
{
    let state = app_handle.state::<AppState>();
    let (_, snapshot) = state.update_hotkey_diagnostic(|diagnostic| {
        update(diagnostic);
    });
    let _ = app_handle.emit("hotkey-diagnostic", snapshot);
}

// ---------------------------------------------------------------------------
// Recording start/stop business logic
// ---------------------------------------------------------------------------

fn update_hotkey_diagnostic_for_trigger<F>(
    app_handle: &tauri::AppHandle,
    trigger: RecordingTrigger,
    update: F,
) where
    F: FnOnce(&mut crate::state::HotkeyDiagnosticState),
{
    if trigger == RecordingTrigger::DictationOriginal {
        update_hotkey_diagnostic(app_handle, update);
    }
}

/// Register a hotkey on the appropriate backend. Returns the backend label on success.
/// Caller holds the configuration lock. Retired Arcs are snapshots of settings,
/// never candidates for publication during rollback.
#[cfg(target_os = "windows")]
fn restore_hotkey_registrations(
    app_handle: &tauri::AppHandle,
    previous: &[(HotkeyKind, Option<Arc<UnifiedHookState>>)],
) -> Result<(), AppError> {
    for (kind, _) in previous {
        if let Some(current) = set_unified_hook_state(*kind, None) {
            force_release_hotkey(&current);
        }
    }
    for (kind, state) in previous {
        if let Some(state) = state {
            let fresh = build_hook_state(
                state.app_handle.clone(),
                *kind,
                state.spec.clone(),
                state.trigger,
            );
            set_unified_hook_state(*kind, Some(fresh.clone()));
            try_register_hotkey_backend(*kind, &fresh);
        }
    }
    if let Err(error) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
        for (kind, _) in previous {
            if let Some(current) = set_unified_hook_state(*kind, None) {
                force_release_hotkey(&current);
            }
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn rollback_hotkey_error(
    app_handle: &tauri::AppHandle,
    previous: &[(HotkeyKind, Option<Arc<UnifiedHookState>>)],
    error: AppError,
) -> AppError {
    match restore_hotkey_registrations(app_handle, previous) {
        Ok(()) => error,
        Err(restore_error) => AppError::Other(format!(
            "{error}; failed to restore previous shortcut: {restore_error}"
        )),
    }
}

/// Register a hotkey on the appropriate backend. Returns the backend label on success.
#[cfg(target_os = "windows")]
fn register_on_chosen_backend(
    app_handle: &tauri::AppHandle,
    kind: HotkeyKind,
    chosen_backend: HotkeyBackend,
    hook_state: Arc<UnifiedHookState>,
    previous_state: Option<Arc<UnifiedHookState>>,
    label: &str,
) -> Result<&'static str, AppError> {
    if chosen_backend == HotkeyBackend::RegisterHotKey {
        if let HotkeySpec::Standard {
            modifiers, main_vk, ..
        } = &hook_state.spec
        {
            let (mods_copy, vk_copy) = (modifiers.clone(), *main_vk);
            match register_via_reg_hotkey(&mods_copy, vk_copy, hook_state.clone()) {
                Ok(()) => {
                    // Sync LLKH lifecycle — may stop hook if not needed by others
                    if let Err(error) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
                        return Err(rollback_hotkey_error(
                            app_handle,
                            &[(kind, previous_state)],
                            error,
                        ));
                    }
                    return Ok("registerHotKey");
                }
                Err(reg_err) => {
                    force_release_hotkey(&hook_state);
                    // RegisterHotKey failed — rebuild state with LowLevelHook backend
                    // so the LLKH callback will process this hotkey.
                    log::warn!(
                        "RegisterHotKey 注册 {} 失败，回退到低层键盘钩子: {}",
                        label,
                        reg_err
                    );
                    let fallback_state = build_hook_state_with_backend(
                        hook_state.app_handle.clone(),
                        hook_state.kind,
                        hook_state.spec.clone(),
                        hook_state.trigger,
                        HotkeyBackend::LowLevelHook,
                    );
                    set_unified_hook_state(kind, Some(fallback_state));
                    // Fall through to LLKH path below
                }
            }
        }
    }

    // LLKH path (either direct or fallback from RegisterHotKey failure)
    if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
        let err = rollback_hotkey_error(app_handle, &[(kind, previous_state)], err);
        let now_ms = now_unix_ms();
        update_hotkey_diagnostic(app_handle, |diagnostic| {
            diagnostic.shortcut = label.to_string();
            diagnostic.registered = false;
            diagnostic.backend = "none".to_string();
            diagnostic.is_pressed = false;
            diagnostic.last_error = Some(err.to_string());
            diagnostic.warning = hotkey_warning_message(label);
            diagnostic.last_event = Some("error".to_string());
            diagnostic.last_event_at_ms = Some(now_ms);
        });
        return Err(err);
    }

    Ok("lowLevelHook")
}

/// Try to register a hotkey via RegisterHotKey if its backend demands it.
/// On failure, rebuilds state with LowLevelHook backend in the slot.
#[cfg(target_os = "windows")]
fn try_register_hotkey_backend(kind: HotkeyKind, state: &Arc<UnifiedHookState>) {
    if state.backend != HotkeyBackend::RegisterHotKey {
        return;
    }
    if let HotkeySpec::Standard {
        modifiers, main_vk, ..
    } = &state.spec
    {
        let (mods_copy, vk_copy) = (modifiers.clone(), *main_vk);
        if let Err(e) = register_via_reg_hotkey(&mods_copy, vk_copy, state.clone()) {
            force_release_hotkey(state);
            log::warn!(
                "{} RegisterHotKey 失败，回退到 LLKH: {}",
                state.spec.label(),
                e
            );
            let fallback = build_hook_state_with_backend(
                state.app_handle.clone(),
                state.kind,
                state.spec.clone(),
                state.trigger,
                HotkeyBackend::LowLevelHook,
            );
            set_unified_hook_state(kind, Some(fallback));
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn register_custom_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    let spec =
        match normalize_shortcut_with_options(&shortcut, is_standalone_special_keys_enabled()) {
            Ok(spec) => spec,
            Err(err) => {
                let now_ms = now_unix_ms();
                update_hotkey_diagnostic(&app_handle, |diagnostic| {
                    diagnostic.shortcut = shortcut.replace("Super", "Win");
                    diagnostic.registered = false;
                    diagnostic.backend = "none".to_string();
                    diagnostic.is_pressed = false;
                    diagnostic.last_error = Some(err.to_string());
                    diagnostic.warning = None;
                    diagnostic.last_event = Some("error".to_string());
                    diagnostic.last_event_at_ms = Some(now_ms);
                });
                return Err(err);
            }
        };

    let label = spec.label().to_string();
    ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Dictation, &label)?;

    // Unregister from the previous backend BEFORE probing — otherwise our own
    // RegisterHotKey registration would cause a false positive system conflict.
    #[cfg(target_os = "windows")]
    {
        unregister_via_reg_hotkey(HotkeyKind::Dictation);
    }

    // Probe system-wide conflict
    #[cfg(target_os = "windows")]
    let system_conflict = probe_system_hotkey_conflict(&spec);
    #[cfg(not(target_os = "windows"))]
    let system_conflict: Option<String> = None;

    if let Some(ref msg) = system_conflict {
        log::warn!("系统快捷键冲突检测: {}", msg);
    }

    #[cfg(target_os = "windows")]
    let hook_state = build_hook_state(
        app_handle.clone(),
        HotkeyKind::Dictation,
        spec,
        RecordingTrigger::DictationOriginal,
    );

    #[cfg(target_os = "windows")]
    let chosen_backend = hook_state.backend;

    #[cfg(target_os = "windows")]
    let previous_state = set_unified_hook_state(HotkeyKind::Dictation, Some(hook_state.clone()));

    #[cfg(target_os = "windows")]
    if let Some(previous) = previous_state.as_ref() {
        force_release_hotkey(previous);
    }

    #[cfg(target_os = "windows")]
    let backend_label = register_on_chosen_backend(
        &app_handle,
        HotkeyKind::Dictation,
        chosen_backend,
        hook_state,
        previous_state,
        &label,
    )?;

    #[cfg(not(target_os = "windows"))]
    let backend_label = {
        sync_hotkey_monitor_lifecycle(app_handle.clone())?;
        "lowLevelHook"
    };

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.shortcut = label.clone();
        diagnostic.registered = true;
        diagnostic.backend = backend_label.to_string();
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.warning = hotkey_warning_message(&label);
        diagnostic.system_conflict = system_conflict;
        diagnostic.last_event = Some("registered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
        diagnostic.last_registered_at_ms = Some(now_ms);
    });

    log::info!("自定义快捷键 {} 已注册（{}）", label, backend_label);
    Ok(format!("Shortcut {} registered", label))
}

#[tauri::command]
pub async fn register_translation_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    register_translation_hotkey_inner(
        app_handle,
        Some(shortcut.trim().to_string()).filter(|value| !value.is_empty()),
    )
}

pub(crate) fn register_translation_hotkey_inner(
    app_handle: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<String, AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    #[cfg(not(target_os = "windows"))]
    {
        if shortcut.is_some() {
            ensure_unified_hotkey_monitor(app_handle)?;
        }
        return Ok("Translation hotkey updated".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let next_state = if let Some(shortcut) = shortcut {
            let spec =
                normalize_shortcut_with_options(&shortcut, is_standalone_special_keys_enabled())?;
            ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Translation, spec.label())?;
            Some(build_hook_state(
                app_handle.clone(),
                HotkeyKind::Translation,
                spec,
                RecordingTrigger::DictationTranslated,
            ))
        } else {
            None
        };

        let previous_state = set_unified_hook_state(HotkeyKind::Translation, next_state.clone());

        if let Some(previous) = previous_state.as_ref() {
            force_release_hotkey(previous);
        }

        if let Some(ref state) = next_state {
            try_register_hotkey_backend(HotkeyKind::Translation, state);
        }

        if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
            return Err(rollback_hotkey_error(
                &app_handle,
                &[(HotkeyKind::Translation, previous_state)],
                err,
            ));
        }

        let label = next_state
            .as_ref()
            .map(|state| state.spec.label().to_string())
            .unwrap_or_else(|| "Not set".to_string());
        log::info!("翻译热键已更新: {}", label);
        Ok(format!("Translation hotkey updated: {}", label))
    }
}

#[tauri::command]
pub async fn register_assistant_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
) -> Result<String, AppError> {
    register_assistant_hotkey_inner(
        app_handle,
        Some(shortcut.trim().to_string()).filter(|value| !value.is_empty()),
    )
}

pub(crate) fn register_assistant_hotkey_inner(
    app_handle: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<String, AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    #[cfg(not(target_os = "windows"))]
    {
        if shortcut.is_some() {
            ensure_unified_hotkey_monitor(app_handle)?;
        }
        return Ok("Assistant hotkey updated".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        let next_state = if let Some(shortcut) = shortcut {
            let spec =
                normalize_shortcut_with_options(&shortcut, is_standalone_special_keys_enabled())?;
            ensure_hotkey_not_conflicting(&app_handle, HotkeyKind::Assistant, spec.label())?;
            Some(build_hook_state(
                app_handle.clone(),
                HotkeyKind::Assistant,
                spec,
                RecordingTrigger::Assistant,
            ))
        } else {
            None
        };

        let previous_state = set_unified_hook_state(HotkeyKind::Assistant, next_state.clone());

        if let Some(previous) = previous_state.as_ref() {
            force_release_hotkey(previous);
        }

        if let Some(ref state) = next_state {
            try_register_hotkey_backend(HotkeyKind::Assistant, state);
        }

        if let Err(err) = sync_hotkey_monitor_lifecycle(app_handle.clone()) {
            return Err(rollback_hotkey_error(
                &app_handle,
                &[(HotkeyKind::Assistant, previous_state)],
                err,
            ));
        }

        let label = next_state
            .as_ref()
            .map(|state| state.spec.label().to_string())
            .unwrap_or_else(|| "Not set".to_string());
        log::info!("助手热键已更新: {}", label);
        Ok(format!("Assistant hotkey updated: {}", label))
    }
}

#[tauri::command]
pub async fn unregister_all_hotkeys(app_handle: tauri::AppHandle) -> Result<String, AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    #[cfg(target_os = "windows")]
    {
        for kind in [
            HotkeyKind::Dictation,
            HotkeyKind::SecondDictation,
            HotkeyKind::Translation,
            HotkeyKind::Assistant,
            HotkeyKind::TerminalPaste,
            HotkeyKind::TypedText,
            HotkeyKind::MeetingToggle,
        ] {
            unregister_via_reg_hotkey(kind);
            if let Some(previous) = set_unified_hook_state(kind, None) {
                force_release_hotkey(&previous);
            }
        }
    }
    stop_unified_hotkey_monitor();
    #[cfg(target_os = "windows")]
    stop_reg_hotkey_backend();

    let now_ms = now_unix_ms();
    update_hotkey_diagnostic(&app_handle, |diagnostic| {
        diagnostic.registered = false;
        diagnostic.is_pressed = false;
        diagnostic.last_error = None;
        diagnostic.last_event = Some("unregistered".to_string());
        diagnostic.last_event_at_ms = Some(now_ms);
    });

    log::info!("所有全局快捷键已注销");
    Ok("All global shortcuts unregistered".to_string())
}

#[tauri::command]
pub async fn set_recording_mode(
    _app_handle: tauri::AppHandle,
    toggle: bool,
) -> Result<(), AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    let previous_mode = toggle_mode_flag().swap(toggle, Ordering::AcqRel);
    if previous_mode == toggle {
        return Ok(());
    }
    // If switching from toggle→hold while toggle is active, stop recording
    if !toggle {
        let state = _app_handle.state::<AppState>();
        let active_session = state
            .recording
            .recording
            .lock()
            .as_ref()
            .map(|slot| (slot.session_id(), slot.trigger()));
        if let Some((session_id, trigger)) = active_session {
            handle_hotkey_stop(
                _app_handle.clone(),
                "切换到按住模式，停止当前录音".to_string(),
                trigger,
                session_id,
            );
        }
    }

    // Mode change may require backend migration for all registered hotkeys.
    // Re-register each active hotkey so classify_backend picks the right backend.
    #[cfg(target_os = "windows")]
    {
        let bundle = get_unified_hook_states();
        for (kind, state) in [
            (HotkeyKind::Dictation, bundle.dictation.as_ref()),
            (
                HotkeyKind::SecondDictation,
                bundle.second_dictation.as_ref(),
            ),
            (HotkeyKind::Translation, bundle.translation.as_ref()),
            (HotkeyKind::Assistant, bundle.assistant.as_ref()),
            (HotkeyKind::TerminalPaste, bundle.terminal_paste.as_ref()),
            (HotkeyKind::TypedText, bundle.typed_text.as_ref()),
            (HotkeyKind::MeetingToggle, bundle.meeting_toggle.as_ref()),
        ] {
            if let Some(old_state) = state {
                let new_backend = classify_backend(kind, &old_state.spec);
                let label = old_state.spec.label().to_string();
                log::info!(
                    "模式切换：{} 从 {:?} 迁移到 {:?}",
                    label,
                    old_state.backend,
                    new_backend
                );

                // Retire even when the backend is unchanged: queued events
                // must not reinterpret an old press using the new mode.
                force_release_hotkey(old_state);

                // Rebuild state with new backend classification
                let new_state = build_hook_state(
                    old_state.app_handle.clone(),
                    kind,
                    old_state.spec.clone(),
                    old_state.trigger,
                );

                set_unified_hook_state(kind, Some(new_state.clone()));

                try_register_hotkey_backend(kind, &new_state);
            }
        }
        if let Err(error) = sync_hotkey_monitor_lifecycle(_app_handle.clone()) {
            toggle_mode_flag().store(previous_mode, Ordering::Release);
            return Err(rollback_hotkey_error(
                &_app_handle,
                &[
                    (HotkeyKind::Dictation, bundle.dictation),
                    (HotkeyKind::SecondDictation, bundle.second_dictation),
                    (HotkeyKind::Translation, bundle.translation),
                    (HotkeyKind::Assistant, bundle.assistant),
                    (HotkeyKind::TerminalPaste, bundle.terminal_paste),
                    (HotkeyKind::TypedText, bundle.typed_text),
                    (HotkeyKind::MeetingToggle, bundle.meeting_toggle),
                ],
                error,
            ));
        }
    }

    log::info!("录音模式已设置为: {}", if toggle { "切换" } else { "按住" });
    Ok(())
}

#[tauri::command]
pub async fn get_hotkey_diagnostic(
    state: tauri::State<'_, AppState>,
) -> Result<crate::state::HotkeyDiagnosticState, AppError> {
    Ok(state.hotkey_diagnostic_snapshot())
}

#[cfg(target_os = "windows")]
fn register_optional_hotkey_slot(
    app_handle: &tauri::AppHandle,
    kind: HotkeyKind,
    trigger: RecordingTrigger,
    shortcut: Option<&str>,
) -> Result<(), AppError> {
    let normalized = shortcut.map(str::trim).filter(|value| !value.is_empty());
    let next_state = if let Some(raw) = normalized {
        let spec = normalize_shortcut_with_options(raw, is_standalone_special_keys_enabled())?;
        ensure_hotkey_not_conflicting(app_handle, kind, spec.label())?;
        Some(build_hook_state(app_handle.clone(), kind, spec, trigger))
    } else {
        None
    };

    unregister_via_reg_hotkey(kind);
    let previous = set_unified_hook_state(kind, next_state.clone());
    if let Some(prev) = previous.as_ref() {
        force_release_hotkey(prev);
    }
    if let Some(ref state) = next_state {
        try_register_hotkey_backend(kind, state);
    }
    Ok(())
}

/// Applies secondary shortcuts (`second_dictation_hotkey`, `terminal_paste_hotkey`,
/// `typed_text_hotkey`, `meeting_hotkey`), passthrough (`share_dictation_shortcut`),
/// `tap_or_hold`, and `standalone_special_keys` from `UserProfile`.
pub(crate) fn sync_extended_hotkeys(
    app_handle: &tauri::AppHandle,
    shortcuts: &crate::state::user_profile::ShortcutsBehaviorConfig,
    labs: &crate::state::user_profile::LabsConfig,
) -> Result<(), AppError> {
    let _configuration = hotkey_configuration_lock().lock();
    SHARE_DICTATION_SHORTCUT.store(shortcuts.share_dictation_shortcut, Ordering::Release);
    TAP_OR_HOLD_ENABLED.store(labs.tap_or_hold, Ordering::Release);
    STANDALONE_SPECIAL_KEYS.store(labs.standalone_special_keys, Ordering::Release);

    #[cfg(target_os = "windows")]
    {
        register_optional_hotkey_slot(
            app_handle,
            HotkeyKind::SecondDictation,
            RecordingTrigger::DictationOriginal,
            shortcuts.second_dictation_hotkey.as_deref(),
        )?;
        register_optional_hotkey_slot(
            app_handle,
            HotkeyKind::TerminalPaste,
            RecordingTrigger::DictationOriginal,
            shortcuts.terminal_paste_hotkey.as_deref(),
        )?;
        register_optional_hotkey_slot(
            app_handle,
            HotkeyKind::TypedText,
            RecordingTrigger::DictationOriginal,
            shortcuts.typed_text_hotkey.as_deref(),
        )?;
        register_optional_hotkey_slot(
            app_handle,
            HotkeyKind::MeetingToggle,
            RecordingTrigger::DictationOriginal,
            shortcuts.meeting_hotkey.as_deref(),
        )?;
        // Refresh primary dictation backend if share_dictation_shortcut toggled it between RegisterHotKey and LLKH.
        let current_dict = unified_hook_state_slot().lock().dictation.clone();
        if let Some(dict_state) = current_dict {
            let desired_backend = classify_backend(HotkeyKind::Dictation, &dict_state.spec);
            if desired_backend != dict_state.backend {
                unregister_via_reg_hotkey(HotkeyKind::Dictation);
                force_release_hotkey(&dict_state);
                let rebuilt = build_hook_state(
                    app_handle.clone(),
                    HotkeyKind::Dictation,
                    dict_state.spec.clone(),
                    RecordingTrigger::DictationOriginal,
                );
                set_unified_hook_state(HotkeyKind::Dictation, Some(rebuilt.clone()));
                try_register_hotkey_backend(HotkeyKind::Dictation, &rebuilt);
            }
        }
        sync_hotkey_monitor_lifecycle(app_handle.clone())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app_handle;
    }
    Ok(())
}

#[tauri::command]
pub async fn register_extended_hotkeys(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let (shortcuts, labs) = state.with_profile(|p| (p.shortcuts_behavior.clone(), p.labs.clone()));
    sync_extended_hotkeys(&app_handle, &shortcuts, &labs)
}

#[cfg(test)]
mod tests;
