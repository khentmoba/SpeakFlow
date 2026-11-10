use crate::state::user_profile::AppPasteMethod;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolvedPasteStrategy {
    SendInput,
    ClipboardCtrlV {
        restore_clipboard: bool,
    },
    ClipboardCtrlShiftV {
        restore_clipboard: bool,
    },
    /// Shift+Insert, for editors that ignore Ctrl+V but accept the X11-style paste.
    ClipboardShiftInsert {
        restore_clipboard: bool,
    },
    CopyOnly,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PasteDeliveryOptions {
    pub process_name: Option<String>,
    pub paste_method_override: AppPasteMethod,
    pub copy_only: bool,
    pub keep_on_clipboard: bool,
    pub paste_with_shift: bool,
    pub auto_terminal_ctrl_shift_v: bool,
    pub return_to_app: bool,
    pub smart_spacing: bool,
}

/// Returns true when the foreground process is a known terminal emulator where
/// Ctrl+Shift+V (or bracketed clipboard paste) is required instead of Ctrl+V.
pub(crate) fn is_terminal_process(process_name: Option<&str>) -> bool {
    let Some(name) = process_name else {
        return false;
    };
    let lower = name.trim().to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    matches!(
        stem,
        "windowsterminal"
            | "wt"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "conhost"
            | "mintty"
            | "alacritty"
            | "wezterm-gui"
            | "wezterm"
            | "kitty"
            | "hyper"
            | "putty"
            | "tabby"
            | "git-bash"
            | "bash"
            | "wsl"
            | "rio"
            | "ghostty"
            | "terminus"
    )
}

pub(crate) fn resolve_paste_strategy(
    default_method: &str,
    options: &PasteDeliveryOptions,
) -> ResolvedPasteStrategy {
    if options.copy_only || options.paste_method_override == AppPasteMethod::CopyOnly {
        return ResolvedPasteStrategy::CopyOnly;
    }
    let restore_clipboard = !options.keep_on_clipboard;
    match options.paste_method_override {
        AppPasteMethod::SendInput => ResolvedPasteStrategy::SendInput,
        AppPasteMethod::Clipboard => {
            if options.paste_with_shift {
                ResolvedPasteStrategy::ClipboardShiftInsert { restore_clipboard }
            } else {
                ResolvedPasteStrategy::ClipboardCtrlV { restore_clipboard }
            }
        }
        AppPasteMethod::TerminalCtrlShiftV => {
            ResolvedPasteStrategy::ClipboardCtrlShiftV { restore_clipboard }
        }
        AppPasteMethod::CopyOnly => ResolvedPasteStrategy::CopyOnly,
        AppPasteMethod::Inherit => {
            if options.paste_with_shift {
                ResolvedPasteStrategy::ClipboardShiftInsert { restore_clipboard }
            } else if options.auto_terminal_ctrl_shift_v
                && is_terminal_process(options.process_name.as_deref())
            {
                ResolvedPasteStrategy::ClipboardCtrlShiftV { restore_clipboard }
            } else if default_method == "clipboard" {
                ResolvedPasteStrategy::ClipboardCtrlV { restore_clipboard }
            } else {
                ResolvedPasteStrategy::SendInput
            }
        }
    }
}

/// Prepends a separator space when inserting new dictated text right after an
/// existing word or sentence ending, or trims redundant leading spaces after whitespace.
pub(crate) fn apply_smart_spacing(text: &str, preceding_char: Option<char>) -> String {
    if text.is_empty() {
        return String::new();
    }
    let Some(prev) = preceding_char else {
        return text.to_string();
    };
    let first = match text.chars().next() {
        Some(c) => c,
        None => return text.to_string(),
    };
    if prev.is_whitespace() {
        return text.trim_start_matches(' ').to_string();
    }
    let prev_needs_space = prev.is_alphanumeric()
        || matches!(
            prev,
            '.' | '!' | '?' | ',' | ';' | ':' | ')' | ']' | '}' | '"' | '\''
        );
    let next_starts_word = first.is_alphanumeric() || matches!(first, '"' | '\'' | '(' | '[' | '#');
    let prev_is_cjk = is_cjk_char(prev);
    let next_is_cjk = is_cjk_char(first);
    if prev_needs_space && next_starts_word && !(prev_is_cjk && next_is_cjk) {
        format!(" {text}")
    } else {
        text.to_string()
    }
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3040..=0x30FF | 0xAC00..=0xD7AF
    )
}

/// A completed dictation keeps the target captured when its recording began.
#[derive(Clone)]
pub(crate) struct PendingPaste {
    pub session_id: u64,
    pub target_window: Option<isize>,
    pub text: String,
    /// Press Enter in the target window after the text is delivered.
    pub press_enter: bool,
    pub options: PasteDeliveryOptions,
}

impl PendingPaste {
    pub fn target_matches(&self, current_window: Option<isize>) -> bool {
        self.target_window.is_some() && self.target_window == current_window
    }
}

#[derive(Default)]
pub(crate) struct PendingPasteQueue {
    pending: Vec<PendingPaste>,
}

impl PendingPasteQueue {
    pub fn push(&mut self, output: PendingPaste) {
        if !self
            .pending
            .iter()
            .any(|item| item.session_id == output.session_id)
        {
            self.pending.push(output);
        }
    }

    pub fn take_if_idle(&mut self, recording: bool) -> Vec<PendingPaste> {
        if recording {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}
