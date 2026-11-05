use super::*;

// ---------------------------------------------------------------------------
// Unified low-level keyboard hook (Windows)
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub(super) struct UnifiedHookState {
    pub(super) app_handle: tauri::AppHandle,
    pub(super) spec: HotkeySpec,
    pub(super) trigger: RecordingTrigger,
    pub(super) gate: Arc<HotkeyEventGate>,
    pub(super) backend: HotkeyBackend,
    pub(super) reg_hotkey_id: Option<i32>,
    /// Per-VK key-down tracking for modifier-only mode
    pub(super) key_down: Vec<AtomicBool>,
    /// Hotkey combination currently active
    pub(super) activated: AtomicBool,
    /// Hotkey locked via double-tap (hands-free recording)
    pub(super) locked: AtomicBool,
    /// Timestamp of last release in ms for double-tap detection
    pub(super) last_release_ms: AtomicU64,
    /// A non-hotkey key was pressed during modifier-only hold → taint (cancel)
    pub(super) tainted: AtomicBool,
    /// A modifier key leaked to OS before the combo was complete
    pub(super) modifier_leaked: AtomicBool,
}

#[cfg(target_os = "windows")]
#[derive(Default, Clone)]
pub(super) struct UnifiedHookBundle {
    pub(super) dictation: Option<Arc<UnifiedHookState>>,
    pub(super) translation: Option<Arc<UnifiedHookState>>,
    pub(super) assistant: Option<Arc<UnifiedHookState>>,
}

#[cfg(target_os = "windows")]
impl UnifiedHookBundle {
    pub(super) fn is_empty(&self) -> bool {
        self.dictation.is_none() && self.translation.is_none() && self.assistant.is_none()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HotkeyKind {
    Dictation,
    Translation,
    Assistant,
}

#[cfg(target_os = "windows")]
pub(super) fn unified_hook_state_slot() -> &'static Mutex<UnifiedHookBundle> {
    static SLOT: OnceLock<Mutex<UnifiedHookBundle>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(UnifiedHookBundle::default()))
}

#[cfg(target_os = "windows")]
pub(super) fn unified_hook_state_snapshot() -> &'static arc_swap::ArcSwap<UnifiedHookBundle> {
    static SNAPSHOT: OnceLock<arc_swap::ArcSwap<UnifiedHookBundle>> = OnceLock::new();
    SNAPSHOT.get_or_init(|| arc_swap::ArcSwap::from_pointee(UnifiedHookBundle::default()))
}

#[cfg(target_os = "windows")]
pub(super) fn publish_unified_hook_state_snapshot(bundle: &UnifiedHookBundle) {
    unified_hook_state_snapshot().store(Arc::new(bundle.clone()));
}

#[cfg(target_os = "windows")]
pub(super) fn set_unified_hook_state(
    kind: HotkeyKind,
    state: Option<Arc<UnifiedHookState>>,
) -> Option<Arc<UnifiedHookState>> {
    let mut guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let slot = match kind {
        HotkeyKind::Dictation => &mut guard.dictation,
        HotkeyKind::Translation => &mut guard.translation,
        HotkeyKind::Assistant => &mut guard.assistant,
    };
    let previous = std::mem::replace(slot, state);
    publish_unified_hook_state_snapshot(&guard);
    previous
}

#[cfg(target_os = "windows")]
pub(super) fn get_unified_hook_states() -> UnifiedHookBundle {
    let guard = match unified_hook_state_slot().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.clone()
}

#[cfg(target_os = "windows")]
pub(super) const SYNTHETIC_KEY_MARKER: usize = 0x4C575F53; // "LW_S"

#[cfg(target_os = "windows")]
pub(super) fn send_synthetic_key_up(vk: u16) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: SYNTHETIC_KEY_MARKER,
            },
        },
    };

    // SAFETY: input is a live INPUT local, and the size passed is size_of::<INPUT>().
    unsafe {
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(target_os = "windows")]
pub(super) fn is_modifier_vk(vk: u32) -> bool {
    matches!(
        vk as u16,
        x if x == VK_LCONTROL
            || x == VK_RCONTROL
            || x == VK_LMENU
            || x == VK_RMENU
            || x == VK_LSHIFT
            || x == VK_RSHIFT
            || x == VK_LWIN
            || x == VK_RWIN
    )
}

#[cfg(target_os = "windows")]
pub(super) fn is_win_vk(vk: u32) -> bool {
    vk as u16 == VK_LWIN || vk as u16 == VK_RWIN
}

#[cfg(target_os = "windows")]
pub(super) fn is_alt_vk(vk: u32) -> bool {
    vk as u16 == VK_LMENU || vk as u16 == VK_RMENU
}

/// Map a VK code to an index in UnifiedHookState::key_down for ModifierOnly specs.
/// Returns None if the physical key is not one of the required modifiers.
#[cfg(target_os = "windows")]
pub(super) fn vk_to_required_index(spec: &HotkeySpec, vk: u32) -> Option<usize> {
    if let HotkeySpec::ModifierOnly {
        required_modifiers, ..
    } = spec
    {
        required_modifiers
            .iter()
            .position(|required| required.matches_vk(vk))
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
pub(super) fn accepts_right_alt(required_modifiers: &[ModifierKey]) -> bool {
    required_modifiers
        .iter()
        .any(|required| required.matches_vk(VK_RMENU as u32))
}

// --- Standard mode helpers ---

#[cfg(target_os = "windows")]
pub(super) fn is_key_physically_down(vk: u16) -> bool {
    // SAFETY: GetAsyncKeyState takes only an integer virtual-key code and no pointers, so any value is valid.
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) < 0 }
}

#[cfg(target_os = "windows")]
pub(super) fn all_modifiers_down(mods: &ShortcutModifiers) -> bool {
    mods.keys.iter().all(|key| key.is_physically_down())
}

// ---------------------------------------------------------------------------
// The unified hook callback
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
pub(super) unsafe extern "system" fn unified_low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    // SAFETY: Forwards the unchanged arguments Windows gave this hook; CallNextHookEx ignores the null hook handle.
    let pass_through = || unsafe { CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param) };

    if n_code != HC_ACTION as i32 {
        return pass_through();
    }

    let bundle = unified_hook_state_snapshot().load_full();
    if bundle.is_empty() {
        return pass_through();
    }

    // SAFETY: n_code is HC_ACTION here, so Windows passes lParam as a valid KBDLLHOOKSTRUCT for the duration of this callback.
    let keyboard = unsafe { &*(l_param as *const KBDLLHOOKSTRUCT) };

    // Skip our own synthetic keys
    if keyboard.dwExtraInfo == SYNTHETIC_KEY_MARKER {
        return pass_through();
    }

    // Filter injected input (macro tools, automation, drivers).
    // LLKHF_INJECTED = 0x10 — set by SendInput / keybd_event from other processes.
    if keyboard.flags & 0x10 != 0 {
        IGNORED_INJECTED_COUNT.fetch_add(1, Ordering::Relaxed);
        return pass_through();
    }

    let message = w_param as u32;
    let is_key_down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if !is_key_down && !is_key_up {
        return pass_through();
    }

    let vk = keyboard.vkCode;

    // 1. Universal Escape Abort: Cancel active recording immediately without pasting
    if vk == VK_ESCAPE as u32 && is_key_down {
        let any_active = [
            bundle.dictation.as_ref(),
            bundle.translation.as_ref(),
            bundle.assistant.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|s| s.activated.load(Ordering::Acquire) || s.locked.load(Ordering::Acquire));

        if any_active {
            for state in [
                bundle.dictation.as_ref(),
                bundle.translation.as_ref(),
                bundle.assistant.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                state.locked.store(false, Ordering::Release);
                state.activated.store(false, Ordering::Release);
                send_dispatch(DispatchEvent::Cancel(
                    state.clone(),
                    "Escape pressed: cancelling active recording".to_string(),
                ));
            }
            return 1;
        }
    }

    // 2. Paste Last Dictation: Ctrl + Alt + V global shortcut
    if vk == 0x56 /* VK_V */ && is_key_down {
        // SAFETY: GetAsyncKeyState takes only an integer virtual-key code and no pointers, so any value is valid.
        let ctrl_down = unsafe { (GetAsyncKeyState(VK_CONTROL as i32) as u16 & 0x8000) != 0 };
        // SAFETY: GetAsyncKeyState takes only an integer virtual-key code and no pointers, so any value is valid.
        let alt_down = unsafe { (GetAsyncKeyState(VK_MENU as i32) as u16 & 0x8000) != 0 };
        // SAFETY: GetAsyncKeyState takes only an integer virtual-key code and no pointers, so any value is valid.
        let shift_down = unsafe { (GetAsyncKeyState(VK_SHIFT as i32) as u16 & 0x8000) != 0 };
        // SAFETY: GetAsyncKeyState takes only an integer virtual-key code and no pointers, so any value is valid.
        let win_down = unsafe {
            (GetAsyncKeyState(VK_LWIN as i32) as u16 & 0x8000) != 0
                || (GetAsyncKeyState(VK_RWIN as i32) as u16 & 0x8000) != 0
        };
        if ctrl_down && alt_down && !shift_down && !win_down {
            if let Some(state) = bundle
                .dictation
                .as_ref()
                .or(bundle.translation.as_ref())
                .or(bundle.assistant.as_ref())
            {
                send_dispatch(DispatchEvent::PasteLast(state.clone()));
                return 1;
            }
        }
    }

    let mut swallow = false;
    for state in [
        bundle.dictation.as_ref(),
        bundle.translation.as_ref(),
        bundle.assistant.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| s.backend == HotkeyBackend::LowLevelHook)
    {
        swallow |= match &state.spec {
            HotkeySpec::ModifierOnly {
                label,
                required_modifiers,
            } => handle_modifier_only_event(state, vk, is_key_down, label, required_modifiers),
            HotkeySpec::Standard {
                label,
                modifiers,
                main_vk,
                consume_main_key,
                ..
            } => handle_standard_event(
                state,
                vk,
                is_key_down,
                label,
                modifiers,
                *main_vk,
                *consume_main_key,
            ),
        };
    }

    if swallow {
        1
    } else {
        pass_through()
    }
}

/// Returns `true` if the event should be swallowed (not forwarded to OS).
/// All heavy work (mutex, IPC) is dispatched via channel — the hook thread
/// only touches atomics.
#[cfg(target_os = "windows")]
pub(super) fn handle_modifier_only_event(
    state: &Arc<UnifiedHookState>,
    vk: u32,
    is_key_down: bool,
    label: &str,
    required_modifiers: &[ModifierKey],
) -> bool {
    let is_required = vk_to_required_index(&state.spec, vk).is_some();
    // Windows represents AltGr as a synthetic Left Ctrl followed by Right Alt.
    // Do not taint a Right Alt binding with that companion event.
    let is_altgr_ctrl_companion = vk as u16 == VK_LCONTROL && accepts_right_alt(required_modifiers);
    let was_activated = state.activated.load(Ordering::Acquire);

    if is_key_down {
        if is_required {
            if let Some(idx) = vk_to_required_index(&state.spec, vk) {
                state.key_down[idx].store(true, Ordering::Release);
            }

            let all_down = state.key_down.iter().all(|b| b.load(Ordering::Acquire));
            if all_down && !was_activated && !state.tainted.load(Ordering::Acquire) {
                let is_locked = state.locked.load(Ordering::Acquire);
                if is_locked {
                    state.locked.store(false, Ordering::Release);
                    state.activated.store(false, Ordering::Release);
                    send_dispatch(DispatchEvent::Release(
                        state.clone(),
                        format!("{} 解锁停止录音", label),
                    ));
                } else {
                    let now = now_unix_ms();
                    let last_rel = state.last_release_ms.load(Ordering::Acquire);
                    if now.saturating_sub(last_rel) < 350 {
                        log::info!("Double-tap lock engaged for {}", label);
                        state.locked.store(true, Ordering::Release);
                    }
                    state.activated.store(true, Ordering::Release);
                    send_dispatch(DispatchEvent::Press(
                        state.clone(),
                        format!("{} 按下，开始录音", label),
                    ));
                }
            }
        } else if !is_altgr_ctrl_companion {
            // Non-required key pressed → taint
            state.tainted.store(true, Ordering::Release);
            if was_activated {
                state.activated.store(false, Ordering::Release);
                send_dispatch(DispatchEvent::Release(
                    state.clone(),
                    format!("{} 被非热键按键打断，停止录音", label),
                ));
            }
        }
    } else {
        // Key up
        if is_required {
            if let Some(idx) = vk_to_required_index(&state.spec, vk) {
                state.key_down[idx].store(false, Ordering::Release);
            }

            if was_activated {
                if state.locked.load(Ordering::Acquire) {
                    // Double-tap lock is active; keep recording hands-free
                } else {
                    state
                        .last_release_ms
                        .store(now_unix_ms(), Ordering::Release);
                    state.activated.store(false, Ordering::Release);
                    send_dispatch(DispatchEvent::Release(
                        state.clone(),
                        format!("{} 松开，停止录音", label),
                    ));
                }
            }
        }

        // When all required modifiers are up, clear taint and handle cleanup
        let any_still_down = state.key_down.iter().any(|b| b.load(Ordering::Acquire));
        if !any_still_down {
            let need_cleanup = was_activated && state.modifier_leaked.swap(false, Ordering::AcqRel);
            state.tainted.store(false, Ordering::Release);
            state.activated.store(false, Ordering::Release);

            if need_cleanup {
                for required in required_modifiers {
                    if required.is_super() {
                        log::debug!("补发合成 Win key-up 修复 OS 修饰键状态");
                        send_synthetic_key_up(VK_LWIN);
                        send_synthetic_key_up(VK_RWIN);
                    }
                    if required.is_alt() {
                        log::debug!("补发合成 Alt key-up 修复 OS 修饰键状态");
                        send_synthetic_key_up(VK_LMENU);
                        send_synthetic_key_up(VK_RMENU);
                    }
                }
            }
        }
    }

    // Decide whether to swallow — must happen AFTER state updates above
    let swallow = should_swallow_modifier_only(state, vk);

    // Track modifier leak: key-down of a required modifier that we didn't swallow
    if is_key_down && is_required && !swallow && is_modifier_vk(vk) {
        state.modifier_leaked.store(true, Ordering::Release);
    }

    swallow
}

#[cfg(target_os = "windows")]
pub(super) fn should_swallow_modifier_only(state: &UnifiedHookState, vk: u32) -> bool {
    let is_required = vk_to_required_index(&state.spec, vk).is_some();
    if !is_required {
        return false;
    }

    let currently_activated = state.activated.load(Ordering::Acquire);
    let any_required_down = state.key_down.iter().any(|b| b.load(Ordering::Acquire));

    // For Win/Alt keys: swallow whenever any required key is down or was activated.
    // This prevents Win from opening start menu, Alt from activating menus.
    if is_win_vk(vk) || is_alt_vk(vk) {
        return currently_activated || any_required_down;
    }

    // For other modifiers (Ctrl, Shift): only swallow when activated
    currently_activated
}

/// Returns `true` if the event should be swallowed.
/// All heavy work (mutex, IPC) is dispatched via channel.
#[cfg(target_os = "windows")]
pub(super) fn handle_standard_event(
    state: &Arc<UnifiedHookState>,
    vk: u32,
    is_key_down: bool,
    label: &str,
    modifiers: &ShortcutModifiers,
    main_vk: u16,
    consume_main_key: bool,
) -> bool {
    let is_main_key = vk as u16 == main_vk;

    if is_key_down && is_main_key {
        let was_activated = state.activated.load(Ordering::Acquire);
        if all_modifiers_down(modifiers) {
            let is_locked = state.locked.load(Ordering::Acquire);
            if is_locked {
                state.locked.store(false, Ordering::Release);
                state.activated.store(false, Ordering::Release);
                send_dispatch(DispatchEvent::Release(
                    state.clone(),
                    format!("{} 解锁停止录音", label),
                ));
            } else if !was_activated {
                let now = now_unix_ms();
                let last_rel = state.last_release_ms.load(Ordering::Acquire);
                if now.saturating_sub(last_rel) < 350 {
                    log::info!("Double-tap lock engaged for {}", label);
                    state.locked.store(true, Ordering::Release);
                }
                state.activated.store(true, Ordering::Release);
                send_dispatch(DispatchEvent::Press(
                    state.clone(),
                    format!("{} 按下，开始录音", label),
                ));
            }
        }
    } else if !is_key_down && state.activated.load(Ordering::Acquire) {
        let combo_broken = is_main_key || !all_modifiers_down(modifiers);
        if combo_broken {
            if state.locked.load(Ordering::Acquire) {
                // Double-tap lock is active; keep recording hands-free
            } else {
                state
                    .last_release_ms
                    .store(now_unix_ms(), Ordering::Release);
                state.activated.store(false, Ordering::Release);
                send_dispatch(DispatchEvent::Release(
                    state.clone(),
                    format!("{} 松开，停止录音", label),
                ));

                // Clean up leaked modifier state in one read
                let leaked = state.modifier_leaked.swap(false, Ordering::AcqRel);
                if leaked {
                    if modifiers.has_super() {
                        send_synthetic_key_up(VK_LWIN);
                        send_synthetic_key_up(VK_RWIN);
                    }
                    if modifiers.has_alt() {
                        send_synthetic_key_up(VK_LMENU);
                        send_synthetic_key_up(VK_RMENU);
                    }
                }
            }
        }
    }

    // Read activated state AFTER updates — swallow the main key while hotkey is active.
    // This correctly swallows:
    //   - The triggering key-down (activated just set to true above)
    //   - Repeated key-downs while held
    // And does NOT swallow:
    //   - The key-up that deactivated (activated just set to false above)
    let currently_activated = state.activated.load(Ordering::Acquire);

    // Track modifier leaks for Win/Alt (key-down that we won't swallow)
    if is_key_down
        && !currently_activated
        && ((is_win_vk(vk) && modifiers.has_super()) || (is_alt_vk(vk) && modifiers.has_alt()))
    {
        state.modifier_leaked.store(true, Ordering::Release);
    }

    is_main_key && (consume_main_key || currently_activated)
}
