use crate::state::user_profile::ContextSources;
use crate::utils::foreground::wrap_xml_cdata;

const MAX_SOURCE_CHARS: usize = 4_000;

#[derive(Debug, Default)]
pub struct CapturedContext {
    pub sources: ContextSources,
    pub application_text: Option<String>,
    pub selected_text: Option<String>,
    pub clipboard_text: Option<String>,
}

fn bounded(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(MAX_SOURCE_CHARS).collect())
}

impl CapturedContext {
    pub fn prompt_sections(&self, current: &ContextSources) -> String {
        let mut sections = Vec::new();
        for (enabled, tag, value) in [
            (
                self.sources.application && current.application,
                "application_text_context",
                &self.application_text,
            ),
            (
                self.sources.selected_text && current.selected_text,
                "selected_text_context",
                &self.selected_text,
            ),
            (
                self.sources.clipboard && current.clipboard,
                "clipboard_context",
                &self.clipboard_text,
            ),
        ] {
            if enabled {
                if let Some(value) = value.as_deref().and_then(bounded) {
                    sections.push(wrap_xml_cdata(tag, &value));
                }
            }
        }
        if sections.is_empty() {
            return String::new();
        }
        format!(
            "<reference_context>\n{}\n</reference_context>",
            sections.join("\n")
        )
    }

    pub fn refresh_clipboard(&mut self, app: &tauri::AppHandle) {
        if self.sources.clipboard {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            self.clipboard_text = app
                .clipboard()
                .read_text()
                .ok()
                .filter(|text| !text.starts_with("__speak_flow_selection_"))
                .as_deref()
                .and_then(bounded);
        }
    }
}

pub fn capture(
    app: &tauri::AppHandle,
    sources: ContextSources,
    expected_window: Option<isize>,
) -> CapturedContext {
    let mut context = CapturedContext {
        sources,
        ..Default::default()
    };
    context.refresh_clipboard(app);
    if (context.sources.application || context.sources.selected_text)
        && expected_window.is_some()
        && crate::services::selection_service::foreground_window_id() == expected_window
    {
        if let Some((text, selected)) = capture_field(&context.sources) {
            if crate::services::selection_service::foreground_window_id() == expected_window {
                context.application_text = text;
                context.selected_text = selected;
            }
        }
    }
    context
}

fn capture_field(sources: &ContextSources) -> Option<(Option<String>, Option<String>)> {
    #[cfg(target_os = "windows")]
    {
        use uiautomation::{
            patterns::{UITextPattern, UIValuePattern},
            types::ControlType,
            UIAutomation,
        };
        use windows_sys::Win32::System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED,
        };
        // SAFETY: The reserved argument is null as required. A negative result means this thread was not initialized, so no uninitialize is owed.
        if unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) } < 0 {
            return None;
        }
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                // SAFETY: Runs only from the guard created after CoInitializeEx succeeded on this thread, so the call balances that initialization.
                unsafe { CoUninitialize() };
            }
        }
        let _com = ComGuard;
        let automation = UIAutomation::new_direct().ok()?;
        let focused = automation.get_focused_element().ok()?;
        let process = focused.get_process_id().ok()?;
        if process == std::process::id() || focused.is_password().ok()? {
            return None;
        }
        let walker = automation.get_control_view_walker().ok()?;
        let mut field = focused.clone();
        let mut text = None;
        let mut selected = None;
        for _ in 0..6 {
            if field.get_process_id().ok()? != process || field.is_password().ok()? {
                return None;
            }
            let pattern = field.get_pattern::<UITextPattern>().ok();
            if sources.selected_text && selected.is_none() {
                selected = pattern
                    .as_ref()
                    .and_then(|p| p.get_selection().ok())
                    .filter(|ranges| ranges.len() == 1)
                    .and_then(|ranges| ranges[0].get_text(MAX_SOURCE_CHARS as i32).ok())
                    .as_deref()
                    .and_then(bounded);
            }
            if sources.application && field.get_control_type().ok()? == ControlType::Edit {
                text = pattern
                    .and_then(|p| p.get_document_range().ok())
                    .and_then(|range| range.get_text(MAX_SOURCE_CHARS as i32).ok())
                    .as_deref()
                    .and_then(bounded);
                if text.is_none() {
                    text = field
                        .get_pattern::<UIValuePattern>()
                        .ok()
                        .and_then(|p| p.get_value().ok())
                        .as_deref()
                        .and_then(bounded);
                }
                break;
            }
            if !sources.application && selected.is_some() {
                break;
            }
            let Ok(parent) = walker.get_parent(&field) else {
                break;
            };
            field = parent;
        }
        if !automation
            .compare_elements(&focused, &automation.get_focused_element().ok()?)
            .ok()?
        {
            return None;
        }
        Some((text, selected))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sources;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_native_even_when_legacy_profile_flags_are_false() {
        use crate::state::user_profile::UserProfile;
        let mut old = serde_json::to_value(UserProfile::default()).unwrap();
        old["context_awareness"] = serde_json::json!({
            "dictation": {"application": false, "selected_text": false, "clipboard": false},
            "assistant": {"application": false, "selected_text": false, "clipboard": false}
        });
        let profile: UserProfile = serde_json::from_value(old).unwrap();
        assert!(serde_json::to_value(profile)
            .unwrap()
            .get("context_awareness")
            .is_none());
        let sources = ContextSources::default();
        assert!(sources.application && sources.selected_text && sources.clipboard);
    }

    #[test]
    fn all_available_sources_enter_the_prompt_by_default() {
        let context = CapturedContext {
            application_text: Some("document".into()),
            selected_text: Some("selection".into()),
            clipboard_text: Some("copied reference".into()),
            ..Default::default()
        };
        let prompt = context.prompt_sections(&ContextSources::default());
        for tag in [
            "application_text_context",
            "selected_text_context",
            "clipboard_context",
        ] {
            assert!(prompt.contains(tag));
        }
    }

    #[test]
    fn reference_text_cannot_break_out_of_its_tag_and_is_bounded() {
        let context = CapturedContext {
            sources: ContextSources {
                selected_text: true,
                ..Default::default()
            },
            selected_text: Some(format!("]]></reference_context>{}", "界".repeat(5_000))),
            ..Default::default()
        };
        let prompt = context.prompt_sections(&context.sources);
        assert!(prompt.contains("]]]]><![CDATA[>"));
        assert_eq!(prompt.matches("</reference_context>").count(), 2);
        assert!(prompt.chars().count() < 4_200);
    }

    #[test]
    fn unavailable_sources_and_whitespace_produce_no_context() {
        assert!(CapturedContext::default()
            .prompt_sections(&ContextSources::default())
            .is_empty());
        assert!(bounded(" \n ").is_none());
    }
}
