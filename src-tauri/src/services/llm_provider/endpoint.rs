use super::*;

/// LLM 提供商配置
pub struct LlmEndpoint {
    pub provider: String,
    pub api_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub api_format: ApiFormat,
}

pub const KEYRING_SERVICE: &str = "speak-flow";

pub(super) const CEREBRAS: &str = "cerebras";
pub(super) const OPENAI: &str = "openai";
pub(super) const XAI: &str = "xai";
pub(super) const DEEPSEEK: &str = "deepseek";
pub(super) const SILICONFLOW: &str = "siliconflow";
pub(super) const OPENROUTER: &str = "openrouter";
pub(super) const CUSTOM: &str = "custom";
pub(super) const CLAUDE: &str = "claude";
pub(super) const MAGPIE: &str = "magpie";

/// 预置服务商列表（用于判断是否为预置）
pub(super) const PRESET_PROVIDERS: &[&str] = &[
    CEREBRAS,
    OPENAI,
    XAI,
    DEEPSEEK,
    SILICONFLOW,
    OPENROUTER,
    CUSTOM,
    CLAUDE,
    MAGPIE,
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LlmReasoningSupport {
    pub supported: bool,
    pub strategy: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoReasoningStrategy {
    OpenaiResponsesReasoning,
    OpenaiChatReasoningEffort,
    TopLevelThinking,
    ChatTemplateThinking,
    NoControls,
}

impl AutoReasoningStrategy {
    pub fn strategy_name(self) -> &'static str {
        match self {
            Self::OpenaiResponsesReasoning => "auto_openai_responses_reasoning",
            Self::OpenaiChatReasoningEffort => "auto_openai_chat_reasoning_effort",
            Self::TopLevelThinking => "auto_top_level_thinking",
            Self::ChatTemplateThinking => "auto_chat_template_thinking",
            Self::NoControls => "auto_no_reasoning_controls",
        }
    }
}

pub(super) static AUTO_REASONING_STRATEGY_CACHE: OnceLock<
    parking_lot::Mutex<HashMap<String, AutoReasoningStrategy>>,
> = OnceLock::new();

pub(crate) fn default_endpoint_parts(provider: &str) -> (&'static str, &'static str, u64) {
    match provider {
        OPENAI => ("https://api.openai.com", "gpt-6-luna", 10),
        XAI => ("https://api.x.ai", "grok-4.7", 10),
        DEEPSEEK => ("https://api.deepseek.com", "deepseek-flash", 10),
        SILICONFLOW => ("https://api.siliconflow.cn", "Qwen/Qwen3.8-27B", 10),
        OPENROUTER => (
            "https://openrouter.ai/api",
            "~deepseek/deepseek-flash-latest",
            10,
        ),
        CLAUDE => ("https://api.anthropic.com", "claude-sonnet-4-5", 15),
        // Magpie routes to whichever subscription it holds, so the model is
        // chosen from its own list rather than guessed here.
        MAGPIE => ("http://127.0.0.1:3425/v1", "", 30),
        CUSTOM => ("http://127.0.0.1:8000", "gpt-6-luna", 10),
        _ => ("https://api.cerebras.ai", "qwen-3.8-27b", 5),
    }
}

pub(super) fn default_api_suffix(provider: &str) -> &'static str {
    match provider {
        OPENAI | XAI => "responses",
        CLAUDE => "messages",
        _ => "chat/completions",
    }
}

pub(super) fn normalize_api_url(
    input: Option<&str>,
    default_base_url: &str,
    api_suffix: &str,
) -> String {
    let raw = input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base_url);

    if let Some(explicit) = raw.strip_suffix('#') {
        return explicit.trim_end_matches('/').to_string();
    }

    let trimmed = raw.trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/chat/completions") || lower.ends_with("/responses") {
        return trimmed.to_string();
    }

    if lower.ends_with("/v1") || lower.ends_with("/api/v3") {
        return format!("{trimmed}/{api_suffix}");
    }

    format!("{trimmed}/v1/{api_suffix}")
}

pub(super) fn normalize_anthropic_url(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.anthropic.com/v1/messages".to_string();
    }

    if let Some(explicit) = trimmed.strip_suffix('#') {
        return explicit.trim_end_matches('/').to_string();
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/messages") {
        return trimmed.to_string();
    }
    if lower.ends_with("/v1") {
        return format!("{trimmed}/messages");
    }

    format!("{trimmed}/v1/messages")
}

pub(super) fn normalize_models_url(input: Option<&str>, default_base_url: &str) -> String {
    let raw = input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_base_url);

    let trimmed = raw.trim_end_matches('#').trim_end_matches('/');
    let lower = trimmed.to_ascii_lowercase();

    if lower.ends_with("/models") {
        return trimmed.to_string();
    }
    if lower.ends_with("/chat/completions") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/chat/completions".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/responses") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/responses".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/v1") || lower.ends_with("/api/v3") {
        return format!("{trimmed}/models");
    }

    format!("{trimmed}/v1/models")
}

pub(super) fn is_preset(provider: &str) -> bool {
    PRESET_PROVIDERS.contains(&provider)
}

/// 根据后端配置获取 LLM 端点
pub fn endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    endpoint_for_provider(config, &config.resolve_active_provider(), None)
}

pub(super) fn endpoint_for_provider(
    config: &LlmProviderConfig,
    provider: &str,
    model_override: Option<&str>,
) -> LlmEndpoint {
    if is_preset(provider) {
        let (default_base_url, default_model, timeout_secs) = default_endpoint_parts(provider);
        let use_active_overrides = provider == config.resolve_active_provider();
        let use_custom_endpoint = provider == CUSTOM && use_active_overrides;
        let api_suffix = default_api_suffix(provider);
        let api_format = if provider == CLAUDE {
            ApiFormat::Anthropic
        } else {
            ApiFormat::OpenaiCompat
        };
        let api_url = if provider == CLAUDE {
            normalize_anthropic_url(default_base_url)
        } else if use_custom_endpoint {
            normalize_api_url(
                config.custom_base_url.as_deref(),
                default_base_url,
                api_suffix,
            )
        } else {
            normalize_api_url(None, default_base_url, api_suffix)
        };
        LlmEndpoint {
            provider: provider.to_string(),
            api_url,
            model: model_override
                .or(config
                    .custom_model
                    .as_deref()
                    .filter(|_| use_active_overrides))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(default_model)
                .to_string(),
            timeout_secs,
            api_format,
        }
    } else if let Some(cp) = config.custom_providers.iter().find(|p| p.id == provider) {
        let model = model_override.unwrap_or(&cp.model);
        let api_url = match cp.api_format {
            ApiFormat::Anthropic => normalize_anthropic_url(&cp.base_url),
            ApiFormat::OpenaiCompat => normalize_api_url(
                Some(&cp.base_url),
                "http://127.0.0.1:8000",
                "chat/completions",
            ),
        };
        LlmEndpoint {
            provider: provider.to_string(),
            api_url,
            model: if model.trim().is_empty() {
                default_endpoint_parts(OPENAI).1.to_string()
            } else {
                model.to_string()
            },
            timeout_secs: if cp.api_format == ApiFormat::Anthropic {
                30
            } else {
                10
            },
            api_format: cp.api_format.clone(),
        }
    } else {
        // fallback to cerebras
        let (base, model, timeout) = default_endpoint_parts(CEREBRAS);
        LlmEndpoint {
            provider: CEREBRAS.to_string(),
            api_url: normalize_api_url(None, base, "chat/completions"),
            model: model.to_string(),
            timeout_secs: timeout,
            api_format: ApiFormat::OpenaiCompat,
        }
    }
}

pub fn assistant_endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    endpoint_for_provider(
        config,
        &config.resolve_assistant_provider(),
        config.assistant_model(),
    )
}

/// Resolve the selection assistant independently while retaining the polish
/// endpoint as the exact fallback when its separate-config switch is off.
pub fn selection_endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    endpoint_for_provider(
        config,
        &config.resolve_selection_provider(),
        config.selection_model(),
    )
}

pub fn validation_endpoint_for_config(config: &LlmProviderConfig) -> LlmEndpoint {
    endpoint_for_provider(
        config,
        &config.resolve_validation_provider(),
        config.validation_model(),
    )
}

pub fn screen_vision_endpoint_for_config(config: &LlmProviderConfig) -> Option<LlmEndpoint> {
    let provider = config.resolve_screen_vision_provider()?;
    let model = config.screen_vision_model()?;
    Some(endpoint_for_provider(config, &provider, Some(model)))
}

pub fn image_support_probe_url(endpoint: &LlmEndpoint) -> Option<String> {
    if endpoint.api_format != ApiFormat::OpenaiCompat {
        return None;
    }

    let model = endpoint.model.trim();
    if model.is_empty() {
        return None;
    }

    let models_url = normalize_models_url(Some(&endpoint.api_url), &endpoint.api_url);
    let mut url = reqwest::Url::parse(&models_url).ok()?;
    // DeepSeek publishes model capabilities through its model list endpoint.
    // https://api-docs.deepseek.com/api/list-models/
    if endpoint.provider != DEEPSEEK {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.push(model);
    }
    Some(url.into())
}

pub fn cerebras_public_model_probe_url(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    let mut url = reqwest::Url::parse("https://api.cerebras.ai/public/v1/models/").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        segments.push(model);
    }
    url.query_pairs_mut().append_pair("format", "openrouter");
    Some(url.into())
}

pub fn should_probe_cerebras_public_model_metadata(endpoint: &LlmEndpoint) -> bool {
    if is_cerebras_like_endpoint(endpoint) {
        return true;
    }

    endpoint
        .model
        .trim()
        .to_ascii_lowercase()
        .contains("gpt-oss")
}

pub fn parse_image_input_support_from_model_metadata(payload: &Value) -> Option<bool> {
    let model = payload
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .unwrap_or(payload);

    let modalities = model.get("input_modalities")?.as_array()?;
    let normalized = modalities
        .iter()
        .filter_map(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();

    if normalized.is_empty() {
        return None;
    }

    if normalized
        .iter()
        .any(|value| value == "image" || value == "input_image")
    {
        return Some(true);
    }

    if normalized.iter().any(|value| value == "text") {
        return Some(false);
    }

    None
}

pub async fn probe_image_support_from_provider_metadata(
    http_client: &reqwest::Client,
    endpoint: &LlmEndpoint,
    api_key: &str,
) -> Option<bool> {
    if codex_oauth_service::decode_chatgpt_bearer_token(api_key).is_some() {
        return None;
    }

    let mut probe_targets = Vec::new();
    if let Some(url) = image_support_probe_url(endpoint) {
        probe_targets.push((url, true));
    }
    if should_probe_cerebras_public_model_metadata(endpoint) {
        if let Some(url) = cerebras_public_model_probe_url(&endpoint.model) {
            if !probe_targets.iter().any(|(existing, _)| existing == &url) {
                probe_targets.push((url, false));
            }
        }
    }

    for (url, use_auth) in probe_targets {
        let mut request = http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(2));
        if use_auth {
            let Ok(headers) = build_auth_headers(&endpoint.api_format, api_key) else {
                continue;
            };
            request = request.headers(headers);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => {
                log::debug!("探测模型图片能力失败: url={}, err={}", url, err);
                continue;
            }
        };

        if !response.status().is_success() {
            log::debug!(
                "模型图片能力探测返回非成功状态: status={}, url={}",
                response.status(),
                url
            );
            continue;
        }

        let payload = match response.json::<Value>().await {
            Ok(payload) => payload,
            Err(err) => {
                log::debug!("解析模型图片能力元数据失败: url={}, err={}", url, err);
                continue;
            }
        };

        let metadata = if endpoint.provider == DEEPSEEK {
            let Some(model) = payload
                .get("data")
                .and_then(Value::as_array)
                .and_then(|models| {
                    models.iter().find(|model| {
                        model.get("id").and_then(Value::as_str) == Some(endpoint.model.trim())
                    })
                })
            else {
                continue;
            };
            model
        } else {
            &payload
        };
        if let Some(supported) = parse_image_input_support_from_model_metadata(metadata) {
            return Some(supported);
        }
    }

    None
}

pub fn endpoint_for_preview(
    provider: &str,
    base_url: Option<&str>,
    model: Option<&str>,
    api_format: ApiFormat,
) -> LlmEndpoint {
    let config = if is_preset(provider) {
        LlmProviderConfig {
            active: provider.to_string(),
            custom_base_url: base_url.map(str::to_string),
            custom_model: model.map(str::to_string),
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
    } else {
        LlmProviderConfig {
            active: provider.to_string(),
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
            custom_providers: vec![CustomProvider {
                id: provider.to_string(),
                name: provider.to_string(),
                base_url: base_url.unwrap_or_default().to_string(),
                model: model.unwrap_or_default().to_string(),
                api_format,
            }],
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
    };

    endpoint_for_config(&config)
}

pub fn endpoint_uses_responses_api(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat
        && endpoint
            .api_url
            .trim_end_matches('/')
            .to_ascii_lowercase()
            .ends_with("/responses")
}

pub fn image_support_cache_key(endpoint: &LlmEndpoint) -> String {
    format!(
        "{:?}|{}|{}|{}",
        endpoint.api_format,
        endpoint.provider,
        endpoint.api_url,
        endpoint.model.trim().to_ascii_lowercase()
    )
}

pub(super) fn indicates_unsupported(normalized: &str) -> bool {
    normalized.contains("not supported")
        || normalized.contains("unsupported")
        || normalized.contains("does not support")
        || normalized.contains("not permitted")
        || normalized.contains("are not valid")
        || normalized.contains("invalidparameter")
        || normalized.contains("invalid parameter")
        || normalized.contains("badrequest")
}

pub fn looks_like_image_input_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_image = normalized.contains("image")
        || normalized.contains("vision")
        || normalized.contains("multimodal")
        || normalized.contains("input_image")
        || normalized.contains("image_url");

    mentions_image
        && (indicates_unsupported(&normalized)
            || normalized.contains("invalid image")
            || normalized.contains("invalid content type")
            || normalized.contains("unsupported content type")
            || normalized.contains("unsupported modality")
            || normalized.contains("modalities are not supported")
            || (normalized.contains("unknown variant")
                && normalized.contains("expected")
                && normalized.contains("text"))
            || normalized.contains("invalid_value"))
}

pub fn looks_like_web_search_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_search = normalized.contains("web_search")
        || normalized.contains("web search")
        || normalized.contains("websearch")
        || normalized.contains("search_preview");

    mentions_search
        && (indicates_unsupported(&normalized)
            || normalized.contains("unknown")
            || normalized.contains("invalid"))
}

pub fn looks_like_json_output_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_json_output = normalized.contains("response_format")
        || normalized.contains("json_object")
        || normalized.contains("text.format")
        || normalized.contains("json schema")
        || normalized.contains("structured output");

    mentions_json_output && indicates_unsupported(&normalized)
}

pub fn looks_like_reasoning_unsupported_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    let mentions_reasoning = normalized.contains("reasoning")
        || normalized.contains("reasoning_effort")
        || normalized.contains("thinking")
        || normalized.contains("chat_template_kwargs")
        || normalized.contains("budget_tokens")
        || normalized.contains("reasoning_content");

    mentions_reasoning
        && (indicates_unsupported(&normalized) || normalized.contains("unknown parameter"))
}

pub(super) fn endpoint_host(endpoint: &LlmEndpoint) -> Option<String> {
    reqwest::Url::parse(&endpoint.api_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
}

pub(super) fn endpoint_host_matches(endpoint: &LlmEndpoint, domain: &str) -> bool {
    let Some(host) = endpoint_host(endpoint) else {
        return false;
    };
    host == domain || host.ends_with(&format!(".{domain}"))
}

pub fn is_volcengine_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    if endpoint.api_format != ApiFormat::OpenaiCompat {
        return false;
    }

    let model = endpoint.model.trim().to_ascii_lowercase();

    endpoint_host_matches(endpoint, "volces.com")
        || endpoint_host_matches(endpoint, "volcengine.com")
        || model.contains("doubao")
        || model.contains("seed-")
}

pub(super) fn is_xai_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == XAI
        || endpoint_host_matches(endpoint, "api.x.ai")
        || endpoint_host_matches(endpoint, "cli-chat-proxy.grok.com")
}

pub(super) fn grok_model_tail(model: &str) -> &str {
    let normalized = model.trim();
    normalized.rsplit('/').next().unwrap_or(normalized)
}

pub(super) fn supports_xai_reasoning(model: &str) -> bool {
    grok_model_tail(model)
        .to_ascii_lowercase()
        .starts_with("grok-")
}

pub(super) fn grok_supports_disabled_reasoning(model: &str) -> bool {
    let tail = grok_model_tail(model).to_ascii_lowercase();
    tail.starts_with("grok-4.3") || tail.starts_with("grok-3")
}

/// Grok 4.5 and later cannot disable reasoning; omitted effort defaults to high.
pub(super) fn grok_effort_for_mode(model: &str, mode: LlmReasoningMode) -> Option<&'static str> {
    match mode {
        LlmReasoningMode::Off => {
            if grok_supports_disabled_reasoning(model) {
                Some("none")
            } else {
                Some("low")
            }
        }
        LlmReasoningMode::Light => Some("low"),
        LlmReasoningMode::Balanced => Some("medium"),
        LlmReasoningMode::Deep => Some("high"),
        LlmReasoningMode::ProviderDefault => None,
    }
}

pub(super) fn is_deepseek_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == DEEPSEEK || endpoint_host_matches(endpoint, "deepseek.com")
}

pub(super) fn is_siliconflow_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == SILICONFLOW || endpoint_host_matches(endpoint, "siliconflow.com")
}

pub fn is_cerebras_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == CEREBRAS || endpoint_host_matches(endpoint, "cerebras.ai")
}

/// OpenRouter 统一使用 `reasoning` 对象控制思考强度，并与顶层 `reasoning_effort`
/// 互为简写。按 host 判定，因此预置项与自定义接入都能命中。
pub fn is_openrouter_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat
        && (endpoint.provider == OPENROUTER || endpoint_host_matches(endpoint, "openrouter.ai"))
}
