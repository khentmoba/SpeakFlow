use super::*;

// HotkeySpec — describes what key combination triggers the hotkey
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(super) enum HotkeySpec {
    /// Pure modifier-key combination (e.g. Ctrl+Win, Alt alone)
    ModifierOnly {
        label: String,
        required_modifiers: Vec<ModifierKey>,
    },
    /// Modifier(s) + a main key (e.g. F2, Ctrl+Space)
    Standard {
        label: String,
        modifiers: ShortcutModifiers,
        main_vk: u16,
        /// Side-specific modifiers and lock keys cannot use RegisterHotKey.
        force_low_level_hook: bool,
        /// Consume both key-down and key-up so lock keys do not change OS state.
        consume_main_key: bool,
    },
}

impl HotkeySpec {
    pub(super) fn label(&self) -> &str {
        match self {
            Self::ModifierOnly { label, .. } => label,
            Self::Standard { label, .. } => label,
        }
    }

    pub(super) fn force_low_level_hook(&self) -> bool {
        matches!(
            self,
            Self::ModifierOnly { .. }
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
// Event gate — deduplicates press/release across rapid repeats
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Key parsing
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// normalize_shortcut → HotkeySpec
// ---------------------------------------------------------------------------

pub(super) fn normalize_shortcut(raw: &str) -> Result<HotkeySpec, AppError> {
    let mut modifiers = ShortcutModifiers::default();
    let mut main_key: Option<String> = None;

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
        } else {
            if main_key.is_some() {
                return Err(AppError::Other(
                    "Invalid shortcut format: only one main key is allowed (for example Ctrl+Win+R)".to_string(),
                ));
            }
            main_key = Some(token.to_string());
        }
    }

    // Build ordered label
    let mut label_parts = Vec::with_capacity(5);
    label_parts.extend(
        modifiers
            .ordered_keys()
            .map(|modifier| modifier.label().to_string()),
    );

    if let Some(ref mk) = main_key {
        label_parts.push(mk.clone());

        #[cfg(target_os = "windows")]
        let main_vk = parse_main_key_to_vk(mk)
            .ok_or_else(|| AppError::Other(format!("Unrecognized key: {}", mk)))?;
        #[cfg(not(target_os = "windows"))]
        let main_vk = 0u16;

        let label = label_parts.join("+");
        let consume_main_key = mk.eq_ignore_ascii_case("CapsLock");
        let force_low_level_hook = modifiers.has_side_specific() || consume_main_key;
        Ok(HotkeySpec::Standard {
            label,
            modifiers,
            main_vk,
            force_low_level_hook,
            consume_main_key,
        })
    } else {
        // Pure modifier-only hotkey
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
