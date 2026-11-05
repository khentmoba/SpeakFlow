/// The hotkey module is split across files; source-contract checks scan all production files.
const PRODUCTION_SOURCE: &str = concat!(
    include_str!("../hotkey.rs"),
    include_str!("unified_hook.rs"),
    include_str!("reg_backend.rs"),
    include_str!("monitor.rs"),
    include_str!("spec.rs"),
);

#[cfg(target_os = "windows")]
use super::{accepts_right_alt, VK_CAPITAL, VK_LCONTROL, VK_LMENU, VK_RCONTROL, VK_RMENU};
use super::{
    is_ignorable_start_audio_error, normalize_shortcut, reset_hotkey_event_gate, HotkeyEventGate,
    HotkeySpec, ModifierKey, RECORDING_ALREADY_ACTIVE_ERROR, RECORDING_NOT_READY_ERROR,
    RECORDING_START_CANCELLED_ERROR,
};
use std::sync::atomic::Ordering;

#[cfg(target_os = "windows")]
#[test]
fn registration_ids_are_not_reused_for_the_same_hotkey() {
    let first = super::next_reg_hotkey_id().unwrap();
    let second = super::next_reg_hotkey_id().unwrap();
    assert_ne!(
        first, second,
        "queued WM_HOTKEY must keep its original registration identity"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn registration_id_exhaustion_does_not_wrap() {
    let next = super::AtomicU32::new(0xBFFF);
    assert_eq!(super::allocate_reg_hotkey_id(&next), Some(0xBFFF));
    assert_eq!(super::allocate_reg_hotkey_id(&next), None);
    assert_eq!(super::allocate_reg_hotkey_id(&next), None);
}

#[test]
fn mode_changes_replace_gates_even_without_backend_migration() {
    let source = PRODUCTION_SOURCE;
    let body = source
        .split("pub async fn set_recording_mode(")
        .nth(1)
        .unwrap();
    let body = body.split("#[tauri::command]").next().unwrap();
    assert!(
        !body.contains("if new_backend != old_state.backend"),
        "a same-backend mode switch must also invalidate queued presses"
    );
    assert!(!body.contains("release_hotkey_registration(old_state, false)"));
}

#[test]
fn failed_registration_does_not_restore_retired_arcs() {
    let source = PRODUCTION_SOURCE;
    let production = source.split("mod tests {").next().unwrap();
    assert!(!production.contains("set_unified_hook_state(kind, previous_state)"));
    assert!(!production.contains("set_unified_hook_state(HotkeyKind::Translation, previous_state)"));
    assert!(!production.contains("set_unified_hook_state(HotkeyKind::Assistant, previous_state)"));
}

#[test]
fn native_success_and_mode_change_do_not_swallow_lifecycle_errors() {
    let source = PRODUCTION_SOURCE;
    let native = source
        .split("fn register_on_chosen_backend(")
        .nth(1)
        .unwrap();
    let success = native
        .split("Ok(()) => {")
        .nth(1)
        .unwrap()
        .split("Err(reg_err) => {")
        .next()
        .unwrap();
    assert!(!success.contains("let _ = sync_hotkey_monitor_lifecycle"));
    let mode = source
        .split("pub async fn set_recording_mode(")
        .nth(1)
        .unwrap()
        .split("#[tauri::command]")
        .next()
        .unwrap();
    assert!(!mode.contains("let _ = sync_hotkey_monitor_lifecycle"));
}

#[test]
fn quick_cancel_is_not_rebroadcast_as_a_start_error() {
    assert!(is_ignorable_start_audio_error(
        RECORDING_START_CANCELLED_ERROR
    ));
    assert!(!is_ignorable_start_audio_error("麦克风不可用"));
}

#[test]
fn all_expected_start_audio_races_are_ignored_but_other_audio_errors_are_not() {
    for message in [
        RECORDING_NOT_READY_ERROR,
        RECORDING_ALREADY_ACTIVE_ERROR,
        RECORDING_START_CANCELLED_ERROR,
    ] {
        assert!(
            is_ignorable_start_audio_error(message),
            "known start race should be ignored: {message}"
        );
    }
    assert!(!is_ignorable_start_audio_error("麦克风不可用"));
}

#[test]
fn reset_hotkey_event_gate_clears_press_release_and_toggle_state() {
    let gate = HotkeyEventGate::default();
    gate.is_pressed.store(true, Ordering::Relaxed);
    gate.last_release_ms.store(123, Ordering::Relaxed);
    gate.toggle_active.store(true, Ordering::Relaxed);

    reset_hotkey_event_gate(&gate);

    assert!(!gate.is_pressed.load(Ordering::Relaxed));
    assert_eq!(gate.last_release_ms.load(Ordering::Relaxed), 0);
    assert!(!gate.toggle_active.load(Ordering::Relaxed));
}

#[test]
fn low_level_hook_callback_does_not_lock_state_slot() {
    let source = PRODUCTION_SOURCE;
    let hook_start = source
        .find("fn unified_low_level_keyboard_proc")
        .expect("low-level hook callback should exist");
    let hook_rest = &source[hook_start..];
    let hook_end = hook_rest
        .find("/// Returns `true` if the event should be swallowed")
        .expect("hook callback section should end before helper docs");
    let hook_body = &hook_rest[..hook_end];

    assert!(
            !hook_body.contains("get_unified_hook_states()")
                && !hook_body.contains("unified_hook_state_slot()")
                && !hook_body.contains(".lock()"),
            "WH_KEYBOARD_LL callback must read lock-free state only; mutex locking in the hook path can stall global keyboard input"
        );
}

#[cfg(target_os = "windows")]
#[test]
fn side_specific_ctrl_is_not_collapsed_to_generic_ctrl() {
    let left = normalize_shortcut("LeftCtrl").expect("Left Ctrl should parse");
    let generic = normalize_shortcut("Ctrl").expect("generic Ctrl should parse");

    let HotkeySpec::ModifierOnly {
        required_modifiers: left_required,
        ..
    } = left
    else {
        panic!("Left Ctrl should be a modifier-only shortcut");
    };
    assert_eq!(left_required, vec![ModifierKey::LeftCtrl]);
    assert!(left_required[0].matches_vk(VK_LCONTROL as u32));
    assert!(!left_required[0].matches_vk(VK_RCONTROL as u32));

    let HotkeySpec::ModifierOnly {
        required_modifiers: generic_required,
        ..
    } = generic
    else {
        panic!("Ctrl should be a modifier-only shortcut");
    };
    assert!(generic_required[0].matches_vk(VK_LCONTROL as u32));
    assert!(generic_required[0].matches_vk(VK_RCONTROL as u32));
}

#[cfg(target_os = "windows")]
#[test]
fn right_alt_accepts_altgr_companion_but_not_left_alt() {
    let spec = normalize_shortcut("RightAlt").expect("Right Alt should parse");
    let HotkeySpec::ModifierOnly {
        required_modifiers, ..
    } = spec
    else {
        panic!("Right Alt should be a modifier-only shortcut");
    };

    assert!(accepts_right_alt(&required_modifiers));
    assert!(required_modifiers[0].matches_vk(VK_RMENU as u32));
    assert!(!required_modifiers[0].matches_vk(VK_LMENU as u32));
}

#[cfg(target_os = "windows")]
#[test]
fn caps_lock_uses_low_level_hook_and_consumes_native_toggle() {
    let spec = normalize_shortcut("CapsLock").expect("Caps Lock should parse");
    let HotkeySpec::Standard {
        main_vk,
        force_low_level_hook,
        consume_main_key,
        ..
    } = spec
    else {
        panic!("Caps Lock should be a standard main key");
    };

    assert_eq!(main_vk, VK_CAPITAL);
    assert!(force_low_level_hook);
    assert!(consume_main_key);
}

#[cfg(target_os = "windows")]
#[test]
fn side_specific_modifier_combinations_force_the_low_level_hook() {
    let spec = normalize_shortcut("RightCtrl+LeftAlt+R")
        .expect("side-specific modifier combination should parse");
    let HotkeySpec::Standard {
        modifiers,
        main_vk,
        force_low_level_hook,
        consume_main_key,
        ..
    } = spec
    else {
        panic!("modifier plus R should be a standard shortcut");
    };

    assert_eq!(
        modifiers.keys,
        vec![ModifierKey::RightCtrl, ModifierKey::LeftAlt]
    );
    assert_eq!(main_vk, b'R' as u16);
    assert!(force_low_level_hook);
    assert!(!consume_main_key);
}

#[cfg(target_os = "windows")]
#[test]
fn legacy_generic_modifier_combinations_keep_the_standard_backend_path() {
    let spec = normalize_shortcut("Ctrl+Alt+F2")
        .expect("legacy generic modifier combination should parse");
    let HotkeySpec::Standard {
        force_low_level_hook,
        consume_main_key,
        ..
    } = spec
    else {
        panic!("modifier plus F2 should be a standard shortcut");
    };

    assert!(!force_low_level_hook);
    assert!(!consume_main_key);
}

#[test]
fn generic_and_side_specific_aliases_cannot_be_mixed() {
    let error = normalize_shortcut("Ctrl+LeftCtrl")
        .expect_err("generic and side-specific Ctrl should conflict")
        .to_string();
    assert!(error.contains("left/right modifiers of the same family"));
}
