//! Short-lived, local-only learning from the field that received a dictation.
use std::sync::atomic::{AtomicU64, Ordering};

static GENERATION: AtomicU64 = AtomicU64::new(0);
const MAX_TEXT: usize = 16_384;

pub fn cancel() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub window: isize,
    pub field: Vec<i32>,
    pub text: String,
    pub selected: String,
}

pub fn capture() -> Option<Snapshot> {
    capture_field(None)
}

#[cfg(target_os = "windows")]
fn is_same_process(hwnd1: isize, hwnd2: isize) -> bool {
    if hwnd1 == hwnd2 {
        return true;
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut pid1 = 0u32;
    let mut pid2 = 0u32;
    // SAFETY: pid1 and pid2 are valid writable u32 locals, and the window handles are plain values that Windows validates.
    unsafe {
        GetWindowThreadProcessId(hwnd1 as _, &mut pid1);
        GetWindowThreadProcessId(hwnd2 as _, &mut pid2);
    }
    pid1 != 0 && pid1 == pid2
}

fn capture_field(expected: Option<&Snapshot>) -> Option<Snapshot> {
    #[cfg(target_os = "windows")]
    {
        use uiautomation::{
            patterns::{UITextPattern, UIValuePattern},
            types::ControlType,
            UIAutomation,
        };
        use windows_sys::Win32::{
            System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
            UI::WindowsAndMessaging::GetForegroundWindow,
        };
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                // SAFETY: Runs only from the guard created after CoInitializeEx succeeded on this thread, so the call balances that initialization.
                unsafe { CoUninitialize() };
            }
        }
        // SAFETY: GetForegroundWindow takes no arguments and returns a window handle value or null.
        let window = unsafe { GetForegroundWindow() } as isize;
        if window == 0 {
            return None;
        }
        if let Some(s) = expected {
            if s.window != window && !is_same_process(s.window, window) {
                return None;
            }
        }
        // SAFETY: The reserved argument is null as required. A negative result means this thread was not initialized, so no uninitialize is owed.
        if unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) } < 0 {
            return None;
        }
        let _com = ComGuard;
        let automation = UIAutomation::new()
            .or_else(|_| UIAutomation::new_direct())
            .ok()?;
        let focused = automation.get_focused_element().ok()?;
        if focused.is_password().ok().unwrap_or(false)
            || focused.get_process_id().ok() == Some(std::process::id())
        {
            return None;
        }
        let walker = automation.get_control_view_walker().ok()?;
        let mut candidate = focused.clone();
        let mut input = focused.clone();
        let mut found_editor = false;

        let focused_type = focused.get_control_type().ok();
        if matches!(
            focused_type,
            Some(ControlType::Edit | ControlType::Document)
        ) || focused.get_pattern::<UITextPattern>().is_ok()
        {
            found_editor = true;
        }

        if !found_editor {
            for _ in 0..8 {
                if candidate.get_process_id().ok() != focused.get_process_id().ok() {
                    break;
                }
                if candidate.is_password().ok().unwrap_or(false) {
                    return None;
                }
                let c_type = candidate.get_control_type().ok();
                if matches!(c_type, Some(ControlType::Edit | ControlType::Document))
                    || candidate.get_pattern::<UITextPattern>().is_ok()
                {
                    input = candidate;
                    break;
                }
                let Ok(parent) = walker.get_parent(&candidate) else {
                    break;
                };
                candidate = parent;
            }
        }

        let field = input.get_runtime_id().ok().unwrap_or_default();
        if let Some(s) = expected {
            if !s.field.is_empty() && !field.is_empty() {
                let exact = s.field == field;
                let same_parent = s.field.len() == field.len()
                    && s.field.get(1) == field.get(1)
                    && (s.field.len() <= 2
                        || s.field[..s.field.len() - 1] == field[..field.len() - 1]);
                if !exact && !same_parent && window != s.window {
                    return None;
                }
            }
        }

        let text_pattern = input
            .get_pattern::<UITextPattern>()
            .ok()
            .or_else(|| focused.get_pattern::<UITextPattern>().ok());
        let value_pattern = input
            .get_pattern::<UIValuePattern>()
            .ok()
            .or_else(|| focused.get_pattern::<UIValuePattern>().ok());

        let mut text = text_pattern
            .as_ref()
            .and_then(|pattern| pattern.get_document_range().ok())
            .and_then(|range| range.get_text(MAX_TEXT as i32 + 1).ok())
            .filter(|t| !t.trim().is_empty());

        if text.as_ref().is_none_or(|t| t.trim().is_empty()) {
            if let Some(ref vp) = value_pattern {
                if !vp.is_readonly().unwrap_or(true) {
                    if let Ok(v) = vp.get_value() {
                        text = Some(v);
                    }
                }
            }
        }

        let text = text.or_else(|| {
            text_pattern
                .as_ref()
                .and_then(|pattern| pattern.get_document_range().ok())
                .and_then(|range| range.get_text(MAX_TEXT as i32 + 1).ok())
        })?;

        let selected = text_pattern
            .as_ref()
            .and_then(|pattern| pattern.get_selection().ok())
            .filter(|ranges| ranges.len() == 1)
            .and_then(|ranges| ranges[0].get_text(MAX_TEXT as i32 + 1).ok())
            .unwrap_or_default();

        if text.encode_utf16().count() > MAX_TEXT {
            return None;
        }

        Some(Snapshot {
            window,
            field,
            text: normalize_line_endings(&text),
            selected: normalize_line_endings(&selected),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = expected;
        None
    }
}

// UI Automation providers expose multiline inputs as CRLF, CR, or LF.
fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

// Require a uniquely identifiable insertion; never guess which duplicate was pasted.
fn insertion<'a>(
    before: &str,
    after: &'a str,
    pasted: &str,
    selected: &str,
) -> Option<(&'a str, &'a str)> {
    if pasted.is_empty() || pasted.chars().count() > MAX_TEXT {
        return None;
    }
    let mut found = None;
    for (start, _) in after
        .char_indices()
        .filter(|(i, _)| after[*i..].starts_with(pasted))
    {
        let prefix = &after[..start];
        let suffix = &after[start + pasted.len()..];
        if passage(before, prefix, suffix) == Some(selected) {
            if found.is_some() {
                return None;
            }
            found = Some((prefix, suffix));
        }
    }
    found
}

fn passage<'a>(text: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    if text.len() < prefix.len() + suffix.len() {
        return None;
    }
    text.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn tokens(text: &str) -> Vec<&str> {
    let mut start = 0;
    let mut result = Vec::with_capacity((text.len() / 4).max(8));
    let mut word = None;
    for (index, ch) in text.char_indices() {
        let next = ch.is_alphanumeric();
        if word.is_some_and(|previous| previous != next) {
            result.push(&text[start..index]);
            start = index;
        }
        word = Some(next);
    }
    result.push(&text[start..]);
    result
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
    )
}

fn word_correction(from: &str, to: &str) -> Option<(String, String)> {
    if !from.chars().all(char::is_alphanumeric)
        || !to.chars().all(char::is_alphanumeric)
        || !from.chars().any(char::is_alphabetic)
        || !to.chars().any(char::is_alphabetic)
        || !from
            .chars()
            .filter(|c| c.is_numeric())
            .eq(to.chars().filter(|c| c.is_numeric()))
    {
        return None;
    }
    let mut a_buf = ['\0'; 25];
    let mut a_len = 0;
    for ch in from.chars() {
        if a_len >= 24 {
            return None;
        }
        a_buf[a_len] = ch;
        a_len += 1;
    }
    let mut b_buf = ['\0'; 25];
    let mut b_len = 0;
    for ch in to.chars() {
        if b_len >= 24 {
            return None;
        }
        b_buf[b_len] = ch;
        b_len += 1;
    }
    if a_len == 0 || b_len == 0 {
        return None;
    }
    let a = &a_buf[..a_len];
    let b = &b_buf[..b_len];
    let max_len = a_len.max(b_len);

    let mut row = [0usize; 26];
    for (idx, val) in row[..=b.len()].iter_mut().enumerate() {
        *val = idx;
    }
    for (i, &left) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &right) in b.iter().enumerate() {
            let previous = row[j + 1];
            row[j + 1] = (diagonal + usize::from(left != right))
                .min(row[j] + 1)
                .min(previous + 1);
            diagonal = previous;
        }
    }
    let swapped = a.len() == b.len()
        && (0..a.len().saturating_sub(1)).any(|i| {
            a[i] == b[i + 1] && a[i + 1] == b[i] && a[..i] == b[..i] && a[i + 2..] == b[i + 2..]
        });
    let distance = if swapped { 1 } else { row[b.len()] };
    let max_distance = if max_len >= 9 { 3 } else { 2 };
    if distance == 0 || distance > max_distance || distance * 2 > max_len {
        return None;
    }
    Some((from.to_string(), to.to_string()))
}

fn compound_correction(orig_mid: &[&str], upd_mid: &[&str]) -> Option<(String, String)> {
    let valid_words = |slice: &[&str]| {
        slice.iter().enumerate().all(|(idx, tok)| {
            if idx % 2 == 1 {
                *tok == " "
            } else {
                !tok.is_empty()
                    && tok.chars().all(char::is_alphanumeric)
                    && tok.chars().any(char::is_alphabetic)
                    && !tok.chars().any(is_cjk)
            }
        })
    };
    if !matches!((orig_mid.len(), upd_mid.len()), (3, 1) | (1, 3))
        || !valid_words(orig_mid)
        || !valid_words(upd_mid)
    {
        return None;
    }
    let joined_from: String = orig_mid.iter().copied().filter(|t| *t != " ").collect();
    let joined_to: String = upd_mid.iter().copied().filter(|t| *t != " ").collect();
    if joined_from.chars().count() < 4
        || joined_to.chars().count() < 4
        || (!joined_from.eq_ignore_ascii_case(&joined_to)
            && word_correction(&joined_from, &joined_to).is_none())
    {
        return None;
    }
    Some((orig_mid.concat(), upd_mid.concat()))
}

// Single word substitution or 2-word compound split/merge with small edit distance.
// shortcut: arbitrary phrase rewrites are skipped; expand only with labeled examples.
fn correction(before: &str, after: &str) -> Option<(String, String)> {
    let original = tokens(before);
    let updated = tokens(after);
    if original.len() == updated.len() {
        let mut changes = original.iter().zip(&updated).filter(|(a, b)| a != b);
        let (&from, &to) = changes.next()?;
        if changes.next().is_some() {
            return None;
        }
        return word_correction(from, to);
    }
    let prefix = original
        .iter()
        .zip(&updated)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = original[prefix..]
        .iter()
        .rev()
        .zip(updated[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    compound_correction(
        &original[prefix..original.len() - suffix],
        &updated[prefix..updated.len() - suffix],
    )
}

/// Apply only unambiguous, user-confirmed spelling rules, without chaining replacements.
pub fn apply_spelling_rules(
    text: &str,
    profile: &crate::state::user_profile::UserProfile,
) -> String {
    use crate::state::user_profile::{CorrectionPattern, CorrectionSource};
    if !profile.external_correction_learning {
        return text.to_string();
    }
    let rules: Vec<_> = profile
        .correction_patterns
        .iter()
        .filter(|rule| {
            rule.source == CorrectionSource::User
                && correction(&rule.original, &rule.corrected).is_some()
        })
        .collect();
    let unique = |key: &str| -> Option<&str> {
        let mut exact_matches = rules.iter().filter(|rule| rule.original == key);
        if let Some(first) = exact_matches.next() {
            if !exact_matches.any(|rule| rule.corrected != first.corrected) {
                return Some(first.corrected.as_str());
            }
        }
        let mut ci_matches = rules
            .iter()
            .filter(|rule| rule.original.eq_ignore_ascii_case(key));
        let first = ci_matches.next()?;
        (!ci_matches.any(|rule| !rule.corrected.eq_ignore_ascii_case(&first.corrected)))
            .then_some(first.corrected.as_str())
    };
    let cjk_rules: Vec<&CorrectionPattern> = rules
        .iter()
        .copied()
        .filter(|rule| {
            rule.original.chars().all(is_cjk)
                && rule.original.chars().count() >= 2
                && unique(&rule.original).is_some()
        })
        .collect();
    let toks = tokens(text);
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < toks.len() {
        if i + 2 < toks.len() && toks[i + 1] == " " {
            let phrase = format!("{} {}", toks[i], toks[i + 2]);
            if let Some(rep) = unique(&phrase) {
                out.push_str(rep);
                i += 3;
                continue;
            }
        }
        if let Some(rep) = unique(toks[i]) {
            out.push_str(rep);
            i += 1;
            continue;
        }
        if !cjk_rules.is_empty() && toks[i].chars().all(is_cjk) {
            let token = toks[i];
            let mut spans: Vec<(usize, usize, &str)> = Vec::new();
            for rule in &cjk_rules {
                for (start, matched) in token.match_indices(&rule.original) {
                    spans.push((start, start + matched.len(), rule.corrected.as_str()));
                }
            }
            if !spans.is_empty() {
                spans.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
                let mut kept: Vec<(usize, usize, &str)> = Vec::new();
                for span in spans {
                    if !kept.iter().any(|k| span.0 < k.1 && k.0 < span.1) {
                        kept.push(span);
                    }
                }
                kept.sort_by_key(|span| span.0);
                let mut cursor = 0;
                for (start, end, rep) in kept {
                    out.push_str(&token[cursor..start]);
                    out.push_str(rep);
                    cursor = end;
                }
                out.push_str(&token[cursor..]);
                i += 1;
                continue;
            }
        }
        out.push_str(toks[i]);
        i += 1;
    }
    out
}

fn corrections(before: &str, after: &str) -> Option<Vec<(String, String)>> {
    let original = tokens(before);
    let updated = tokens(after);
    if original.len() != updated.len() {
        let prefix = original
            .iter()
            .zip(&updated)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = original[prefix..]
            .iter()
            .rev()
            .zip(updated[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        if let Some(pair) = compound_correction(
            &original[prefix..original.len() - suffix],
            &updated[prefix..updated.len() - suffix],
        ) {
            return Some(vec![pair]);
        }
    }
    if updated.len() < original.len() {
        return corrections_with_deletions(&original, &updated);
    }
    if updated.len() > original.len()
        && original
            .last()
            .is_some_and(|last| last.chars().any(char::is_alphanumeric))
        && updated[original.len()].chars().any(char::is_alphanumeric)
    {
        return None;
    }
    let last_idx = original.len().saturating_sub(1);
    let pairs: Option<Vec<_>> = original
        .iter()
        .enumerate()
        .zip(updated.iter().take(original.len()))
        .filter(|&((i, a), b)| {
            if a == b {
                return false;
            }
            !(i == last_idx
                && updated.len() > original.len()
                && !a.chars().any(char::is_alphanumeric)
                && b.starts_with(*a))
        })
        .map(|((_, a), b)| word_correction(a, b))
        .collect();
    pairs.filter(|pairs| !pairs.is_empty())
}

// Align only deletions and safe spelling substitutions. No insertion/rewrite may be
// disguised as a deletion, and deleted words never become global removal rules.
fn corrections_with_deletions(
    original: &[&str],
    updated: &[&str],
) -> Option<Vec<(String, String)>> {
    // Bound quadratic work even for a large pasted document.
    if original.len() > 512 || updated.len() > 512 {
        return None;
    }
    let mut costs = vec![vec![usize::MAX; updated.len() + 1]; original.len() + 1];
    costs[0][0] = 0;
    for i in 0..original.len() {
        for j in 0..=updated.len() {
            let cost = costs[i][j];
            if cost == usize::MAX {
                continue;
            }
            // Only words and whitespace can be deleted, not sentence boundaries.
            if original[i]
                .chars()
                .all(|c| c.is_alphanumeric() || c.is_whitespace())
            {
                costs[i + 1][j] = costs[i + 1][j].min(cost + 1);
            }
            if j < updated.len() {
                if original[i] == updated[j] {
                    costs[i + 1][j + 1] = costs[i + 1][j + 1].min(cost);
                } else if word_correction(original[i], updated[j]).is_some() {
                    costs[i + 1][j + 1] = costs[i + 1][j + 1].min(cost + 1);
                }
            }
        }
    }
    if costs[original.len()][updated.len()] == usize::MAX {
        return None;
    }
    let (mut i, mut j) = (original.len(), updated.len());
    let mut pairs = Vec::new();
    while i > 0 {
        let deleted = original[i - 1]
            .chars()
            .all(|c| c.is_alphanumeric() || c.is_whitespace())
            && costs[i - 1][j].checked_add(1) == Some(costs[i][j]);
        let pair = if j > 0 {
            word_correction(original[i - 1], updated[j - 1])
        } else {
            None
        };
        let matched = j > 0
            && (original[i - 1] == updated[j - 1] || pair.is_some())
            && costs[i - 1][j - 1].checked_add(usize::from(pair.is_some())) == Some(costs[i][j]);
        // Repeated whitespace is harmless; an ambiguous substitution is not.
        if (!matched && !deleted) || (matched && deleted && pair.is_some()) {
            return None;
        }
        if matched {
            if let Some(pair) = pair {
                pairs.push(pair);
            }
            j -= 1;
        }
        i -= 1;
    }
    pairs.reverse();
    (!pairs.is_empty()).then_some(pairs)
}

fn is_safe_external_pair(orig: &str, corr: &str) -> bool {
    let orig = orig.trim();
    let corr = corr.trim();
    if orig.is_empty() || corr.is_empty() || orig == corr {
        return false;
    }
    let orig_len = orig.chars().count();
    let corr_len = corr.chars().count();
    if orig_len > 30 || corr_len > 30 {
        return false;
    }
    let (longer, shorter) = (orig_len.max(corr_len), orig_len.min(corr_len));
    if shorter >= 2 && longer > shorter * 3 {
        return false;
    }
    if orig_len == 1 && corr_len != 1 {
        return false;
    }
    if orig.chars().all(char::is_alphanumeric)
        && corr.chars().all(char::is_alphanumeric)
        && word_correction(orig, corr).is_some()
    {
        return true;
    }
    let orig_toks = tokens(orig);
    let corr_toks = tokens(corr);
    if compound_correction(&orig_toks, &corr_toks).is_some() {
        return true;
    }
    if orig.chars().any(is_cjk) && corr.chars().any(is_cjk) {
        return true;
    }
    if orig.eq_ignore_ascii_case(corr) {
        return true;
    }
    false
}

fn token_diff_corrections(original: &str, updated: &str) -> Vec<(String, String)> {
    let orig_toks = tokens(original);
    let upd_toks = tokens(updated);

    if orig_toks.len() == upd_toks.len() {
        let mut pairs = Vec::new();
        for (a, b) in orig_toks.iter().zip(upd_toks.iter()) {
            if a != b {
                if let Some(pair) = word_correction(a, b) {
                    pairs.push(pair);
                } else if is_safe_external_pair(a, b) {
                    pairs.push((a.to_string(), b.to_string()));
                }
            }
        }
        if !pairs.is_empty() {
            return pairs;
        }
    }

    let prefix = orig_toks
        .iter()
        .zip(upd_toks.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = orig_toks[prefix..]
        .iter()
        .rev()
        .zip(upd_toks[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    if prefix <= orig_toks.len() - suffix && prefix <= upd_toks.len() - suffix {
        let orig_mid = &orig_toks[prefix..orig_toks.len() - suffix];
        let upd_mid = &upd_toks[prefix..upd_toks.len() - suffix];

        if let Some(pair) = compound_correction(orig_mid, upd_mid) {
            return vec![pair];
        }

        if orig_mid.len() == 1 && upd_mid.len() == 1 {
            let (a, b) = (orig_mid[0], upd_mid[0]);
            if let Some(pair) = word_correction(a, b) {
                return vec![pair];
            } else if is_safe_external_pair(a, b) {
                return vec![(a.to_string(), b.to_string())];
            }
        }
    }

    if let Some(pairs) = corrections(original, updated) {
        return pairs;
    }

    Vec::new()
}

pub fn watch(
    app: tauri::AppHandle,
    target_window: Option<isize>,
    before: Option<Snapshot>,
    pasted: String,
    generation: u64,
) {
    use crate::{
        services::profile_service,
        state::{user_profile::CorrectionSource, AppState},
    };
    use std::time::{Duration, Instant};
    use tauri::Manager;
    use tauri_plugin_clipboard_manager::ClipboardExt;

    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let pasted = normalize_line_endings(&pasted);
        let active = || {
            GENERATION.load(Ordering::SeqCst) == generation
                && state.with_profile(|p| p.external_correction_learning)
                && state.recording.recording.lock().is_none()
        };

        let mut baseline = None;
        let mut prefix = String::new();
        let mut suffix = String::new();
        let mut found_passage = false;
        let mut active_snapshot = before.clone();

        let before_text = before.as_ref().map(|b| b.text.as_str()).unwrap_or("");
        let before_sel = before.as_ref().map(|b| b.selected.as_str()).unwrap_or("");

        // Allow up to 1.8 seconds (12 * 150ms) for async paste (sendInput or clipboard) to be applied.
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(150));
            if !active() {
                return;
            }
            let current = if let Some(ref b) = active_snapshot {
                capture_field(Some(b))
            } else {
                capture_field(None)
            };
            let Some(current) = current else {
                continue;
            };

            if active_snapshot.is_none() {
                if let Some(target) = target_window {
                    if current.window != target && !is_same_process(current.window, target) {
                        continue;
                    }
                }
                active_snapshot = Some(Snapshot {
                    window: current.window,
                    field: current.field.clone(),
                    text: before_text.to_string(),
                    selected: before_sel.to_string(),
                });
            }

            if let Some((p, s)) = insertion(before_text, &current.text, &pasted, before_sel) {
                prefix = p.to_string();
                suffix = s.to_string();
                baseline = Some(current.text);
                found_passage = true;
                break;
            }

            let pasted_trimmed = pasted.trim();
            if !pasted_trimmed.is_empty()
                && current.text.contains(pasted_trimmed)
                && current.text != before_text
            {
                if let Some(idx) = current.text.find(pasted_trimmed) {
                    prefix = current.text[..idx].to_string();
                    suffix = current.text[idx + pasted_trimmed.len()..].to_string();
                    baseline = Some(current.text);
                    found_passage = true;
                    break;
                }
            }
        }

        let baseline = match baseline {
            Some(b) => b,
            None => {
                if let Some(current) = active_snapshot
                    .as_ref()
                    .and_then(|b| capture_field(Some(b)))
                {
                    if current.text != before_text {
                        current.text
                    } else {
                        // If UIA cannot detect text in this field, fallback to clipboard watching below
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
        };

        let deadline = Instant::now() + Duration::from_secs(60);
        let mut learned = std::collections::HashSet::new();

        // If UIA baseline was not captured, watch clipboard as fallback for non-UIA apps
        if baseline.is_empty() {
            let mut last_clip = app.clipboard().read_text().ok();
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(400));
                if !active() {
                    return;
                }
                let Ok(clip) = app.clipboard().read_text() else {
                    continue;
                };
                let clip = normalize_line_endings(&clip);
                if clip.trim().is_empty() || last_clip.as_ref() == Some(&clip) || clip == pasted {
                    continue;
                }
                last_clip = Some(clip.clone());

                let diff_pairs =
                    profile_service::collect_diff_correction_pairs(&[pasted.as_str()], &clip);
                let safe_pairs: Vec<_> = diff_pairs
                    .into_iter()
                    .filter(|(orig, corr)| is_safe_external_pair(orig, corr))
                    .filter(|pair| learned.insert(pair.clone()))
                    .collect();

                if !safe_pairs.is_empty() {
                    profile_service::update_profile_and_schedule(&state, |profile| {
                        if profile.external_correction_learning
                            && GENERATION.load(Ordering::SeqCst) == generation
                        {
                            profile_service::learn_from_structured(
                                profile,
                                &safe_pairs,
                                &[],
                                CorrectionSource::User,
                            );
                        }
                    });
                    log::info!(
                        "Learned external correction from clipboard: {:?}",
                        safe_pairs
                    );
                }
            }
            return;
        }

        let baseline_text = baseline.clone();
        let mut latest = if found_passage {
            pasted.clone()
        } else {
            baseline_text.clone()
        };
        let mut changed_at = Instant::now();
        let mut evaluated = false;

        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(350));
            if !active() {
                return;
            }
            let Some(current) = active_snapshot
                .as_ref()
                .and_then(|b| capture_field(Some(b)))
            else {
                continue;
            };

            let current_target = if found_passage {
                passage(&current.text, &prefix, &suffix)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| current.text.clone())
            } else {
                current.text.clone()
            };

            if current_target != latest {
                latest = current_target;
                changed_at = Instant::now();
                evaluated = false;
                continue;
            }

            let initial_compare = if found_passage {
                &pasted
            } else {
                &baseline_text
            };
            if evaluated
                || &latest == initial_compare
                || changed_at.elapsed() < Duration::from_millis(1500)
            {
                continue;
            }
            evaluated = true;

            let baseline_ref = if found_passage {
                &pasted
            } else {
                &baseline_text
            };
            let mut pairs = token_diff_corrections(baseline_ref, &latest);

            if pairs.is_empty() {
                let diff_pairs = profile_service::collect_diff_correction_pairs(
                    &[baseline_ref.as_str()],
                    &latest,
                );
                pairs = diff_pairs
                    .into_iter()
                    .filter(|(orig, corr)| is_safe_external_pair(orig, corr))
                    .collect();
            }

            let pairs: Vec<_> = pairs
                .into_iter()
                .filter(|pair| learned.insert(pair.clone()))
                .collect();
            if pairs.is_empty() {
                continue;
            }

            profile_service::update_profile_and_schedule(&state, |profile| {
                if profile.external_correction_learning
                    && GENERATION.load(Ordering::SeqCst) == generation
                {
                    profile_service::learn_from_structured(
                        profile,
                        &pairs,
                        &[],
                        CorrectionSource::User,
                    );
                }
            });

            log::info!("Learned external correction: {:?}", pairs);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learning_defaults_on_and_preserves_an_explicit_opt_out() {
        use crate::state::user_profile::UserProfile;
        assert!(UserProfile::default().external_correction_learning);
        let mut legacy = serde_json::to_value(UserProfile::default()).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("external_correction_learning");
        let mut profile: UserProfile = serde_json::from_value(legacy).unwrap();
        assert!(profile.external_correction_learning);
        profile.external_correction_learning = false;
        let loaded: UserProfile =
            serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
        assert!(!loaded.external_correction_learning);
    }

    #[test]
    fn learned_spelling_is_applied_without_ai_or_replacement_chains() {
        use crate::services::profile_service::learn_from_structured;
        use crate::state::user_profile::{CorrectionSource, UserProfile};
        let mut profile = UserProfile {
            external_correction_learning: true,
            ..Default::default()
        };
        let pairs = corrections("teh Speekflow app", "the Speakflow app").unwrap();
        learn_from_structured(&mut profile, &pairs, &[], CorrectionSource::User);
        assert_eq!(
            apply_spelling_rules("teh Speekflow, not Speekflows.", &profile),
            "the Speakflow, not Speekflows."
        );
        learn_from_structured(
            &mut profile,
            &[("Speakflow".into(), "Speakfloo".into())],
            &[],
            CorrectionSource::User,
        );
        assert_eq!(apply_spelling_rules("Speekflow", &profile), "Speakflow");
        learn_from_structured(
            &mut profile,
            &[("Speekflow".into(), "Speekfloo".into())],
            &[],
            CorrectionSource::User,
        );
        assert_eq!(apply_spelling_rules("Speekflow", &profile), "Speekflow");
        profile.external_correction_learning = false;
        assert_eq!(apply_spelling_rules("teh", &profile), "teh");
    }

    #[test]
    fn learned_spelling_matches_case_insensitively_as_fallback() {
        use crate::services::profile_service::learn_from_structured;
        use crate::state::user_profile::{CorrectionSource, UserProfile};
        let mut profile = UserProfile {
            external_correction_learning: true,
            ..Default::default()
        };
        let pairs = corrections("speekflow app", "SpeakFlow app").unwrap();
        learn_from_structured(&mut profile, &pairs, &[], CorrectionSource::User);
        assert_eq!(
            apply_spelling_rules("Speekflow is great, and speekflow is fast.", &profile),
            "SpeakFlow is great, and SpeakFlow is fast."
        );
    }

    #[test]
    fn multiple_edits_must_all_be_small_word_corrections() {
        assert_eq!(corrections("teh recieve", "the receive").unwrap().len(), 2);
        assert_eq!(
            corrections("teh Speekflow", "the Speakflow is great").unwrap(),
            vec![
                ("teh".into(), "the".into()),
                ("Speekflow".into(), "Speakflow".into()),
            ]
        );
        assert_eq!(
            corrections("Use configuraton.", "Use configuration. Next sentence").unwrap(),
            vec![("configuraton".into(), "configuration".into())]
        );
        assert_eq!(corrections("hello", "hello there"), None);
        assert_eq!(corrections("teh hello!", "the hello?"), None);
        assert_eq!(corrections("teh hello", "the goodbye"), None);
        assert_eq!(corrections("hello", "hello"), None);
    }

    #[test]
    fn matches_multiline_pastes_across_windows_text_providers() {
        let before = normalize_line_endings("Before\r\n\r\nafter");
        let pasted = normalize_line_endings("teh\nrecieve");
        for raw in [
            "Before\r\nteh\r\nrecieve\r\nafter",
            "Before\rteh\rrecieve\rafter",
            "Before\nteh\nrecieve\nafter",
        ] {
            let baseline = normalize_line_endings(raw);
            let (prefix, suffix) = insertion(&before, &baseline, &pasted, "").unwrap();
            let edited = normalize_line_endings("Before\r\nthe\r\nreceive\r\nafter");
            let edited = passage(&edited, prefix, suffix).unwrap();
            assert_eq!(corrections(&pasted, edited).unwrap().len(), 2);
        }
    }

    #[test]
    fn identifies_only_the_inserted_passage() {
        assert_eq!(
            insertion("Before  after", "Before teh after", "teh", ""),
            Some(("Before ", " after"))
        );
        assert_eq!(insertion("hi", "hihi", "hi", ""), None);
        assert_eq!(insertion("a", "aaa", "aa", ""), None);
        assert_eq!(
            insertion("selected", "replacement", "replacement", ""),
            None
        );
        assert_eq!(insertion("", "", "", ""), None);
        assert_eq!(
            passage("Before the after", "Before ", " after"),
            Some("the")
        );
        assert_eq!(passage("Different the after", "Before ", " after"), None);
        assert_eq!(passage("short", "long prefix", "suffix"), None);
    }

    #[test]
    fn tracks_pastes_that_replace_selected_text() {
        assert_eq!(
            insertion(
                "Before selected after",
                "Before teh after",
                "teh",
                "selected"
            ),
            Some(("Before ", " after"))
        );
        assert_eq!(insertion("old", "teh", "teh", "old"), Some(("", "")));
        assert_eq!(insertion("private", "teh", "teh", "different"), None);
        assert_eq!(insertion("a", "aaa", "aa", ""), None);
    }

    #[test]
    fn deleting_words_does_not_discard_other_spelling_corrections() {
        assert_eq!(
            corrections("Use teh extra word today.", "Use the word today."),
            Some(vec![("teh".into(), "the".into())])
        );
        assert_eq!(
            corrections(
                "Please please recieve teh file.",
                "Please receive the file."
            ),
            Some(vec![
                ("recieve".into(), "receive".into()),
                ("teh".into(), "the".into())
            ])
        );
        assert_eq!(corrections("Delete this word.", "Delete word."), None);
        assert_eq!(corrections("teh hello there", "the goodbye"), None);
        assert_eq!(corrections("teh. Extra", "the"), None);
        assert_eq!(corrections("", ""), None);
        assert_eq!(
            corrections_with_deletions(&vec!["teh"; 513], &["the"]),
            None
        );
    }

    #[test]
    fn learns_whole_words_not_character_fragments() {
        assert_eq!(
            correction("Use Speekflow today.", "Use Speakflow today."),
            Some(("Speekflow".into(), "Speakflow".into()))
        );
        assert_eq!(correction("teh", "the"), Some(("teh".into(), "the".into())));
        assert_eq!(
            correction("recieve", "receive"),
            Some(("recieve".into(), "receive".into()))
        );
        assert_eq!(
            correction("café", "cafe"),
            Some(("café".into(), "cafe".into()))
        );
    }

    #[test]
    fn rejects_rewrites_additions_deletions_and_punctuation() {
        for (before, after) in [
            ("hello", "hello there"),
            ("hello there", "hello"),
            ("send it", "keep it"),
            ("hello!", "hello?"),
            ("one two", "won too"),
            ("hello", "hello"),
            ("", ""),
            ("word", ""),
            ("a", "b"),
            ("1000", "2000"),
            ("2025", "2026"),
            ("GPT4", "GPT5"),
        ] {
            assert_eq!(correction(before, after), None, "{before} -> {after}");
        }
    }

    #[test]
    fn learns_compound_words_cjk_terms_and_promotes_proper_hotwords() {
        use crate::services::profile_service::learn_from_structured;
        use crate::state::user_profile::{CorrectionSource, UserProfile};
        let mut profile = UserProfile {
            external_correction_learning: true,
            ..Default::default()
        };
        let compound = corrections("Use speak flow today.", "Use SpeakFlow today.").unwrap();
        let cjk = corrections("同机大学", "同济大学").unwrap();
        let longer = corrections("KhentWhisper", "KhentWhispr").unwrap();
        let common = corrections("teh", "the").unwrap();
        for pairs in [&compound, &cjk, &longer, &common] {
            learn_from_structured(&mut profile, pairs, &[], CorrectionSource::User);
        }
        assert_eq!(
            apply_spelling_rules(
                "Use speak flow with KhentWhisper at 今天去同机大学开会。",
                &profile
            ),
            "Use SpeakFlow with KhentWhispr at 今天去同济大学开会。"
        );
        let hot_words: Vec<&str> = profile.hot_words.iter().map(|h| h.text.as_str()).collect();
        assert!(hot_words.contains(&"SpeakFlow"));
        assert!(hot_words.contains(&"同济大学"));
        assert!(hot_words.contains(&"KhentWhispr"));
        assert!(!hot_words.contains(&"the"));
    }

    #[test]
    fn validates_safe_external_pairs() {
        assert!(is_safe_external_pair("teh", "the"));
        assert!(is_safe_external_pair("speekflow", "SpeakFlow"));
        assert!(is_safe_external_pair("funtion", "function"));
        assert!(is_safe_external_pair("speak flow", "SpeakFlow"));
        assert!(is_safe_external_pair("同机大学", "同济大学"));
        assert!(!is_safe_external_pair("hello", "hello there"));
        assert!(!is_safe_external_pair("hello", "goodbye"));
        assert!(!is_safe_external_pair("", "word"));
        assert!(!is_safe_external_pair("same", "same"));
    }

    #[test]
    fn learns_corrections_from_diff_fallback_when_passage_shifts() {
        let baseline = "const teh_var = 1;";
        let updated = "const the_var = 1;";
        let pairs = token_diff_corrections(baseline, updated);
        assert_eq!(pairs, vec![("teh".into(), "the".into())]);

        let baseline_multiline = "function main() {\n    let funtion_name = \"test\";\n}";
        let updated_multiline = "function main() {\n    let function_name = \"test\";\n}";
        let pairs2 = token_diff_corrections(baseline_multiline, updated_multiline);
        assert_eq!(pairs2, vec![("funtion".into(), "function".into())]);
    }

    #[test]
    fn token_diff_rejects_unrelated_line_rewrites() {
        let baseline = "const a = 1;";
        let updated = "const completely_different_identifier = 42;";
        let pairs = token_diff_corrections(baseline, updated);
        assert!(pairs.is_empty());
    }
}
