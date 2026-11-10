use super::*;

// ---------------------------------------------------------------------------
// HotkeySpec — describes what key/mouse combination triggers the hotkey
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MouseButtonKind {
    Middle,
    XButton1,
    XButton2,
}

impl MouseButtonKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Middle => "Mouse3",
            Self::XButton1 => "Mouse4",
            Self::XButton2 => "Mouse5",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum HotkeySpec {
    /// Pure modifier-key combination (e.g. Ctrl+Win, Alt alone)
    ModifierOnly {
        label: String,
        required_modifiers: Vec<ModifierKey>,
    },
    /// Modifier(s) + a main key (e.g. F2, Ctrl+Space, CapsLock, NumLock)
    Standard {
        label: String,
        modifiers: ShortcutModifiers,
        main_vk: u16,
        /// Side-specific modifiers and lock keys cannot use RegisterHotKey.
        force_low_level_hook: bool,
        /// Consume both key-down and key-up so lock keys do not change OS state.
        consume_main_key: bool,
    },
    /// Mouse button (Mouse3/Middle, Mouse4/XButton1, Mouse5/XButton2), with optional modifiers
    Mouse {
        label: String,
        modifiers: ShortcutModifiers,
        button: MouseButtonKind,
    },
}

impl HotkeySpec {
    pub(super) fn label(&self) -> &str {
        match self {
            Self::ModifierOnly { label, .. } => label,
            Self::Standard { label, .. } => label,
            Self::Mouse { label, .. } => label,
        }
    }

    pub(super) fn force_low_level_hook(&self) -> bool {
        matches!(
            self,
            Self::ModifierOnly { .. }
                | Self::Mouse { .. }
                | Self::Standard {
                    force_low_level_hook: true,
                    ..
                }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModifierKey {
    Ctrl,
    LeftCtrl,
    RightCtrl,
    Alt,
    LeftAlt,
    RightAlt,
    Shift,
    Super,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModifierFamily {
    Ctrl,
    Alt,
    Shift,
    Super,
}

pub(super) const MODIFIER_ORDER: [ModifierKey; 8] = [
    ModifierKey::Ctrl,
    ModifierKey::LeftCtrl,
    ModifierKey::RightCtrl,
    ModifierKey::Alt,
    ModifierKey::LeftAlt,
    ModifierKey::RightAlt,
    ModifierKey::Shift,
    ModifierKey::Super,
];

impl ModifierKey {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::LeftCtrl => "Left Ctrl",
            Self::RightCtrl => "Right Ctrl",
            Self::Alt => "Alt",
            Self::LeftAlt => "Left Alt",
            Self::RightAlt => "Right Alt",
            Self::Shift => "Shift",
            Self::Super => "Win",
        }
    }

    pub(super) fn family(self) -> ModifierFamily {
        match self {
            Self::Ctrl | Self::LeftCtrl | Self::RightCtrl => ModifierFamily::Ctrl,
            Self::Alt | Self::LeftAlt | Self::RightAlt => ModifierFamily::Alt,
            Self::Shift => ModifierFamily::Shift,
            Self::Super => ModifierFamily::Super,
        }
    }

    pub(super) fn is_generic(self) -> bool {
        matches!(self, Self::Ctrl | Self::Alt | Self::Shift | Self::Super)
    }

    #[cfg(target_os = "windows")]
    pub(super) fn matches_vk(self, physical_vk: u32) -> bool {
        let vk = physical_vk as u16;
        match self {
            Self::Ctrl => vk == VK_LCONTROL || vk == VK_RCONTROL,
            Self::LeftCtrl => vk == VK_LCONTROL,
            Self::RightCtrl => vk == VK_RCONTROL,
            Self::Alt => vk == VK_LMENU || vk == VK_RMENU,
            Self::LeftAlt => vk == VK_LMENU,
            Self::RightAlt => vk == VK_RMENU,
            Self::Shift => vk == VK_LSHIFT || vk == VK_RSHIFT,
            Self::Super => vk == VK_LWIN || vk == VK_RWIN,
        }
    }

    #[cfg(target_os = "windows")]
    pub(super) fn is_physically_down(self) -> bool {
        match self {
            Self::Ctrl => {
                is_key_physically_down(VK_LCONTROL) || is_key_physically_down(VK_RCONTROL)
            }
            Self::LeftCtrl => is_key_physically_down(VK_LCONTROL),
            Self::RightCtrl => is_key_physically_down(VK_RCONTROL),
            Self::Alt => is_key_physically_down(VK_LMENU) || is_key_physically_down(VK_RMENU),
            Self::LeftAlt => is_key_physically_down(VK_LMENU),
            Self::RightAlt => is_key_physically_down(VK_RMENU),
            Self::Shift => is_key_physically_down(VK_LSHIFT) || is_key_physically_down(VK_RSHIFT),
            Self::Super => is_key_physically_down(VK_LWIN) || is_key_physically_down(VK_RWIN),
        }
    }

    pub(super) fn is_alt(self) -> bool {
        matches!(self, Self::Alt | Self::LeftAlt | Self::RightAlt)
    }

    pub(super) fn is_super(self) -> bool {
        self == Self::Super
    }
}

#[derive(Clone, Default, Debug)]
pub(super) struct ShortcutModifiers {
    pub(super) keys: Vec<ModifierKey>,
}

impl ShortcutModifiers {
    pub(super) fn insert(&mut self, key: ModifierKey) -> Result<(), AppError> {
        if self.keys.contains(&key) {
            return Ok(());
        }
        if self.keys.iter().any(|existing| {
            existing.family() == key.family() && (existing.is_generic() || key.is_generic())
        }) {
            return Err(AppError::Other(format!(
                "Invalid shortcut format: {} cannot be combined with left/right modifiers of the same family",
                key.label()
            )));
        }
        self.keys.push(key);
        Ok(())
    }

    pub(super) fn ordered_keys(&self) -> impl Iterator<Item = ModifierKey> + '_ {
        MODIFIER_ORDER
            .into_iter()
            .filter(|key| self.keys.contains(key))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub(super) fn has_ctrl(&self) -> bool {
        self.keys
            .iter()
            .any(|key| key.family() == ModifierFamily::Ctrl)
    }

    pub(super) fn has_alt(&self) -> bool {
        self.keys
            .iter()
            .any(|key| key.family() == ModifierFamily::Alt)
    }

    pub(super) fn has_shift(&self) -> bool {
        self.keys.contains(&ModifierKey::Shift)
    }

    pub(super) fn has_super(&self) -> bool {
        self.keys.contains(&ModifierKey::Super)
    }

    pub(super) fn has_side_specific(&self) -> bool {
        self.keys.iter().any(|key| !key.is_generic())
    }
}

// ---------------------------------------------------------------------------
// Key parsing
// ---------------------------------------------------------------------------

pub(super) fn parse_mouse_button(token: &str) -> Option<MouseButtonKind> {
    match token.trim().to_ascii_lowercase().as_str() {
        "mouse3" | "middle" | "middleclick" | "mousemiddle" => Some(MouseButtonKind::Middle),
        "mouse4" | "xbutton1" | "mousex1" | "mouseback" => Some(MouseButtonKind::XButton1),
        "mouse5" | "xbutton2" | "mousex2" | "mouseforward" => Some(MouseButtonKind::XButton2),
        _ => None,
    }
}

#[cfg(target_os = "windows")]
pub(super) fn parse_main_key_to_vk(main_key: &str) -> Option<u16> {
    match main_key {
        "Escape" => Some(VK_ESCAPE),
        "Enter" => Some(VK_RETURN),
        "Tab" => Some(VK_TAB),
        "Space" => Some(VK_SPACE),
        "Backspace" => Some(VK_BACK),
        "Delete" => Some(VK_DELETE),
        "Insert" => Some(VK_INSERT),
        "Home" => Some(VK_HOME),
        "End" => Some(VK_END),
        "PageUp" => Some(VK_PRIOR),
        "PageDown" => Some(VK_NEXT),
        "ArrowUp" => Some(VK_UP),
        "ArrowDown" => Some(VK_DOWN),
        "ArrowLeft" => Some(VK_LEFT),
        "ArrowRight" => Some(VK_RIGHT),
        "CapsLock" => Some(VK_CAPITAL),
        "NumLock" => Some(0x90),
        "ScrollLock" => Some(0x91),
        "Pause" => Some(0x13),
        "PrintScreen" => Some(0x2C),
        "ContextMenu" | "Apps" => Some(0x5D),
        "BrowserBack" => Some(0xA6),
        "BrowserForward" => Some(0xA7),
        "MediaPlayPause" => Some(0xB3),
        "VolumeMute" => Some(0xAD),
        "Backquote" => Some(0xC0),
        _ if main_key.len() == 1 => {
            let byte = main_key.as_bytes()[0];
            if byte.is_ascii_uppercase() || byte.is_ascii_digit() {
                Some(byte as u16)
            } else {
                None
            }
        }
        _ if main_key.starts_with('F') => {
            let suffix = main_key.strip_prefix('F')?;
            let index = suffix.parse::<u16>().ok()?;
            if (1..=24).contains(&index) {
                Some(VK_F1 + index - 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_extended_standalone_special_key(key: &str) -> bool {
    matches!(
        key,
        "NumLock"
            | "ScrollLock"
            | "Pause"
            | "PrintScreen"
            | "ContextMenu"
            | "Apps"
            | "BrowserBack"
            | "BrowserForward"
            | "MediaPlayPause"
            | "VolumeMute"
    )
}

// ---------------------------------------------------------------------------
// normalize_shortcut → HotkeySpec
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(super) fn normalize_shortcut(raw: &str) -> Result<HotkeySpec, AppError> {
    normalize_shortcut_with_options(raw, true)
}

pub(super) fn normalize_shortcut_with_options(
    raw: &str,
    allow_standalone_special_keys: bool,
) -> Result<HotkeySpec, AppError> {
    let mut modifiers = ShortcutModifiers::default();
    let mut main_key: Option<String> = None;
    let mut mouse_button: Option<MouseButtonKind> = None;

    for token in raw.split('+').map(str::trim) {
        if token.is_empty() {
            return Err(AppError::Other(
                "Invalid shortcut format: contains an empty key token; please set it again"
                    .to_string(),
            ));
        }

        let modifier = match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(ModifierKey::Ctrl),
            "leftctrl" | "ctrlleft" | "leftcontrol" | "controlleft" => Some(ModifierKey::LeftCtrl),
            "rightctrl" | "ctrlright" | "rightcontrol" | "controlright" => {
                Some(ModifierKey::RightCtrl)
            }
            "alt" | "option" | "altgraph" => Some(ModifierKey::Alt),
            "leftalt" | "altleft" | "leftoption" => Some(ModifierKey::LeftAlt),
            "rightalt" | "altright" | "rightoption" => Some(ModifierKey::RightAlt),
            "shift" => Some(ModifierKey::Shift),
            "super" | "meta" | "win" | "windows" | "cmd" | "command" | "os" => {
                Some(ModifierKey::Super)
            }
            _ => None,
        };

        if let Some(modifier) = modifier {
            modifiers.insert(modifier)?;
        } else if let Some(btn) = parse_mouse_button(token) {
            if main_key.is_some() || mouse_button.is_some() {
                return Err(AppError::Other(
                    "Invalid shortcut format: only one main key or mouse button is allowed"
                        .to_string(),
                ));
            }
            mouse_button = Some(btn);
        } else {
            if main_key.is_some() || mouse_button.is_some() {
                return Err(AppError::Other(
                    "Invalid shortcut format: only one main key is allowed (for example Ctrl+Win+R)".to_string(),
                ));
            }
            main_key = Some(token.to_string());
        }
    }

    let mut label_parts = Vec::with_capacity(5);
    label_parts.extend(
        modifiers
            .ordered_keys()
            .map(|modifier| modifier.label().to_string()),
    );

    if let Some(button) = mouse_button {
        label_parts.push(button.label().to_string());
        return Ok(HotkeySpec::Mouse {
            label: label_parts.join("+"),
            modifiers,
            button,
        });
    }

    if let Some(ref mk) = main_key {
        if modifiers.is_empty()
            && is_extended_standalone_special_key(mk)
            && !allow_standalone_special_keys
        {
            return Err(AppError::Other(format!(
                "Special key '{}' without modifiers requires enabling Labs > Standalone special keys",
                mk
            )));
        }

        label_parts.push(mk.clone());

        #[cfg(target_os = "windows")]
        let main_vk = parse_main_key_to_vk(mk)
            .ok_or_else(|| AppError::Other(format!("Unrecognized key: {}", mk)))?;
        #[cfg(not(target_os = "windows"))]
        let main_vk = 0u16;

        let label = label_parts.join("+");
        let consume_main_key = mk.eq_ignore_ascii_case("CapsLock")
            || mk.eq_ignore_ascii_case("NumLock")
            || mk.eq_ignore_ascii_case("ScrollLock");
        let force_low_level_hook = modifiers.has_side_specific()
            || consume_main_key
            || is_extended_standalone_special_key(mk);
        Ok(HotkeySpec::Standard {
            label,
            modifiers,
            main_vk,
            force_low_level_hook,
            consume_main_key,
        })
    } else {
        if modifiers.is_empty() {
            return Err(AppError::Other(
                "Invalid shortcut format: please specify at least one key".to_string(),
            ));
        }

        #[cfg(target_os = "windows")]
        let required_modifiers = modifiers.ordered_keys().collect();
        #[cfg(not(target_os = "windows"))]
        let required_modifiers = Vec::new();

        let label = label_parts.join("+");
        Ok(HotkeySpec::ModifierOnly {
            label,
            required_modifiers,
        })
    }
}
