use super::*;

// Monitor thread management (LLKH + Mouse LL backend)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub(super) struct UnifiedHotkeyMonitor {
    pub(super) thread_id: u32,
    pub(super) has_mouse: bool,
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
        let mut guard = monitor_slot().lock();
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
pub(super) fn install_mouse_hook_on_thread() -> Result<HookHandle, String> {
    // SAFETY: Passing null returns the HMODULE of the current executable for the low-level mouse hook.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    // SAFETY: `unified_low_level_mouse_proc` matches HOOKPROC signature and module is valid for process lifetime.
    let hook =
        unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(unified_low_level_mouse_proc), module, 0) };
    if hook.is_null() {
        Err(format!(
            "Failed to install mouse hook: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(hook)
    }
}

#[cfg(target_os = "windows")]
pub(super) fn ensure_unified_hotkey_monitor(
    _app_handle: tauri::AppHandle,
    need_mouse: bool,
) -> Result<(), AppError> {
    {
        let guard = monitor_slot().lock();
        if let Some(monitor) = guard.as_ref() {
            if monitor.has_mouse == need_mouse {
                return Ok(());
            }
        }
    }
    stop_unified_hotkey_monitor();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("unified-hotkey-monitor".to_string())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId has no preconditions and returns the Win32 ID of this OS thread.
            let thread_id = unsafe { GetCurrentThreadId() };

            let kbd_hook = match install_hook_on_thread() {
                Ok(h) => h,
                Err(msg) => {
                    let _ = ready_tx.send(Err(msg));
                    return;
                }
            };
            let mouse_hook = if need_mouse {
                match install_mouse_hook_on_thread() {
                    Ok(h) => Some(h),
                    Err(msg) => {
                        // SAFETY: `kbd_hook` was returned by `SetWindowsHookExW` above.
                        unsafe { UnhookWindowsHookEx(kbd_hook) };
                        let _ = ready_tx.send(Err(msg));
                        return;
                    }
                }
            } else {
                None
            };

            // SAFETY: `MSG` is a plain Win32 C struct; zero-initialization is valid before PeekMessageW populates it.
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: `peek_msg` points to writable stack memory on the current thread to initialize its message queue.
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                // SAFETY: `kbd_hook` and `mh` were created on this thread via `SetWindowsHookExW`.
                unsafe { UnhookWindowsHookEx(kbd_hook) };
                if let Some(mh) = mouse_hook {
                    // SAFETY: `mh` was created on this thread via `SetWindowsHookExW`.
                    unsafe { UnhookWindowsHookEx(mh) };
                }
                return;
            }

            // SAFETY: `MSG` is a plain Win32 C struct populated by GetMessageW below.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                // SAFETY: `message` is writable stack storage owned by this thread's message loop.
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result <= 0 {
                    break;
                }
                // SAFETY: `message` was initialized by GetMessageW above.
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            // SAFETY: Unhooking the Win32 hooks registered by this thread before exit.
            unsafe { UnhookWindowsHookEx(kbd_hook) };
            if let Some(mh) = mouse_hook {
                // SAFETY: `mh` was registered by this thread via `SetWindowsHookExW`.
                unsafe { UnhookWindowsHookEx(mh) };
            }
        })
        .map_err(|e| AppError::Other(format!("Failed to start hotkey listener thread: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| AppError::Other(format!("Timed out waiting for hotkey listener: {}", e)))?
        .map_err(AppError::Other)?;

    let mut guard = monitor_slot().lock();
    *guard = Some(UnifiedHotkeyMonitor {
        thread_id,
        has_mouse: need_mouse,
        handle,
    });
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
    if state.kind.is_recording_trigger() {
        if let Some(session_id) = state.gate.start_owner.release() {
            handle_hotkey_stop(
                state.app_handle.clone(),
                label.to_string(),
                state.trigger,
                session_id,
            );
        }
    }
    reset_hotkey_event_gate(&state.gate);
}

#[cfg(not(target_os = "windows"))]
pub(super) fn ensure_unified_hotkey_monitor(
    _app_handle: tauri::AppHandle,
    _need_mouse: bool,
) -> Result<(), AppError> {
    Err(AppError::Other(
        "Low-level keyboard hook hotkeys are not supported on this OS".to_string(),
    ))
}

#[cfg(target_os = "windows")]
pub(super) fn build_hook_state(
    app_handle: tauri::AppHandle,
    kind: HotkeyKind,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
) -> Arc<UnifiedHookState> {
    let backend = classify_backend(kind, &spec);
    build_hook_state_with_backend(app_handle, kind, spec, trigger, backend)
}

pub(super) fn build_hook_state_with_backend(
    app_handle: tauri::AppHandle,
    kind: HotkeyKind,
    spec: HotkeySpec,
    trigger: RecordingTrigger,
    backend: HotkeyBackend,
) -> Arc<UnifiedHookState> {
    let key_down_count = match &spec {
        HotkeySpec::ModifierOnly {
            required_modifiers, ..
        } => required_modifiers.len(),
        HotkeySpec::Standard { .. } | HotkeySpec::Mouse { .. } => 0,
    };

    Arc::new(UnifiedHookState {
        app_handle,
        kind,
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
    let (has_llkh, has_mouse) = {
        let guard = unified_hook_state_slot().lock();
        let all = guard.all_states();
        let has_llkh = all
            .iter()
            .flatten()
            .any(|s| s.backend == HotkeyBackend::LowLevelHook);
        let has_mouse = all
            .iter()
            .flatten()
            .any(|s| matches!(s.spec, HotkeySpec::Mouse { .. }));
        (has_llkh, has_mouse)
    };

    if has_llkh || has_mouse {
        ensure_unified_hotkey_monitor(app_handle, has_mouse)
    } else {
        stop_unified_hotkey_monitor();
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn sync_hotkey_monitor_lifecycle(app_handle: tauri::AppHandle) -> Result<(), AppError> {
    ensure_unified_hotkey_monitor(app_handle, false)
}
