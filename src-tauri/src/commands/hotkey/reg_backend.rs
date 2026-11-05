use super::*;

// ---------------------------------------------------------------------------
// RegisterHotKey backend — dedicated thread with message pump
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub(super) enum RegHotkeyCmd {
    Register {
        id: i32,
        mods: u32,
        vk: u32,
        state: Arc<UnifiedHookState>,
        result_tx: std::sync::mpsc::SyncSender<Result<(), String>>,
    },
    Unregister {
        id: i32,
    },
}

#[cfg(target_os = "windows")]
pub(super) fn reg_hotkey_cmd_channel() -> &'static (
    std::sync::mpsc::Sender<RegHotkeyCmd>,
    Mutex<std::sync::mpsc::Receiver<RegHotkeyCmd>>,
) {
    static CH: OnceLock<(
        std::sync::mpsc::Sender<RegHotkeyCmd>,
        Mutex<std::sync::mpsc::Receiver<RegHotkeyCmd>>,
    )> = OnceLock::new();
    CH.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        (tx, Mutex::new(rx))
    })
}

#[cfg(target_os = "windows")]
pub(super) struct RegisterHotkeyBackend {
    pub(super) thread_id: u32,
    pub(super) handle: JoinHandle<()>,
}

#[cfg(target_os = "windows")]
pub(super) fn reg_backend_slot() -> &'static Mutex<Option<RegisterHotkeyBackend>> {
    static SLOT: OnceLock<Mutex<Option<RegisterHotkeyBackend>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Windows application hotkey IDs are 0x0000..=0xBFFF. Never reuse one in this
/// process: an old WM_HOTKEY may remain queued after UnregisterHotKey.
#[cfg(target_os = "windows")]
pub(super) fn allocate_reg_hotkey_id(next: &AtomicU32) -> Option<i32> {
    next.fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| {
        (id <= 0xBFFF).then(|| id + 1)
    })
    .ok()
    .map(|id| id as i32)
}

#[cfg(target_os = "windows")]
pub(super) fn next_reg_hotkey_id() -> Option<i32> {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    allocate_reg_hotkey_id(&NEXT)
}

/// Build MOD_ flags for RegisterHotKey from ShortcutModifiers
#[cfg(target_os = "windows")]
pub(super) fn shortcut_mods_to_reg_mods(mods: &ShortcutModifiers) -> u32 {
    let mut m = MOD_NOREPEAT;
    if mods.has_ctrl() {
        m |= MOD_CONTROL;
    }
    if mods.has_alt() {
        m |= MOD_ALT;
    }
    if mods.has_shift() {
        m |= MOD_SHIFT;
    }
    if mods.has_super() {
        m |= MOD_WIN;
    }
    m
}

#[cfg(target_os = "windows")]
pub(super) fn ensure_reg_hotkey_backend() -> Result<(), AppError> {
    let backend_running = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.is_some()
    };
    if backend_running {
        return Ok(());
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let handle = std::thread::Builder::new()
        .name("register-hotkey-backend".into())
        .spawn(move || {
            // SAFETY: GetCurrentThreadId takes no arguments and cannot fail.
            let thread_id = unsafe { GetCurrentThreadId() };

            // Ensure message queue exists
            // SAFETY: MSG is a plain C struct of integers and pointers, so the all-zero bit pattern is a valid value.
            let mut peek_msg: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: peek_msg is a valid writable MSG; PM_NOREMOVE only creates this thread's queue and does not dequeue messages.
            let _ = unsafe { PeekMessageW(&mut peek_msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };

            if ready_tx.send(Ok(thread_id)).is_err() {
                return;
            }

            // Registered hotkey states, keyed by id
            let mut registered: std::collections::HashMap<i32, Arc<UnifiedHookState>> =
                std::collections::HashMap::new();

            // SAFETY: MSG is a plain C struct of integers and pointers, so the all-zero bit pattern is a valid value.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                // SAFETY: message is a valid writable MSG, and a null window retrieves messages for this thread's queue.
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result <= 0 {
                    break;
                }

                match message.message {
                    WM_HOTKEY => {
                        let id = message.wParam as i32;
                        if let Some(state) = registered.get(&id) {
                            send_dispatch(DispatchEvent::Press(
                                state.clone(),
                                format!("{} 按下（RegisterHotKey）", state.spec.label()),
                            ));
                        }
                    }
                    WM_NULL => {
                        // Drain command channel
                        let rx_guard = match reg_hotkey_cmd_channel().1.lock() {
                            Ok(g) => g,
                            Err(p) => p.into_inner(),
                        };
                        while let Ok(cmd) = rx_guard.try_recv() {
                            match cmd {
                                RegHotkeyCmd::Register {
                                    id,
                                    mods,
                                    vk,
                                    state,
                                    result_tx,
                                } => {
                                    if !state.gate.registered.load(Ordering::Acquire) {
                                        let _ = result_tx.send(Err(
                                            "Hotkey registration is no longer valid".into(),
                                        ));
                                        continue;
                                    }
                                    // SAFETY: RegisterHotKey takes only integer arguments; a null window binds the hotkey to this thread's queue.
                                    let ok = unsafe {
                                        RegisterHotKey(std::ptr::null_mut(), id, mods, vk)
                                    };
                                    if ok != 0 {
                                        registered.insert(id, state);
                                        let _ = result_tx.send(Ok(()));
                                    } else {
                                        let err = std::io::Error::last_os_error();
                                        let _ = result_tx
                                            .send(Err(format!("RegisterHotKey failed: {}", err)));
                                    }
                                }
                                RegHotkeyCmd::Unregister { id } => {
                                    if registered.remove(&id).is_some() {
                                        // SAFETY: UnregisterHotKey takes only integer arguments and releases a hotkey this thread registered earlier.
                                        unsafe {
                                            UnregisterHotKey(std::ptr::null_mut(), id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // SAFETY: message was just filled by GetMessageW on this thread and is a valid MSG for both calls.
                    _ => unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    },
                }
            }

            // Cleanup all registered hotkeys
            for &id in registered.keys() {
                // SAFETY: UnregisterHotKey takes only integer arguments and releases hotkeys this thread registered.
                unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
            }
        })
        .map_err(|e| AppError::Other(format!("Failed to start RegisterHotKey thread: {}", e)))?;

    let thread_id = ready_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| {
            AppError::Other(format!(
                "Timed out waiting for RegisterHotKey thread: {}",
                e
            ))
        })?
        .map_err(AppError::Other)?;

    let mut guard = match reg_backend_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = Some(RegisterHotkeyBackend { thread_id, handle });
    Ok(())
}

/// Register a hotkey via the RegisterHotKey backend.
/// Blocks until the registration is confirmed or rejected.
#[cfg(target_os = "windows")]
pub(super) fn register_via_reg_hotkey(
    mods: &ShortcutModifiers,
    main_vk: u16,
    state: Arc<UnifiedHookState>,
) -> Result<(), AppError> {
    ensure_reg_hotkey_backend()?;

    let id = state.reg_hotkey_id.ok_or_else(|| {
        AppError::Other(
            "RegisterHotKey IDs exhausted; falling back to low-level keyboard hook".into(),
        )
    })?;
    let win_mods = shortcut_mods_to_reg_mods(mods);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);

    reg_hotkey_cmd_channel()
        .0
        .send(RegHotkeyCmd::Register {
            id,
            mods: win_mods,
            vk: main_vk as u32,
            state,
            result_tx,
        })
        .map_err(|_| AppError::Other("RegisterHotKey command channel closed".into()))?;

    // Wake the backend thread
    let tid = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.as_ref().map(|b| b.thread_id)
    };
    if let Some(tid) = tid {
        // SAFETY: PostThreadMessageW takes only integers; if the target thread has exited, the call fails without touching memory.
        unsafe { PostThreadMessageW(tid, WM_NULL, 0, 0) };
    }

    result_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| AppError::Other("RegisterHotKey registration timed out".into()))?
        .map_err(AppError::Other)
}

/// Unregister a hotkey from the RegisterHotKey backend.
#[cfg(target_os = "windows")]
pub(super) fn unregister_via_reg_hotkey(kind: HotkeyKind) {
    let bundle = get_unified_hook_states();
    let state = match kind {
        HotkeyKind::Dictation => bundle.dictation,
        HotkeyKind::Translation => bundle.translation,
        HotkeyKind::Assistant => bundle.assistant,
    };
    if let Some(id) = state.and_then(|state| state.reg_hotkey_id) {
        unregister_reg_hotkey_id(id);
    }
}

#[cfg(target_os = "windows")]
pub(super) fn unregister_reg_hotkey_id(id: i32) {
    let _ = reg_hotkey_cmd_channel()
        .0
        .send(RegHotkeyCmd::Unregister { id });

    let tid = {
        let guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.as_ref().map(|b| b.thread_id)
    };
    if let Some(tid) = tid {
        // SAFETY: PostThreadMessageW takes only integers; if the target thread has exited, the call fails without touching memory.
        unsafe { PostThreadMessageW(tid, WM_NULL, 0, 0) };
    }
}

#[cfg(target_os = "windows")]
pub(super) fn stop_reg_hotkey_backend() {
    let backend = {
        let mut guard = match reg_backend_slot().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.take()
    };

    if let Some(backend) = backend {
        // SAFETY: PostThreadMessageW takes only integers; if the backend thread has exited, the call fails without touching memory.
        let _ = unsafe { PostThreadMessageW(backend.thread_id, WM_QUIT, 0, 0) };
        let _ = backend.handle.join();
    }
}

// ---------------------------------------------------------------------------
