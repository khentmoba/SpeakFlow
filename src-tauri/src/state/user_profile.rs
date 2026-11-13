use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Hotword source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HotWordSource {
    /// Added manually by the user
    User,
    /// Learned automatically from corrections
    Learned,
}

/// Correction source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CorrectionSource {
    /// Learned automatically by AI polish
    #[default]
    Ai,
    /// Manual user correction
    User,
}

/// Hotword entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotWord {
    pub text: String,
    /// Weight 1-5; higher means higher priority
    pub weight: u8,
    pub source: HotWordSource,
    pub use_count: u32,
    /// Unix timestamp (seconds)
    pub last_used: u64,
}

/// Correction pattern record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionPattern {
    /// Raw ASR output fragment
    pub original: String,
    /// Corrected fragment
    pub corrected: String,
    /// Number of occurrences
    pub count: u32,
    /// Timestamp of the most recent occurrence
    pub last_seen: u64,
    /// Correction source
    #[serde(default)]
    pub source: CorrectionSource,
    /// Typed in the Correction Rules dialog rather than learned from edits.
    #[serde(default)]
    pub manual: bool,
}

/// Word-frequency entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabEntry {
    pub count: u32,
    pub last_seen: u64,
}

/// Jev evaluation provider used by the optional ordinary-dictation gate.
///
/// The serde names are part of the persisted profile contract. Keep them
/// lowercase so existing profiles and the frontend use the same values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum JevProvider {
    #[default]
    TypeSafe,
    OpenRouter,
    Vercel,
    Liquid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JevConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider: JevProvider,
    #[serde(default)]
    pub screen_routing: bool,
    #[serde(default)]
    pub correction_review: bool,
    #[serde(default)]
    pub search_routing: bool,
    #[serde(default)]
    pub polish_audit: bool,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: JevProvider::TypeSafe,
            screen_routing: false,
            correction_review: false,
            search_routing: false,
            polish_audit: false,
        }
    }
}

/// User profile
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct R2T2Config {
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextSources {
    pub application: bool,
    pub selected_text: bool,
    pub clipboard: bool,
}

impl Default for ContextSources {
    fn default() -> Self {
        Self {
            application: true,
            selected_text: true,
            clipboard: true,
        }
    }
}

impl ContextSources {
    pub fn enabled(&self) -> bool {
        self.application || self.selected_text || self.clipboard
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// Local learning from edits in the field receiving automatic dictation; enabled by default.
    #[serde(default = "default_external_correction_learning")]
    pub external_correction_learning: bool,
    /// Hide the main window on launch at login, leaving only the system tray entry.
    #[serde(default)]
    pub autostart_minimized: bool,
    #[serde(default)]
    pub r2t2: R2T2Config,
    pub hot_words: Vec<HotWord>,
    pub correction_patterns: Vec<CorrectionPattern>,
    pub vocab_frequency: HashMap<String, VocabEntry>,
    pub total_transcriptions: u64,
    /// Last update time (Unix timestamp)
    pub last_updated: u64,
    /// LLM backend configuration
    pub llm_provider: LlmProviderConfig,
    /// Optional Jev gate configuration; provider credentials remain in the system keyring.
    #[serde(default)]
    pub jev: JevConfig,
    /// Translation target language (None = translation off, non-empty = on and translate into that language)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_target: Option<String>,
    /// Independent hotkey for translation mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_hotkey: Option<String>,
    /// User-defined polish instructions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    /// How strongly AI polish categorizes, reorders and summarizes dictated content
    #[serde(default)]
    pub polish_structure_level: PolishStructureLevel,
    /// Independent hotkey for assistant mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_hotkey: Option<String>,
    /// Extra system prompt for assistant mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_system_prompt: Option<String>,
    /// Whether assistant mode attaches the current screen screenshot as context
    #[serde(default)]
    pub assistant_screen_context_enabled: bool,
    /// Whether AI polish attaches the current screen screenshot as context
    #[serde(default)]
    pub ai_polish_screen_context_enabled: bool,
    /// System-level selection assistant configuration
    #[serde(default)]
    pub selection_assistant: SelectionAssistantConfig,
    /// Blocklist of hotwords that must not be learned back automatically after the user deletes them manually
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_hot_words: Vec<String>,
    /// Web search configuration (assistant mode only)
    #[serde(default)]
    pub web_search: WebSearchConfig,
    /// Whether to periodically run the LLM review of correction rules
    #[serde(default)]
    pub correction_validation_enabled: bool,
    /// Timestamp of the last LLM review (Unix seconds)
    #[serde(default)]
    pub last_correction_validation: u64,
    /// Retention policy for local transcription history
    #[serde(default)]
    pub history_settings: HistorySettings,
    /// Dictation override rules matched by foreground app; order is priority and the first match wins
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_profile_rules: Vec<AppProfileRule>,
    /// Audio ducking, Bluetooth guard, pre-roll, and adaptive release tail settings.
    #[serde(default)]
    pub audio_behavior: AudioBehaviorConfig,
    /// Clipboard, terminal paste, and copy-only output behavior.
    #[serde(default)]
    pub output_behavior: OutputBehaviorConfig,
    /// Secondary dictation, terminal paste, typed-text, meeting, and passthrough shortcuts.
    #[serde(default)]
    pub shortcuts_behavior: ShortcutsBehaviorConfig,
    /// Experimental and power-user feature toggles surfaced under Settings > Labs.
    #[serde(default)]
    pub labs: LabsConfig,
    /// Switch local ASR engine by active Windows keyboard layout (BCP-47 / primary tag).
    #[serde(default)]
    pub keyboard_model_switching: bool,
    /// Mapping from keyboard layout language tag (e.g. "en", "zh", "ja") to ASR engine key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyboard_model_rules: Vec<KeyboardModelRule>,
    /// Dual-track meeting capture, call detection, retention, and summary settings.
    #[serde(default)]
    pub meetings: MeetingsConfig,
    /// Ordered global text transformation IDs (built-in or from speakflow.config.ts).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub global_transformations: Vec<String>,
}

fn default_external_correction_learning() -> bool {
    true
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            external_correction_learning: default_external_correction_learning(),
            autostart_minimized: Default::default(),
            r2t2: Default::default(),
            hot_words: Default::default(),
            correction_patterns: Default::default(),
            vocab_frequency: Default::default(),
            total_transcriptions: Default::default(),
            last_updated: Default::default(),
            llm_provider: Default::default(),
            jev: Default::default(),
            translation_target: Default::default(),
            translation_hotkey: Default::default(),
            custom_prompt: Default::default(),
            polish_structure_level: Default::default(),
            assistant_hotkey: Default::default(),
            assistant_system_prompt: Default::default(),
            assistant_screen_context_enabled: Default::default(),
            ai_polish_screen_context_enabled: Default::default(),
            selection_assistant: Default::default(),
            blocked_hot_words: Default::default(),
            web_search: Default::default(),
            correction_validation_enabled: Default::default(),
            last_correction_validation: Default::default(),
            history_settings: Default::default(),
            app_profile_rules: Default::default(),
            audio_behavior: Default::default(),
            output_behavior: Default::default(),
            shortcuts_behavior: Default::default(),
            labs: Default::default(),
            keyboard_model_switching: false,
            keyboard_model_rules: Default::default(),
            meetings: Default::default(),
            global_transformations: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioDuckingMode {
    #[default]
    Off,
    Duck,
    Mute,
    PauseMedia,
}

fn default_duck_volume() -> f32 {
    0.2
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioBehaviorConfig {
    #[serde(default)]
    pub ducking_mode: AudioDuckingMode,
    #[serde(default = "default_duck_volume")]
    pub duck_volume: f32,
    #[serde(default)]
    pub speakers_only: bool,
    #[serde(default = "default_true")]
    pub avoid_bluetooth_microphone: bool,
    #[serde(default)]
    pub release_mic_while_idle: bool,
    #[serde(default = "default_true")]
    pub pre_roll_enabled: bool,
    #[serde(default = "default_true")]
    pub adaptive_release_tail: bool,
}

impl Default for AudioBehaviorConfig {
    fn default() -> Self {
        Self {
            ducking_mode: AudioDuckingMode::Off,
            duck_volume: default_duck_volume(),
            speakers_only: false,
            avoid_bluetooth_microphone: true,
            release_mic_while_idle: false,
            pre_roll_enabled: true,
            adaptive_release_tail: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OutputBehaviorConfig {
    #[serde(default)]
    pub paste_with_shift: bool,
    #[serde(default)]
    pub auto_terminal_ctrl_shift_v: bool,
    #[serde(default)]
    pub keep_on_clipboard: bool,
    #[serde(default)]
    pub copy_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutsBehaviorConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_dictation_hotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_paste_hotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed_text_hotkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meeting_hotkey: Option<String>,
    #[serde(default)]
    pub share_dictation_shortcut: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LabsConfig {
    #[serde(default = "default_true")]
    pub failure_notices: bool,
    #[serde(default)]
    pub return_to_app: bool,
    #[serde(default)]
    pub smart_spacing: bool,
    #[serde(default)]
    pub spoken_formatting: bool,
    #[serde(default)]
    pub spoken_symbols: bool,
    #[serde(default)]
    pub skip_pauses: bool,
    #[serde(default)]
    pub tap_or_hold: bool,
    #[serde(default)]
    pub standalone_special_keys: bool,
    #[serde(default)]
    pub output_fixes: bool,
    #[serde(default)]
    pub api_processing: bool,
    #[serde(default)]
    pub meeting_finished_event: bool,
    #[serde(default = "default_true")]
    pub meeting_notes_vocabulary: bool,
    #[serde(default)]
    pub expire_meeting_audio: bool,
}

impl Default for LabsConfig {
    fn default() -> Self {
        Self {
            failure_notices: true,
            return_to_app: false,
            smart_spacing: false,
            spoken_formatting: false,
            spoken_symbols: false,
            skip_pauses: false,
            tap_or_hold: false,
            standalone_special_keys: false,
            output_fixes: false,
            api_processing: false,
            meeting_finished_event: false,
            meeting_notes_vocabulary: true,
            expire_meeting_audio: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardModelRule {
    pub layout_tag: String,
    pub engine: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MeetingAppPolicy {
    #[default]
    Ask,
    Always,
    Never,
}

fn default_meeting_keep_audio_days() -> u32 {
    30
}

fn default_meeting_font_size() -> u8 {
    13
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsConfig {
    #[serde(default = "default_true")]
    pub call_detection_enabled: bool,
    #[serde(default)]
    pub more_call_apps: bool,
    #[serde(default)]
    pub app_policies: HashMap<String, MeetingAppPolicy>,
    #[serde(default = "default_true")]
    pub auto_summarize: bool,
    #[serde(default)]
    pub save_markdown_transcripts: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown_output_dir: Option<String>,
    #[serde(default = "default_meeting_keep_audio_days")]
    pub keep_audio_days: u32,
    #[serde(default = "default_meeting_font_size")]
    pub transcript_font_size: u8,
}

impl Default for MeetingsConfig {
    fn default() -> Self {
        Self {
            call_detection_enabled: true,
            more_call_apps: false,
            app_policies: HashMap::new(),
            auto_summarize: true,
            save_markdown_transcripts: false,
            markdown_output_dir: None,
            keep_audio_days: default_meeting_keep_audio_days(),
            transcript_font_size: default_meeting_font_size(),
        }
    }
}

fn default_history_enabled() -> bool {
    false
}

fn default_history_retention_days() -> u32 {
    90
}

/// Local history saves text only by default; audio must be turned on by the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySettings {
    #[serde(default = "default_history_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub save_audio: bool,
    /// 0 means keep forever; otherwise text and audio are cleaned up by day.
    #[serde(default = "default_history_retention_days")]
    pub retention_days: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            save_audio: false,
            retention_days: default_history_retention_days(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppRuleOverride {
    #[default]
    Inherit,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTranslationOverride {
    #[default]
    Inherit,
    Disabled,
    Target,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppPasteMethod {
    #[default]
    Inherit,
    SendInput,
    Clipboard,
    TerminalCtrlShiftV,
    CopyOnly,
}

fn default_app_rule_enabled() -> bool {
    true
}

/// Dictation override rule for one Windows process (optionally narrowed by window title or browser domain).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProfileRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_app_rule_enabled")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title_contains: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_domain: Option<String>,
    #[serde(default)]
    pub ai_polish: AppRuleOverride,
    #[serde(default)]
    pub translation: AppTranslationOverride,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation_target: Option<String>,
    #[serde(default)]
    pub screen_context: AppRuleOverride,
    #[serde(default)]
    pub history: AppRuleOverride,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone_mode: Option<String>,
    #[serde(default)]
    pub paste_method: AppPasteMethod,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vocabulary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transformations: Vec<String>,
}

impl Default for AppProfileRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            process_name: String::new(),
            window_title_contains: None,
            browser_domain: None,
            ai_polish: AppRuleOverride::Inherit,
            translation: AppTranslationOverride::Inherit,
            translation_target: None,
            screen_context: AppRuleOverride::Inherit,
            history: AppRuleOverride::Inherit,
            custom_prompt: None,
            tone_mode: None,
            paste_method: AppPasteMethod::Inherit,
            vocabulary: Vec::new(),
            transformations: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedAppProfile {
    pub rule_id: Option<String>,
    pub rule_name: Option<String>,
    pub ai_polish_enabled: Option<bool>,
    pub translation_target: Option<Option<String>>,
    pub screen_context_enabled: Option<bool>,
    pub history_enabled: Option<bool>,
    pub custom_prompt: Option<String>,
    pub tone_mode: Option<String>,
    pub paste_method: AppPasteMethod,
    pub vocabulary: Vec<String>,
    pub transformations: Vec<String>,
}

fn normalized_process_name(value: &str) -> String {
    let normalized = value.trim().to_lowercase();
    normalized
        .strip_suffix(".exe")
        .unwrap_or(&normalized)
        .to_string()
}

impl AppProfileRule {
    pub fn matches(&self, process_name: &str, window_title: &str) -> bool {
        if !self.enabled || self.process_name.trim().is_empty() {
            return false;
        }
        if normalized_process_name(&self.process_name) != normalized_process_name(process_name) {
            return false;
        }
        match self
            .window_title_contains
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(needle) => window_title.to_lowercase().contains(&needle.to_lowercase()),
            None => true,
        }
    }

    fn resolve(&self) -> ResolvedAppProfile {
        let bool_override = |value: AppRuleOverride| match value {
            AppRuleOverride::Inherit => None,
            AppRuleOverride::Enabled => Some(true),
            AppRuleOverride::Disabled => Some(false),
        };
        let translation_target = match self.translation {
            AppTranslationOverride::Inherit => None,
            AppTranslationOverride::Disabled => Some(None),
            AppTranslationOverride::Target => self
                .translation_target
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| Some(value.to_string())),
        };
        ResolvedAppProfile {
            rule_id: Some(self.id.clone()),
            rule_name: Some(self.name.clone()),
            ai_polish_enabled: bool_override(self.ai_polish),
            translation_target,
            screen_context_enabled: bool_override(self.screen_context),
            history_enabled: bool_override(self.history),
            custom_prompt: self
                .custom_prompt
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            tone_mode: self
                .tone_mode
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            paste_method: self.paste_method,
            vocabulary: self
                .vocabulary
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            transformations: self
                .transformations
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
        }
    }
}

/// Behavior configuration for the system-level selection assistant. Off by default so an upgrade does not unexpectedly listen to the global mouse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionAssistantConfig {
    pub enabled: bool,
    #[serde(default)]
    pub auto_screenshot: bool,
    #[serde(default)]
    pub screenshot_routing: bool,
    #[serde(default)]
    pub custom_prompt: Option<String>,
    #[serde(default = "default_selection_translation_target")]
    pub translation_target: String,
    #[serde(default = "default_selection_excluded_apps")]
    pub excluded_apps: Vec<String>,
}

fn default_selection_translation_target() -> String {
    "English".to_string()
}

fn default_selection_excluded_apps() -> Vec<String> {
    ["speak-flow.exe", "snipaste.exe", "pixpin.exe", "sharex.exe"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

impl Default for SelectionAssistantConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_screenshot: false,
            screenshot_routing: false,
            custom_prompt: None,
            translation_target: default_selection_translation_target(),
            excluded_apps: default_selection_excluded_apps(),
        }
    }
}

/// Web search method
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchProvider {
    /// Use the model vendor's built-in search capability (OpenAI web_search / Anthropic web_search)
    #[default]
    ModelNative,
    /// Bing public web search (no API key).
    Bing,
    /// Exa MCP (rate-limited free tier; optional API key).
    Exa,
    /// Tavily Search API (requires an API key)
    Tavily,
    /// Gemini Grounding with Google Search (requires a Google AI API key)
    Google,
}

/// Web search configuration (effective in assistant mode only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    pub enabled: bool,
    #[serde(default)]
    pub provider: WebSearchProvider,
    /// Number of search results (1-10, default 5)
    #[serde(default = "default_max_results")]
    pub max_results: u8,
}

fn default_max_results() -> u8 {
    5
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: WebSearchProvider::default(),
            max_results: default_max_results(),
        }
    }
}

/// API protocol format
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    #[default]
    OpenaiCompat,
    Anthropic,
}

/// OpenAI auth method selection (effective only when the active / assistant provider is openai)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenaiAuthMode {
    /// Use only the API key the user entered and ignore any signed-in OAuth session
    ApiKey,
    /// Use only the OAuth session and ignore a hand-entered API key
    Oauth,
}

/// xAI auth method selection (effective only when the active / assistant provider is xai)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum XaiAuthMode {
    ApiKey,
    Oauth,
}

/// Anthropic auth method selection (effective only when the provider is claude)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeAuthMode {
    ApiKey,
    Oauth,
}

/// Structured rewrite strength of AI polish. Independent of the model's reasoning depth.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PolishStructureLevel {
    /// Only correct and punctuate; do not reorder content
    #[default]
    Off,
    /// Only handle explicit lists, steps and natural paragraph breaks
    Light,
    /// Proactively group related points so long dictation is easier to scan
    Balanced,
    /// Allow summarizing and reordering while faithfully preserving key facts
    Strong,
}

/// Reasoning/thinking mode (cross-vendor abstraction layer)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmReasoningMode {
    /// Send no reasoning parameters and follow the vendor's default behavior
    #[default]
    ProviderDefault,
    /// Turn thinking off or keep it as low as possible
    Off,
    /// Lighter
    Light,
    /// Standard
    Balanced,
    /// Deeper
    Deep,
}

/// User-defined LLM provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_format: ApiFormat,
}

/// LLM backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    /// Currently used backend: a preset key or a custom provider id
    pub active: String,
    /// Legacy field, kept for migration compatibility
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_model: Option<String>,
    /// Cross-vendor thinking-mode abstraction layer
    #[serde(default)]
    pub reasoning_mode: LlmReasoningMode,
    /// Independent thinking mode for the AI polish pipeline; falls back to the legacy reasoning_mode when empty
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polish_reasoning_mode: Option<LlmReasoningMode>,
    /// Independent thinking mode for the AI assistant pipeline; falls back to the legacy reasoning_mode when empty
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_reasoning_mode: Option<LlmReasoningMode>,
    /// Whether the AI assistant uses its own model, separate from polish
    #[serde(default)]
    pub assistant_use_separate_model: bool,
    /// Independent AI assistant model; effective only when assistant_use_separate_model = true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_model: Option<String>,
    /// Independent AI assistant provider; effective only when assistant_use_separate_model = true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_provider: Option<String>,
    /// Selection assistant thinking mode; falls back to the polish thinking mode when empty
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_reasoning_mode: Option<LlmReasoningMode>,
    /// Whether the selection assistant uses its own model, separate from polish
    #[serde(default)]
    pub selection_use_separate_model: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_provider: Option<String>,
    /// User-defined provider list
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_providers: Vec<CustomProvider>,
    /// Whether correction review uses its own provider/model
    #[serde(default)]
    pub validation_use_separate_model: bool,
    /// Independent provider for correction review
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_provider: Option<String>,
    /// Independent model for correction review
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_model: Option<String>,
    /// When the current model does not support image input, whether to use a separate vision model to read the screen
    #[serde(default)]
    pub screen_vision_enabled: bool,
    /// Screen vision model provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_vision_provider: Option<String>,
    /// Screen vision model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_vision_model: Option<String>,
    /// OpenAI auth method: None = the user has not chosen explicitly (the resolver infers a smart default from OAuth sign-in state);
    /// Some(ApiKey) / Some(Oauth) = stored after the user picked one explicitly on the settings page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_auth_mode: Option<OpenaiAuthMode>,
    /// ChatGPT OAuth fast mode: injects service_tier="priority" to use the priority processing lane.
    /// Applies only to signed-in ChatGPT OAuth sessions; API key requests never carry this flag.
    /// Called "fast mode" on the user side; the wire value is "priority" (matching the official Codex CLI).
    #[serde(default)]
    pub openai_fast_mode: bool,
    /// xAI auth method: None = the user has not chosen explicitly (the resolver infers a smart default from Grok Build sign-in state);
    /// Some(ApiKey) / Some(Oauth) = stored after the user picked one explicitly on the settings page
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xai_auth_mode: Option<XaiAuthMode>,
    /// Anthropic auth method: None = infer from Claude sign-in state; Some(..) = the user chose explicitly
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_auth_mode: Option<ClaudeAuthMode>,
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self {
            active: "cerebras".to_string(),
            custom_base_url: None,
            custom_model: None,
            reasoning_mode: LlmReasoningMode::ProviderDefault,
            polish_reasoning_mode: None,
            assistant_reasoning_mode: None,
            assistant_use_separate_model: false,
            assistant_model: None,
            assistant_provider: None,
            selection_reasoning_mode: None,
            selection_use_separate_model: false,
            selection_model: None,
            selection_provider: None,
            custom_providers: Vec::new(),
            validation_use_separate_model: false,
            validation_provider: None,
            validation_model: None,
            screen_vision_enabled: false,
            screen_vision_provider: None,
            screen_vision_model: None,
            openai_auth_mode: None,
            openai_fast_mode: false,
            xai_auth_mode: None,
            claude_auth_mode: None,
        }
    }
}

impl LlmProviderConfig {
    pub(crate) fn is_builtin_provider(provider: &str) -> bool {
        matches!(
            provider,
            "cerebras"
                | "openai"
                | "xai"
                | "deepseek"
                | "siliconflow"
                | "openrouter"
                | "claude"
                | "magpie"
                | "custom"
        )
    }

    pub fn resolve_active_provider(&self) -> String {
        if Self::is_builtin_provider(&self.active)
            || self.custom_providers.iter().any(|p| p.id == self.active)
        {
            return self.active.clone();
        }

        self.custom_providers
            .last()
            .map(|provider| provider.id.clone())
            .unwrap_or_else(|| "cerebras".to_string())
    }

    pub fn fallback_provider_after_removal(&self, removed_id: &str) -> String {
        if self.active != removed_id {
            return self.resolve_active_provider();
        }

        if let Some(index) = self
            .custom_providers
            .iter()
            .position(|p| p.id == removed_id)
        {
            if index > 0 {
                return self.custom_providers[index - 1].id.clone();
            }

            if let Some(next) = self
                .custom_providers
                .iter()
                .enumerate()
                .rev()
                .find(|(idx, _)| *idx != index)
                .map(|(_, provider)| provider.id.clone())
            {
                return next;
            }
        }

        "cerebras".to_string()
    }

    pub fn polish_reasoning_mode(&self) -> LlmReasoningMode {
        self.polish_reasoning_mode.unwrap_or(self.reasoning_mode)
    }

    /// Sets only the polish effort. The legacy `reasoning_mode` is left alone so
    /// the assistant mode that falls back to it does not move, and the saved
    /// selection mode is kept as it is. Selection still follows the polish
    /// effort when no separate selection model is configured, which is how
    /// `selection_reasoning_mode` already resolves.
    pub fn set_polish_reasoning_mode(&mut self, mode: LlmReasoningMode) {
        self.polish_reasoning_mode = Some(mode);
    }

    pub fn assistant_reasoning_mode(&self) -> LlmReasoningMode {
        self.assistant_reasoning_mode.unwrap_or(self.reasoning_mode)
    }

    pub fn assistant_model(&self) -> Option<&str> {
        if !self.assistant_use_separate_model {
            return None;
        }
        self.assistant_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn selection_reasoning_mode(&self) -> LlmReasoningMode {
        if self.has_valid_separate_selection_model() {
            self.selection_reasoning_mode
                .unwrap_or_else(|| self.polish_reasoning_mode())
        } else {
            self.polish_reasoning_mode()
        }
    }

    pub fn selection_model(&self) -> Option<&str> {
        if !self.has_valid_separate_selection_model() {
            return None;
        }
        self.selection_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn resolve_selection_provider(&self) -> String {
        if self.has_valid_separate_selection_model() {
            return self
                .selection_provider
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
        }
        self.resolve_active_provider()
    }

    fn has_valid_separate_selection_model(&self) -> bool {
        if !self.selection_use_separate_model {
            return false;
        }
        let provider = self
            .selection_provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let model_is_set = self
            .selection_model
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        provider.is_some_and(|provider| {
            Self::is_builtin_provider(provider)
                || self.custom_providers.iter().any(|item| item.id == provider)
        }) && model_is_set
    }

    pub fn validation_model(&self) -> Option<&str> {
        if !self.validation_use_separate_model {
            return None;
        }
        self.validation_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn resolve_validation_provider(&self) -> String {
        if self.validation_use_separate_model {
            if let Some(ref p) = self.validation_provider {
                if Self::is_builtin_provider(p)
                    || self.custom_providers.iter().any(|cp| cp.id == *p)
                {
                    return p.clone();
                }
            }
        }
        self.resolve_active_provider()
    }

    pub fn screen_vision_model(&self) -> Option<&str> {
        if !self.has_valid_screen_vision_model() {
            return None;
        }
        self.screen_vision_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn resolve_screen_vision_provider(&self) -> Option<String> {
        if !self.has_valid_screen_vision_model() {
            return None;
        }
        self.screen_vision_provider
            .as_deref()
            .map(str::trim)
            .map(str::to_string)
    }

    pub fn has_valid_screen_vision_model(&self) -> bool {
        if !self.screen_vision_enabled {
            return false;
        }
        let provider = self
            .screen_vision_provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let model_is_set = self
            .screen_vision_model
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        provider.is_some_and(|provider| {
            Self::is_builtin_provider(provider)
                || self.custom_providers.iter().any(|item| item.id == provider)
        }) && model_is_set
    }

    /// Resolve the provider the assistant actually uses (independent config > active polish)
    pub fn resolve_assistant_provider(&self) -> String {
        if self.assistant_use_separate_model {
            if let Some(ref p) = self.assistant_provider {
                if Self::is_builtin_provider(p)
                    || self.custom_providers.iter().any(|cp| cp.id == *p)
                {
                    return p.clone();
                }
            }
        }
        self.resolve_active_provider()
    }
}

impl UserProfile {
    pub fn screen_context_enabled(&self) -> bool {
        self.ai_polish_screen_context_enabled || self.assistant_screen_context_enabled
    }

    pub fn resolve_app_profile(
        &self,
        process_name: &str,
        window_title: &str,
    ) -> ResolvedAppProfile {
        self.app_profile_rules
            .iter()
            .find(|rule| rule.matches(process_name, window_title))
            .map(AppProfileRule::resolve)
            .unwrap_or_default()
    }

    pub fn resolve_app_profile_with_domain(
        &self,
        process_name: &str,
        window_title: &str,
        browser_domain: Option<&str>,
    ) -> ResolvedAppProfile {
        if let Some(domain) = browser_domain.map(str::trim).filter(|d| !d.is_empty()) {
            let domain_lower = domain.to_ascii_lowercase();
            if let Some(rule) = self.app_profile_rules.iter().find(|rule| {
                rule.enabled
                    && rule
                        .browser_domain
                        .as_deref()
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                        .is_some_and(|d| {
                            let target = d.to_ascii_lowercase();
                            domain_lower == target || domain_lower.ends_with(&format!(".{target}"))
                        })
            }) {
                return rule.resolve();
            }
            let legacy_domain = self.resolve_app_profile(domain, window_title);
            if legacy_domain.rule_name.is_some() {
                return legacy_domain;
            }
        }
        self.resolve_app_profile(process_name, window_title)
    }

    /// Get the hotword text list sorted by weight (for ASR injection)
    pub fn get_hot_word_texts(&self, limit: usize) -> Vec<String> {
        let mut words: Vec<&HotWord> = self.hot_words.iter().collect();
        words.sort_by(|a, b| {
            (b.source == HotWordSource::User)
                .cmp(&(a.source == HotWordSource::User))
                .then(b.weight.cmp(&a.weight))
                .then(b.use_count.cmp(&a.use_count))
        });
        words
            .into_iter()
            .take(limit)
            .map(|w| w.text.clone())
            .collect()
    }

    /// Prioritize mode-specific vocabulary ahead of global vocabulary up to `limit`.
    pub fn get_hot_word_texts_for_app(&self, app_vocab: &[String], limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        let mut merged = Vec::with_capacity(limit);
        let mut seen = std::collections::HashSet::new();
        for term in app_vocab {
            let trimmed = term.trim();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            if seen.insert(key) {
                merged.push(trimmed.to_string());
                if merged.len() >= limit {
                    return merged;
                }
            }
        }
        for term in self.get_hot_word_texts(limit) {
            let key = term.to_lowercase();
            if seen.insert(key) {
                merged.push(term);
                if merged.len() >= limit {
                    break;
                }
            }
        }
        merged
    }

    /// Dynamically retrieve relevant correction patterns for the input text
    ///
    /// Strategy: exact substring match first (input contains pattern.original),
    /// when fewer than limit are found, fill with high-frequency corrections. The User source always takes priority over Ai.
    pub fn get_relevant_corrections(&self, input: &str, limit: usize) -> Vec<&CorrectionPattern> {
        let source_ord = |s: &CorrectionSource| match s {
            CorrectionSource::User => 0,
            CorrectionSource::Ai => 1,
        };

        // Only inject corrections whose substring matches the current input; do not inject unrelated high-frequency rules
        let mut matched: Vec<&CorrectionPattern> = self
            .correction_patterns
            .iter()
            .filter(|p| !p.original.is_empty() && input.contains(&p.original))
            .collect();
        matched.sort_by(|a, b| {
            source_ord(&a.source)
                .cmp(&source_ord(&b.source))
                .then(b.count.cmp(&a.count))
        });
        matched.truncate(limit);
        matched
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApiFormat, AppProfileRule, AppRuleOverride, AppTranslationOverride, CustomProvider,
        HistorySettings, LlmProviderConfig, LlmReasoningMode, PolishStructureLevel,
        SelectionAssistantConfig, UserProfile,
    };

    #[test]
    fn polish_effort_change_keeps_assistant_selection_and_model() {
        let mut llm = LlmProviderConfig {
            custom_model: Some("kept-model".to_string()),
            reasoning_mode: LlmReasoningMode::Off,
            assistant_reasoning_mode: None,
            selection_reasoning_mode: Some(LlmReasoningMode::Deep),
            ..Default::default()
        };

        llm.set_polish_reasoning_mode(LlmReasoningMode::Light);

        assert_eq!(llm.polish_reasoning_mode(), LlmReasoningMode::Light);
        // The assistant inherits the legacy field, so clobbering it would show here.
        assert_eq!(llm.reasoning_mode, LlmReasoningMode::Off);
        assert_eq!(llm.assistant_reasoning_mode(), LlmReasoningMode::Off);
        assert_eq!(llm.selection_reasoning_mode, Some(LlmReasoningMode::Deep));
        assert_eq!(llm.custom_model.as_deref(), Some("kept-model"));
    }

    fn custom_provider(id: &str) -> CustomProvider {
        CustomProvider {
            id: id.to_string(),
            name: id.to_string(),
            base_url: format!("https://{id}.example.com"),
            model: format!("model-{id}"),
            api_format: ApiFormat::OpenaiCompat,
        }
    }

    #[test]
    fn manual_hot_words_take_priority_in_model_prompts() {
        use super::{HotWord, HotWordSource};

        let profile = UserProfile {
            hot_words: vec![
                HotWord {
                    text: "LearnedTerm".into(),
                    weight: 5,
                    source: HotWordSource::Learned,
                    use_count: 100,
                    last_used: 0,
                },
                HotWord {
                    text: "SpeakFlow".into(),
                    weight: 3,
                    source: HotWordSource::User,
                    use_count: 0,
                    last_used: 0,
                },
            ],
            ..UserProfile::default()
        };
        assert_eq!(profile.get_hot_word_texts(1), vec!["SpeakFlow"]);
        assert_eq!(
            profile.get_hot_word_texts(2),
            vec!["SpeakFlow", "LearnedTerm"]
        );
        assert!(profile.get_hot_word_texts(0).is_empty());
    }

    #[test]
    fn autostart_minimized_defaults_off_for_legacy_profiles_and_round_trips() {
        let mut value = serde_json::to_value(UserProfile::default()).unwrap();
        value.as_object_mut().unwrap().remove("autostart_minimized");
        let mut profile: UserProfile = serde_json::from_value(value).unwrap();
        assert!(!profile.autostart_minimized);

        profile.autostart_minimized = true;
        let saved = serde_json::to_string(&profile).unwrap();
        let restored: UserProfile = serde_json::from_str(&saved).unwrap();
        assert!(restored.autostart_minimized);
    }

    #[test]
    fn legacy_profile_without_structure_level_defaults_to_unstructured() {
        let mut value = serde_json::to_value(UserProfile::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("polish_structure_level");

        let profile: UserProfile = serde_json::from_value(value).unwrap();

        assert_eq!(profile.polish_structure_level, PolishStructureLevel::Off);
    }

    #[test]
    fn legacy_profile_without_screen_vision_defaults_to_disabled() {
        let mut value = serde_json::to_value(UserProfile::default()).unwrap();
        let llm_provider = value
            .get_mut("llm_provider")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap();
        llm_provider.remove("screen_vision_enabled");
        llm_provider.remove("screen_vision_provider");
        llm_provider.remove("screen_vision_model");

        let profile: UserProfile = serde_json::from_value(value).unwrap();

        assert!(!profile.llm_provider.screen_vision_enabled);
        assert!(!profile.llm_provider.has_valid_screen_vision_model());
    }

    #[test]
    fn legacy_screen_context_flags_are_unified_when_either_is_enabled() {
        let polish_only = UserProfile {
            ai_polish_screen_context_enabled: true,
            ..Default::default()
        };
        let assistant_only = UserProfile {
            assistant_screen_context_enabled: true,
            ..Default::default()
        };

        assert!(polish_only.screen_context_enabled());
        assert!(assistant_only.screen_context_enabled());
        assert!(!UserProfile::default().screen_context_enabled());
    }

    #[test]
    fn screen_vision_requires_an_existing_provider_and_model() {
        let valid = LlmProviderConfig {
            screen_vision_enabled: true,
            screen_vision_provider: Some("openai".to_string()),
            screen_vision_model: Some("gpt-4.1-mini".to_string()),
            ..Default::default()
        };
        let invalid = LlmProviderConfig {
            screen_vision_provider: Some("missing".to_string()),
            ..valid.clone()
        };

        assert!(valid.has_valid_screen_vision_model());
        assert!(!invalid.has_valid_screen_vision_model());
    }

    #[test]
    fn keeps_claude_as_an_active_builtin_provider() {
        // Claude is a preset, not a custom provider. If it is missing from the
        // builtin list the selection silently collapses to Cerebras.
        let config = LlmProviderConfig {
            active: "claude".to_string(),
            ..Default::default()
        };

        assert!(LlmProviderConfig::is_builtin_provider("claude"));
        assert_eq!(config.resolve_active_provider(), "claude");
    }

    #[test]
    fn resolves_invalid_active_provider_to_latest_custom_provider() {
        let config = LlmProviderConfig {
            active: "missing".to_string(),
            custom_providers: vec![custom_provider("a"), custom_provider("b")],
            ..Default::default()
        };

        assert_eq!(config.resolve_active_provider(), "b");
    }

    #[test]
    fn falls_back_to_previous_provider_after_removal() {
        let config = LlmProviderConfig {
            active: "b".to_string(),
            custom_providers: vec![
                custom_provider("a"),
                custom_provider("b"),
                custom_provider("c"),
            ],
            ..Default::default()
        };

        assert_eq!(config.fallback_provider_after_removal("b"), "a");
    }

    #[test]
    fn falls_back_to_last_remaining_provider_when_removing_first() {
        let config = LlmProviderConfig {
            active: "a".to_string(),
            custom_providers: vec![
                custom_provider("a"),
                custom_provider("b"),
                custom_provider("c"),
            ],
            ..Default::default()
        };

        assert_eq!(config.fallback_provider_after_removal("a"), "c");
    }

    #[test]
    fn split_reasoning_modes_fall_back_to_legacy_mode() {
        let config = LlmProviderConfig {
            active: "openai".to_string(),
            reasoning_mode: LlmReasoningMode::Light,
            ..Default::default()
        };

        assert_eq!(config.polish_reasoning_mode(), LlmReasoningMode::Light);
        assert_eq!(config.assistant_reasoning_mode(), LlmReasoningMode::Light);
    }

    #[test]
    fn selection_config_follows_polish_as_an_atomic_default() {
        let config = LlmProviderConfig {
            active: "deepseek".to_string(),
            polish_reasoning_mode: Some(LlmReasoningMode::Light),
            selection_reasoning_mode: Some(LlmReasoningMode::Deep),
            selection_provider: Some("openai".to_string()),
            selection_model: Some("gpt-5".to_string()),
            ..Default::default()
        };

        assert_eq!(config.resolve_selection_provider(), "deepseek");
        assert_eq!(config.selection_model(), None);
        assert_eq!(config.selection_reasoning_mode(), LlmReasoningMode::Light);
    }

    #[test]
    fn selection_config_falls_back_atomically_when_provider_was_removed() {
        let config = LlmProviderConfig {
            active: "deepseek".to_string(),
            polish_reasoning_mode: Some(LlmReasoningMode::Light),
            selection_use_separate_model: true,
            selection_provider: Some("removed-provider".to_string()),
            selection_model: Some("stale-model".to_string()),
            selection_reasoning_mode: Some(LlmReasoningMode::Deep),
            ..Default::default()
        };

        assert_eq!(config.resolve_selection_provider(), "deepseek");
        assert_eq!(config.selection_model(), None);
        assert_eq!(config.selection_reasoning_mode(), LlmReasoningMode::Light);
    }

    #[test]
    fn selection_config_uses_a_complete_custom_model_override() {
        let config = LlmProviderConfig {
            active: "deepseek".to_string(),
            selection_use_separate_model: true,
            selection_provider: Some("vision".to_string()),
            selection_model: Some(" vision-v2 ".to_string()),
            selection_reasoning_mode: Some(LlmReasoningMode::Off),
            custom_providers: vec![custom_provider("vision")],
            ..Default::default()
        };

        assert_eq!(config.resolve_selection_provider(), "vision");
        assert_eq!(config.selection_model(), Some("vision-v2"));
        assert_eq!(config.selection_reasoning_mode(), LlmReasoningMode::Off);
    }

    #[test]
    fn automatic_selection_screenshots_are_opt_in() {
        assert!(!SelectionAssistantConfig::default().auto_screenshot);
        assert!(!SelectionAssistantConfig::default().screenshot_routing);
        assert!(SelectionAssistantConfig::default().custom_prompt.is_none());

        let legacy: SelectionAssistantConfig =
            serde_json::from_value(serde_json::json!({ "enabled": true })).unwrap();
        assert!(!legacy.auto_screenshot);
        assert!(!legacy.screenshot_routing);
        assert!(legacy.custom_prompt.is_none());
    }

    #[test]
    fn selection_and_dictation_instructions_round_trip_independently() {
        let profile = UserProfile {
            custom_prompt: Some("Dictation instruction".to_string()),
            selection_assistant: SelectionAssistantConfig {
                custom_prompt: Some("Selection instruction".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let reloaded: UserProfile =
            serde_json::from_value(serde_json::to_value(profile).unwrap()).unwrap();
        assert_eq!(
            reloaded.custom_prompt.as_deref(),
            Some("Dictation instruction")
        );
        assert_eq!(
            reloaded.selection_assistant.custom_prompt.as_deref(),
            Some("Selection instruction")
        );
    }

    #[test]
    fn legacy_selection_length_limits_are_ignored() {
        let config: SelectionAssistantConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "min_chars": 4,
            "max_chars": 900
        }))
        .unwrap();
        let serialized = serde_json::to_value(config).unwrap();

        assert!(serialized.get("min_chars").is_none());
        assert!(serialized.get("max_chars").is_none());
    }

    /// TDD red-state: the new `openai_fast_mode` field must default to `false`
    /// on `LlmProviderConfig`, so existing profiles without the key behave
    /// exactly as before. The impl agent is responsible for adding the field
    /// (serde-flagged as `#[serde(default)]`) and wiring its default.
    #[test]
    fn default_llm_provider_config_has_fast_mode_disabled() {
        let config = LlmProviderConfig::default();

        assert!(
            !config.openai_fast_mode,
            "openai_fast_mode must default to false so existing users do not silently start consuming fast-mode credits"
        );
    }

    #[test]
    fn history_is_opt_in_and_defaults_to_no_audio() {
        let settings = HistorySettings::default();
        assert!(!settings.enabled);
        assert!(!settings.save_audio);
        assert_eq!(settings.retention_days, 90);
    }

    #[test]
    fn first_matching_app_rule_resolves_case_insensitively() {
        let profile = UserProfile {
            app_profile_rules: vec![
                AppProfileRule {
                    id: "specific".into(),
                    name: "VS Code Markdown".into(),
                    process_name: "Code.exe".into(),
                    window_title_contains: Some("README".into()),
                    ai_polish: AppRuleOverride::Enabled,
                    translation: AppTranslationOverride::Target,
                    translation_target: Some("English".into()),
                    history: AppRuleOverride::Disabled,
                    ..Default::default()
                },
                AppProfileRule {
                    id: "fallback".into(),
                    name: "VS Code".into(),
                    process_name: "code".into(),
                    ai_polish: AppRuleOverride::Disabled,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let resolved = profile.resolve_app_profile("CODE.EXE", "README.md - speak-flow");
        assert_eq!(resolved.rule_id.as_deref(), Some("specific"));
        assert_eq!(resolved.ai_polish_enabled, Some(true));
        assert_eq!(resolved.translation_target, Some(Some("English".into())));
        assert_eq!(resolved.history_enabled, Some(false));
    }

    #[test]
    fn app_rule_with_nonmatching_title_falls_through() {
        let profile = UserProfile {
            app_profile_rules: vec![
                AppProfileRule {
                    id: "specific".into(),
                    name: "Only README".into(),
                    process_name: "Code.exe".into(),
                    window_title_contains: Some("README".into()),
                    ..Default::default()
                },
                AppProfileRule {
                    id: "fallback".into(),
                    name: "All VS Code".into(),
                    process_name: "Code.exe".into(),
                    screen_context: AppRuleOverride::Disabled,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let resolved = profile.resolve_app_profile("Code.exe", "main.rs - speak-flow");
        assert_eq!(resolved.rule_id.as_deref(), Some("fallback"));
        assert_eq!(resolved.screen_context_enabled, Some(false));
    }
}
