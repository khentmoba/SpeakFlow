//! Active Windows keyboard-layout detection and automatic ASR engine routing.
//!
//! Reads the foreground thread's `HKL` via `GetKeyboardLayout` and resolves the
//! matching rule from `UserProfile::keyboard_model_rules` (BCP-47 / primary language tag).

use crate::state::user_profile::UserProfile;

/// Reads the active Windows foreground thread's keyboard layout as a lowercase
/// BCP-47 tag (e.g. `"en-us"`, `"zh-cn"`, `"ja-jp"`), or primary language code
/// fallback (`"en"`, `"zh"`, `"ja"`).
pub fn detect_active_keyboard_language() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Globalization::LCIDToLocaleName;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        // SAFETY: Win32 window and thread queries accept null/opaque HWNDs safely.
        unsafe {
            let hwnd = GetForegroundWindow();
            let thread_id = if hwnd.is_null() {
                0
            } else {
                GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
            };
            let hkl = GetKeyboardLayout(thread_id) as usize;
            if hkl == 0 {
                return None;
            }
            let lang_id = (hkl & 0xFFFF) as u32;
            let mut buf = [0u16; 85];
            let len = LCIDToLocaleName(lang_id, buf.as_mut_ptr(), buf.len() as i32, 0);
            if len > 1 {
                let locale = String::from_utf16_lossy(&buf[..(len as usize - 1)]);
                let trimmed = locale.trim().to_ascii_lowercase();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
            primary_lang_id_to_tag((lang_id & 0x03FF) as u16).map(str::to_string)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn primary_lang_id_to_tag(primary: u16) -> Option<&'static str> {
    match primary {
        0x04 => Some("zh"),
        0x07 => Some("de"),
        0x09 => Some("en"),
        0x0A => Some("es"),
        0x0C => Some("fr"),
        0x10 => Some("it"),
        0x11 => Some("ja"),
        0x12 => Some("ko"),
        0x16 => Some("pt"),
        0x19 => Some("ru"),
        _ => None,
    }
}

/// Resolves which ASR engine key should run for `layout_tag` under `profile`.
/// Exact tag matches (e.g. `"zh-cn"`) win over primary-subtag matches (`"zh"`).
/// Returns `None` when keyboard-layout switching is disabled or no rule matches.
pub fn match_engine_for_keyboard(
    profile: &UserProfile,
    layout_tag: Option<&str>,
) -> Option<String> {
    if !profile.keyboard_model_switching || profile.keyboard_model_rules.is_empty() {
        return None;
    }
    let normalized = layout_tag?.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let primary = normalized.split(['-', '_']).next().unwrap_or(&normalized);

    if let Some(exact) = profile
        .keyboard_model_rules
        .iter()
        .find(|rule| rule.layout_tag.trim().to_ascii_lowercase() == normalized)
    {
        return Some(exact.engine.trim().to_string());
    }

    profile
        .keyboard_model_rules
        .iter()
        .find(|rule| rule.layout_tag.trim().to_ascii_lowercase() == primary)
        .map(|rule| rule.engine.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::user_profile::KeyboardModelRule;

    #[test]
    fn matches_exact_locale_before_primary_language_fallback() {
        let mut profile = UserProfile {
            keyboard_model_switching: true,
            keyboard_model_rules: vec![
                KeyboardModelRule {
                    layout_tag: "zh".into(),
                    engine: "qwen3-asr-0.6b".into(),
                },
                KeyboardModelRule {
                    layout_tag: "zh-tw".into(),
                    engine: "whisper-large-v3-turbo".into(),
                },
                KeyboardModelRule {
                    layout_tag: "en".into(),
                    engine: "parakeet-tdt-0.6b-v3-int8".into(),
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            match_engine_for_keyboard(&profile, Some("en-US")).as_deref(),
            Some("parakeet-tdt-0.6b-v3-int8")
        );
        assert_eq!(
            match_engine_for_keyboard(&profile, Some("zh-CN")).as_deref(),
            Some("qwen3-asr-0.6b")
        );
        assert_eq!(
            match_engine_for_keyboard(&profile, Some("zh-TW")).as_deref(),
            Some("whisper-large-v3-turbo")
        );
        assert_eq!(match_engine_for_keyboard(&profile, Some("ja-JP")), None);

        profile.keyboard_model_switching = false;
        assert_eq!(match_engine_for_keyboard(&profile, Some("en-US")), None);
    }
}
