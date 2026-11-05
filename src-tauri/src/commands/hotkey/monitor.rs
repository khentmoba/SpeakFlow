use super::*;

// Monitor thread management (LLKH backend)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub(super) struct UnifiedHotkeyMonitor {
    pub(super) thread_id: u32,
    pub(super) handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
pub(super) fn monitor_slot() -> &'static Mutex<Option<UnifiedHotkeyMonitor>> {
    static SLOT: OnceLock<Mutex<Option<UnifiedHotkeyMonitor>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "windows")]
pub(super) fn stop_unified_hotkey_monitor() {
    let monitor = {
        let mut guard = match monitor_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.take()
    };

    if let Some(monitor) = monitor {
        // SAFETY: PostThreadMessageW takes only integers; if the monitor thread has exited, the call fails without touching memory.
        let _ = unsafe { PostThreadMessageW(monitor.thread_id, WM_QUIT, 0, 0) };
        let _ = monitor.handle.join();
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn stop_unified_hotkey_monitor() {}

#[cfg(target_os = "windows")]
/// HHOOK is *mut c_void in windows-sys
pub(super) type HookHandle = *mut std::ffi::c_void;

pub(super) fn install_hook_on_thread() -> Result<HookHandle, String> {
    // SAFETY: A null module name returns a handle to the current executable and dereferences no pointer.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    // SAFETY: The callback is a static function with the signature WH_KEYBOARD_LL requires; module is this executable's handle.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(unified_low_level_keyboard_proc),
            module,
            0,
        )
    };
    if hook.is_null() {
        Err(format!(
            "Failed to install keyboard hook: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(hook)
    }
}

#[cfg(target_os = "windows")]
pub(super) fn ensure_unified_hotkey_monitor(_app_handle: tauri::AppHandle) -> Result<(), AppError> {
    let monitor_running = {
        let guard = match monitor_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.is_some()
    };
    if monitor_running {
        return Ok(());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("unified-hotkey-monitor".to_string())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId takes no arguments and cannot fail.
            let thread_id = unsafe { GetCurrentThreadId() };

            let hook = match install_hook_on_thread() {
                Ok(h) => h,
                Err(msg) => {
                    let _ = ready_tx.send(Err(msg));
                    return;
                }
            };
            let hook_handle: HookHandle = hook;

            // Ensure message queue exists
            // SAFETY: MSG is a plain C struct of integers and pointers, so the all-zero bit pattern is a valid value.
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: peek_msg is a valid writable MSG; PM_NOREMOVE only creates this thread's queue and does not dequeue messages.
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                // SAFETY: hook_handle was just returned by SetWindowsHookExW on this thread and has not been unhooked yet.
                unsafe { UnhookWindowsHookEx(hook_handle) };
                return;
            }

            // Simple message loop — the hook callback is kept fast enough that
            // Windows will not silently remove it.
            // SAFETY: MSG is a plain C struct of integers and pointers, so the all-zero bit pattern is a valid value.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                // SAFETY: message is a valid writable MSG, and a null window retrieves messages for this thread's queue.
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result <= 0 {
                    break;
                }

                // SAFETY: message was just filled by GetMessageW on this thread and is a valid MSG for both calls.
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            // SAFETY: hook_handle is still installed by this thread and is unhooked once, after the message loop exits.
            unsafe { UnhookWindowsHookEx(hook_handle) };
        })
        .map_err(|e| AppError::Other(format!("Failed to start hotkey listener thread: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("Timed out waiting for hotkey listener: {}", e)))?
        .map_err(AppError::Other)?;

    let mut guard = match monitor_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = Some(UnifiedHotkeyMonitor { thread_id, handle });
    Ok(())
}

#[cfg(target_os = "windows")]
pub(super) fn force_release_hotkey(state: &UnifiedHookState) {
    let _transition = state.gate.transition.lock();
    state.gate.registered.store(false, Ordering::Release);
    if let Some(id) = state.reg_hotkey_id {
        unregister_reg_hotkey_id(id);
    }
    let label = state.spec.label();
    if let Some(session_id) = state.gate.start_owner.release() {
        handle_hotkey_stop(
            state.app_handle.clone(),
            label.to_string(),
            state.trigger,
            session_id,
        );
    }
    reset_hotkey_event_gate(&state.gate);
}

#[cfg(not(target_os = "windows"))]
pub(super) fn ensure_unified_hotkey_monitor(_app_handle: tauri::AppHandle) -> Result<(), AppError> {
    Err(AppError::Other(
        "Low-level keyboard hook hotkeys are not supported on this OS".to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub(super) fn build_hook_state(
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
) -> Arc<UnifiedHookState> {
    let backend = classify_backend(&spec);
    build_hook_state_with_backend(app_handle, spec, trigger, backend)
}

pub(super) fn build_hook_state_with_backend(
    app_handle: tauri::AppHandle,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
    backend: HotkeyBackend,
) -> Arc<UnifiedHookState> {
    let key_down_count = match &spec {
        HotkeySpec::ModifierOnly {
            required_modifiers, ..
        } => required_modifiers.len(),
        HotkeySpec::Standard { .. } => 0,
    };

    Arc::new(UnifiedHookState {
        app_handle,
        backend,
        reg_hotkey_id: if backend == HotkeyBackend::RegisterHotKey {
            next_reg_hotkey_id()
        } else {
            None
        },
        spec,
        trigger,
        gate: Arc::new(HotkeyEventGate::default()),
        key_down: (0..key_down_count)
            .map(|_| AtomicBool::new(false))
            .collect(),
        activated: AtomicBool::new(false),
        locked: AtomicBool::new(false),
        last_release_ms: AtomicU64::new(0),
        tainted: AtomicBool::new(false),
        modifier_leaked: AtomicBool::new(false),
    })
}

#[cfg(target_os = "windows")]
pub(super) fn sync_hotkey_monitor_lifecycle(app_handle: tauri::AppHandle) -> Result<(), AppError> {
    let (has_llkh, _has_any) = {
        let guard = match unified_hook_state_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let has_llkh = [
            guard.dictation.as_ref(),
            guard.translation.as_ref(),
            guard.assistant.as_ref(),
        ]
        .iter()
        .flatten()
        .any(|s| s.backend == HotkeyBackend::LowLevelHook);
        let has_any = !guard.is_empty();
        (has_llkh, has_any)
    };

    // Only install LLKH when at least one hotkey actually needs it
    if has_llkh {
        ensure_unified_hotkey_monitor(app_handle)
    } else {
        stop_unified_hotkey_monitor();
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn sync_hotkey_monitor_lifecycle(app_handle: tauri::AppHandle) -> Result<(), AppError> {
    ensure_unified_hotkey_monitor(app_handle)
}
