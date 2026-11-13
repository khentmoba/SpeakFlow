use super::*;

pub(super) const GPT5_EFFORTS: &[&str] = &["minimal", "low", "medium", "high"];
pub(super) const GPT5_1_EFFORTS: &[&str] = &["none", "low", "medium", "high"];
pub(super) const GPT5_2_54_EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];
pub(super) const GPT5_5_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
// The public API also accepts `none`, while the Codex OAuth catalog starts at
// `low`. Use the common set so the same saved mode works with either credential.
pub(super) const GPT5_6_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
pub(super) const GPT5_PRO_EFFORTS: &[&str] = &["high"];
pub(super) const GPT5_2_54_PRO_EFFORTS: &[&str] = &["medium", "high", "xhigh"];
pub(super) const GPT5_2_3_CODEX_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh"];
pub(super) const GPT5_1_CODEX_MAX_EFFORTS: &[&str] = &["none", "medium", "high", "xhigh"];

pub(super) fn openai_reasoning_efforts(model: &str) -> Option<&'static [&'static str]> {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    match tail {
        "gpt-5-pro" => Some(GPT5_PRO_EFFORTS),
        _ if tail.starts_with("gpt-5-pro-") => Some(GPT5_PRO_EFFORTS),
        "gpt-5.2-pro" | "gpt-5.4-pro" | "gpt-5.5-pro" => Some(GPT5_2_54_PRO_EFFORTS),
        _ if tail.starts_with("gpt-5.2-pro-")
            || tail.starts_with("gpt-5.4-pro-")
            || tail.starts_with("gpt-5.5-pro-") =>
        {
            Some(GPT5_2_54_PRO_EFFORTS)
        }
        "gpt-5.2-codex" | "gpt-5.3-codex" => Some(GPT5_2_3_CODEX_EFFORTS),
        _ if tail.starts_with("gpt-5.2-codex-") || tail.starts_with("gpt-5.3-codex-") => {
            Some(GPT5_2_3_CODEX_EFFORTS)
        }
        "gpt-5.1-codex-max" => Some(GPT5_1_CODEX_MAX_EFFORTS),
        _ if tail.starts_with("gpt-5.1-codex-max-") => Some(GPT5_1_CODEX_MAX_EFFORTS),
        "gpt-5-mini" | "gpt-5-nano" => Some(GPT5_EFFORTS),
        _ if tail.starts_with("gpt-5-mini-") || tail.starts_with("gpt-5-nano-") => {
            Some(GPT5_EFFORTS)
        }
        "gpt-5.1" => Some(GPT5_1_EFFORTS),
        _ if tail.starts_with("gpt-5.1-") => Some(GPT5_1_EFFORTS),
        "gpt-5.2" | "gpt-5.4" => Some(GPT5_2_54_EFFORTS),
        _ if tail.starts_with("gpt-5.2-") || tail.starts_with("gpt-5.4-") => {
            Some(GPT5_2_54_EFFORTS)
        }
        "gpt-5.5" => Some(GPT5_5_EFFORTS),
        _ if tail.starts_with("gpt-5.5-") => Some(GPT5_5_EFFORTS),
        "gpt-5.6" => Some(GPT5_6_EFFORTS),
        _ if tail.starts_with("gpt-5.6-") => Some(GPT5_6_EFFORTS),
        "gpt-6-astra" => Some(GPT5_6_EFFORTS),
        "gpt-6-sol" | "gpt-6.1-sol" | "gpt-6-luna" => Some(GPT5_6_EFFORTS),
        "gpt-5" => Some(GPT5_EFFORTS),
        _ if tail.starts_with("gpt-5-") => Some(GPT5_EFFORTS),
        _ => None,
    }
}

pub(super) fn openai_effort_for_mode(model: &str, mode: LlmReasoningMode) -> Option<&'static str> {
    let efforts = openai_reasoning_efforts(model)?;
    let index = match mode {
        LlmReasoningMode::Off => 0,
        LlmReasoningMode::Light => 1,
        LlmReasoningMode::Balanced => 2,
        LlmReasoningMode::Deep => 3,
        LlmReasoningMode::ProviderDefault => return None,
    }
    .min(efforts.len() - 1);

    Some(efforts[index])
}

pub(super) fn supports_anthropic_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("claude-3-7-sonnet")
        || model.contains("claude-sonnet-4")
        || model.contains("claude-opus-4")
}

pub(super) fn supports_volcengine_thinking(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    (model.contains("doubao-seed-1-6-")
        || model.contains("doubao-seed-2-0-")
        || model.contains("doubao-1.5-thinking-pro")
        || model.contains("doubao"))
        && (model.contains("thinking")
            || model.contains("flash")
            || model.contains("seed-2-0-mini")
            || model.contains("seed-2-0-lite")
            || model.contains("seed-2-0-pro"))
}

pub(super) fn supports_siliconflow_reasoning(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    normalized.contains("qwen/qwen3-")
        || tail == "qwen3.8-27b"
        || normalized.contains("qwen/qwq-")
        || normalized.contains("thudm/glm-z1-")
        || normalized.contains("minimaxai/minimax-m2.1")
        || normalized.contains("tencent/hunyuan-a13b-instruct")
        || normalized.contains("deepseek-ai/deepseek-r1")
        || normalized.contains("glm-4.1v-9b-thinking")
        || tail.starts_with("qwen3-")
        || tail.starts_with("qwq-")
        || tail.starts_with("glm-z1-")
        || tail.contains("deepseek-r1")
        || tail.contains("thinking")
}

pub(super) fn supports_cerebras_reasoning(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(tail, "gpt-oss-120b" | "qwen-3.8-27b")
}

pub(super) fn supports_deepseek_thinking(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
    matches!(
        tail,
        "deepseek-flash"
            | "deepseek-v4-flash"
            | "deepseek-v4-pro"
            | "deepseek-chat"
            | "deepseek-reasoner"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReasoningControlKind {
    OpenaiEffort,
    XaiReasoningEffort,
    AnthropicThinking,
    DeepSeekThinkingToggle,
    SiliconFlowThinkingBudget,
    CerebrasReasoningEffort,
    CerebrasGlmToggle,
    VolcengineThinkingType,
    OpenRouterReasoning,
    AutoOpenaiCompat,
}

impl ReasoningControlKind {
    fn strategy_name(self) -> &'static str {
        match self {
            Self::OpenaiEffort => "openai_reasoning_effort",
            Self::XaiReasoningEffort => "xai_reasoning_effort",
            Self::AnthropicThinking => "anthropic_thinking",
            Self::DeepSeekThinkingToggle => "deepseek_thinking",
            Self::SiliconFlowThinkingBudget => "siliconflow_thinking_budget",
            Self::CerebrasReasoningEffort => "cerebras_reasoning_effort",
            Self::CerebrasGlmToggle => "cerebras_disable_reasoning",
            Self::VolcengineThinkingType => "volcengine_thinking_type",
            Self::OpenRouterReasoning => "openrouter_reasoning",
            Self::AutoOpenaiCompat => "auto_openai_compat_probe",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::OpenaiEffort => {
                "This model supports reasoning effort; Off, Light, Balanced, and Deep map to the corresponding reasoning level."
            }
            Self::XaiReasoningEffort => {
                "This Grok model supports reasoning.effort; Off and Light map to low (Grok 4.5 and later cannot disable thinking and default to high when omitted)."
            }
            Self::AnthropicThinking => {
                "This model supports extended thinking and maps levels to thinking + budget_tokens."
            }
            Self::DeepSeekThinkingToggle => {
                "This model supports thinking.type; Off sends disabled and other levels enable thinking."
            }
            Self::SiliconFlowThinkingBudget => {
                "This model supports thinking_budget and maps each level to a token budget."
            }
            Self::CerebrasReasoningEffort => {
                "This model supports reasoning_effort and maps each level to the corresponding effort."
            }
            Self::CerebrasGlmToggle => {
                "This model supports disable_reasoning; Off disables reasoning and other levels enable it."
            }
            Self::VolcengineThinkingType => {
                "This model supports thinking.type; Off=disabled, Light/Balanced=auto, Deep=enabled."
            }
            Self::OpenRouterReasoning => {
                "This is an OpenRouter endpoint and uses the unified reasoning parameter; the Off level sends reasoning.enabled=false and the other levels map to reasoning.effort."
            }
            Self::AutoOpenaiCompat => {
                "OpenAI-compatible backend; automatically probes supported reasoning parameters and caches the working strategy."
            }
        }
    }
}

pub(super) fn reasoning_control_kind(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
) -> Option<ReasoningControlKind> {
    let model = endpoint.model.trim();

    if endpoint.api_format == ApiFormat::Anthropic {
        return supports_anthropic_thinking(model)
            .then_some(ReasoningControlKind::AnthropicThinking);
    }

    // host takes priority over model-name heuristics: OpenRouter proxies models from any vendor upstream
    // (e.g. doubao/seed-* would be hit by is_volcengine_like_endpoint), so we must return early here.
    if is_openrouter_like_endpoint(endpoint) {
        return Some(ReasoningControlKind::OpenRouterReasoning);
    }

    if is_volcengine_like_endpoint(endpoint)
        && !uses_responses_api
        && supports_volcengine_thinking(model)
    {
        return Some(ReasoningControlKind::VolcengineThinkingType);
    }

    if is_deepseek_like_endpoint(endpoint) && supports_deepseek_thinking(model) {
        return Some(ReasoningControlKind::DeepSeekThinkingToggle);
    }

    if is_siliconflow_like_endpoint(endpoint) && supports_siliconflow_reasoning(model) {
        return Some(ReasoningControlKind::SiliconFlowThinkingBudget);
    }

    if is_xai_like_endpoint(endpoint) && supports_xai_reasoning(model) {
        return Some(ReasoningControlKind::XaiReasoningEffort);
    }

    if is_cerebras_like_endpoint(endpoint) && supports_cerebras_reasoning(model) {
        return Some(ReasoningControlKind::CerebrasReasoningEffort);
    }

    if is_cerebras_like_endpoint(endpoint) {
        let normalized = model.trim().to_ascii_lowercase();
        let tail = normalized.rsplit('/').next().unwrap_or(&normalized);
        if tail == "zai-glm-4.7" {
            return Some(ReasoningControlKind::CerebrasGlmToggle);
        }
    }

    if is_auto_probe_openai_compatible_endpoint(endpoint) {
        return Some(ReasoningControlKind::AutoOpenaiCompat);
    }

    if openai_reasoning_efforts(model).is_some() {
        return Some(ReasoningControlKind::OpenaiEffort);
    }

    None
}

pub(super) fn is_auto_probe_openai_compatible_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.api_format == ApiFormat::OpenaiCompat && !is_openai_like_endpoint(endpoint)
}

pub fn is_openai_like_endpoint(endpoint: &LlmEndpoint) -> bool {
    endpoint.provider == OPENAI || endpoint_host(endpoint).as_deref() == Some("api.openai.com")
}

pub fn reasoning_support(endpoint: &LlmEndpoint, uses_responses_api: bool) -> LlmReasoningSupport {
    if let Some(kind) = reasoning_control_kind(endpoint, uses_responses_api) {
        return LlmReasoningSupport {
            supported: true,
            strategy: Some(kind.strategy_name().to_string()),
            summary: kind.summary().to_string(),
        };
    }

    let summary = if endpoint.api_format == ApiFormat::Anthropic {
        "This Anthropic model does not support official extended thinking; thinking mode is unavailable."
    } else if is_volcengine_like_endpoint(endpoint) {
        "This Volcengine Ark model does not support official thinking.type controls; thinking mode is unavailable."
    } else if is_deepseek_like_endpoint(endpoint) {
        "No official thinking controls were detected for this DeepSeek model; thinking mode is unavailable."
    } else if is_siliconflow_like_endpoint(endpoint) {
        "This SiliconFlow model does not support official thinking_budget controls; thinking mode is unavailable."
    } else if is_cerebras_like_endpoint(endpoint) {
        "No official reasoning_effort support was detected for this Cerebras model; thinking mode is unavailable."
    } else if openai_reasoning_efforts(&endpoint.model).is_some() {
        "This model name looks like an OpenAI GPT model, but the current endpoint path does not support those reasoning controls."
    } else {
        "No official thinking controls were detected for this model; thinking mode is unavailable."
    };

    LlmReasoningSupport {
        supported: false,
        strategy: None,
        summary: summary.to_string(),
    }
}

pub fn reasoning_support_for_mode(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> LlmReasoningSupport {
    let support = reasoning_support(endpoint, uses_responses_api);
    if mode == LlmReasoningMode::ProviderDefault || !support.supported {
        return support;
    }

    if is_auto_reasoning_endpoint(endpoint, uses_responses_api)
        && cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode)
            == Some(AutoReasoningStrategy::NoControls)
    {
        return LlmReasoningSupport {
            supported: false,
            strategy: Some(
                AutoReasoningStrategy::NoControls
                    .strategy_name()
                    .to_string(),
            ),
            summary: "The backend rejected thinking controls for this level; requests will use the model default behavior.".to_string(),
        };
    }

    support
}

pub(super) fn auto_reasoning_intent(mode: LlmReasoningMode) -> &'static str {
    match mode {
        LlmReasoningMode::Off => "off",
        LlmReasoningMode::Light => "light",
        LlmReasoningMode::Balanced => "balanced",
        LlmReasoningMode::Deep => "deep",
        LlmReasoningMode::ProviderDefault => "provider_default",
    }
}

pub(super) fn auto_reasoning_strategy_cache_key(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    intent: &str,
) -> String {
    format!(
        "{:?}|{}|{}|{}|{}",
        endpoint.api_format,
        endpoint.api_url,
        endpoint.model.trim().to_ascii_lowercase(),
        uses_responses_api,
        intent
    )
}

pub(super) fn auto_reasoning_strategy_cache(
) -> &'static parking_lot::Mutex<HashMap<String, AutoReasoningStrategy>> {
    AUTO_REASONING_STRATEGY_CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

pub fn cached_auto_reasoning_strategy(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> Option<AutoReasoningStrategy> {
    auto_reasoning_strategy_cache()
        .lock()
        .get(&auto_reasoning_strategy_cache_key(
            endpoint,
            uses_responses_api,
            auto_reasoning_intent(mode),
        ))
        .copied()
}

pub fn remember_auto_reasoning_strategy(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    mode: LlmReasoningMode,
    strategy: AutoReasoningStrategy,
) {
    auto_reasoning_strategy_cache().lock().insert(
        auto_reasoning_strategy_cache_key(
            endpoint,
            uses_responses_api,
            auto_reasoning_intent(mode),
        ),
        strategy,
    );
}

pub fn is_auto_reasoning_endpoint(endpoint: &LlmEndpoint, uses_responses_api: bool) -> bool {
    reasoning_control_kind(endpoint, uses_responses_api)
        == Some(ReasoningControlKind::AutoOpenaiCompat)
}

pub(super) fn preferred_auto_reasoning_strategy(
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> AutoReasoningStrategy {
    if mode == LlmReasoningMode::Off {
        AutoReasoningStrategy::TopLevelThinking
    } else if uses_responses_api {
        AutoReasoningStrategy::OpenaiResponsesReasoning
    } else {
        AutoReasoningStrategy::OpenaiChatReasoningEffort
    }
}

pub(super) fn auto_reasoning_fallback_strategies(
    uses_responses_api: bool,
    mode: LlmReasoningMode,
) -> &'static [AutoReasoningStrategy] {
    const RESPONSES: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::OpenaiChatReasoningEffort];
    const CHAT_OFF: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::ChatTemplateThinking];
    const CHAT_EFFORT: &[AutoReasoningStrategy] = &[AutoReasoningStrategy::TopLevelThinking];

    if mode == LlmReasoningMode::Off {
        CHAT_OFF
    } else if uses_responses_api {
        RESPONSES
    } else {
        CHAT_EFFORT
    }
}

pub(super) fn auto_effort_for_mode(mode: LlmReasoningMode) -> Option<&'static str> {
    match mode {
        LlmReasoningMode::Off => None,
        LlmReasoningMode::Light => Some("low"),
        LlmReasoningMode::Balanced => Some("medium"),
        LlmReasoningMode::Deep => Some("high"),
        LlmReasoningMode::ProviderDefault => None,
    }
}

pub(super) fn apply_auto_reasoning_strategy(
    body: &mut serde_json::Value,
    strategy: AutoReasoningStrategy,
    mode: LlmReasoningMode,
) {
    match strategy {
        AutoReasoningStrategy::OpenaiResponsesReasoning => {
            let Some(effort) = auto_effort_for_mode(mode) else {
                return;
            };
            body["reasoning"] = serde_json::json!({ "effort": effort });
        }
        AutoReasoningStrategy::OpenaiChatReasoningEffort => {
            let Some(effort) = auto_effort_for_mode(mode) else {
                return;
            };
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        AutoReasoningStrategy::TopLevelThinking => {
            let thinking_type = if mode == LlmReasoningMode::Off {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        AutoReasoningStrategy::ChatTemplateThinking => {
            body["chat_template_kwargs"] =
                serde_json::json!({ "thinking": mode != LlmReasoningMode::Off });
        }
        AutoReasoningStrategy::NoControls => {}
    }
}

pub fn applied_auto_reasoning_strategy(body: &serde_json::Value) -> Option<AutoReasoningStrategy> {
    let map = body.as_object()?;
    if map.contains_key("thinking") {
        return Some(AutoReasoningStrategy::TopLevelThinking);
    }
    if map.contains_key("chat_template_kwargs") {
        return Some(AutoReasoningStrategy::ChatTemplateThinking);
    }
    if map.contains_key("reasoning") {
        return Some(AutoReasoningStrategy::OpenaiResponsesReasoning);
    }
    if map.contains_key("reasoning_effort") {
        return Some(AutoReasoningStrategy::OpenaiChatReasoningEffort);
    }
    None
}

pub fn auto_reasoning_fallback_bodies(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    request_body: &serde_json::Value,
    mode: LlmReasoningMode,
) -> Vec<(AutoReasoningStrategy, serde_json::Value)> {
    if !is_auto_reasoning_endpoint(endpoint, uses_responses_api)
        || mode == LlmReasoningMode::ProviderDefault
    {
        return Vec::new();
    }

    let cached = cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode);
    if cached == Some(AutoReasoningStrategy::NoControls) {
        return Vec::new();
    }

    let current = applied_auto_reasoning_strategy(request_body).or(cached);
    let mut base = request_body.clone();
    strip_reasoning_controls(&mut base);

    auto_reasoning_fallback_strategies(uses_responses_api, mode)
        .iter()
        .copied()
        .filter(|strategy| Some(*strategy) != current)
        .map(|strategy| {
            let mut body = base.clone();
            apply_auto_reasoning_strategy(&mut body, strategy, mode);
            (strategy, body)
        })
        .collect()
}

pub fn apply_reasoning_controls(
    endpoint: &LlmEndpoint,
    uses_responses_api: bool,
    body: &mut serde_json::Value,
    mode: LlmReasoningMode,
) {
    let Some(kind) = reasoning_control_kind(endpoint, uses_responses_api) else {
        return;
    };

    match (kind, mode) {
        // Cerebras reasoning models: ProviderDefault maps to low (restoring the hardcoded behavior from before the refactor,
        // to avoid the server-side default reasoning effort being so high that TTFT and generation time rise noticeably)
        (ReasoningControlKind::CerebrasReasoningEffort, LlmReasoningMode::ProviderDefault) => {
            body["reasoning_effort"] = serde_json::json!("low");
        }
        (_, LlmReasoningMode::ProviderDefault) => {}
        (ReasoningControlKind::AnthropicThinking, _) => {
            if mode == LlmReasoningMode::Off {
                return;
            }

            let budget_tokens = match mode {
                LlmReasoningMode::Light => 1_024,
                LlmReasoningMode::Balanced => 2_048,
                LlmReasoningMode::Deep => 4_096,
                _ => 1_024,
            };
            body["thinking"] = serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget_tokens,
            });
        }
        (ReasoningControlKind::VolcengineThinkingType, _) => {
            let thinking_type = match mode {
                LlmReasoningMode::Off => "disabled",
                LlmReasoningMode::Light | LlmReasoningMode::Balanced => "auto",
                LlmReasoningMode::Deep => "enabled",
                LlmReasoningMode::ProviderDefault => return,
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        (ReasoningControlKind::DeepSeekThinkingToggle, _) => {
            if uses_responses_api {
                let effort = match mode {
                    LlmReasoningMode::Off => "none",
                    LlmReasoningMode::Light => "low",
                    LlmReasoningMode::Balanced => "medium",
                    LlmReasoningMode::Deep => "high",
                    LlmReasoningMode::ProviderDefault => return,
                };
                body["reasoning"] = serde_json::json!({ "effort": effort });
                return;
            }
            let thinking_type = if mode == LlmReasoningMode::Off {
                "disabled"
            } else {
                "enabled"
            };
            body["thinking"] = serde_json::json!({ "type": thinking_type });
        }
        (ReasoningControlKind::SiliconFlowThinkingBudget, _) => {
            let thinking_budget = match mode {
                LlmReasoningMode::Off => {
                    body["enable_thinking"] = serde_json::json!(false);
                    return;
                }
                LlmReasoningMode::Light => 1024,
                LlmReasoningMode::Balanced => 4096,
                LlmReasoningMode::Deep => 8192,
                LlmReasoningMode::ProviderDefault => return,
            };
            body["enable_thinking"] = serde_json::json!(true);
            body["thinking_budget"] = serde_json::json!(thinking_budget);
        }
        (ReasoningControlKind::CerebrasReasoningEffort, _) => {
            let effort = match mode {
                LlmReasoningMode::Off
                    if endpoint
                        .model
                        .trim()
                        .rsplit('/')
                        .next()
                        .is_some_and(|model| model.eq_ignore_ascii_case("qwen-3.8-27b")) =>
                {
                    "none"
                }
                LlmReasoningMode::Off => return,
                LlmReasoningMode::Light => "low",
                LlmReasoningMode::Balanced => "medium",
                LlmReasoningMode::Deep => "high",
                LlmReasoningMode::ProviderDefault => return,
            };
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        (ReasoningControlKind::CerebrasGlmToggle, _) => {
            body["disable_reasoning"] = serde_json::json!(mode == LlmReasoningMode::Off);
        }
        (ReasoningControlKind::OpenRouterReasoning, _) => {
            // OpenRouter unified entry: enabled=false turns thinking off, effort controls its intensity.
            let reasoning = match mode {
                LlmReasoningMode::ProviderDefault => return,
                LlmReasoningMode::Off => serde_json::json!({ "enabled": false }),
                LlmReasoningMode::Light => serde_json::json!({ "effort": "low" }),
                LlmReasoningMode::Balanced => serde_json::json!({ "effort": "medium" }),
                LlmReasoningMode::Deep => serde_json::json!({ "effort": "high" }),
            };
            body["reasoning"] = reasoning;
        }
        (ReasoningControlKind::AutoOpenaiCompat, _) => {
            let Some(strategy) = cached_auto_reasoning_strategy(endpoint, uses_responses_api, mode)
                .or_else(|| Some(preferred_auto_reasoning_strategy(uses_responses_api, mode)))
            else {
                return;
            };
            apply_auto_reasoning_strategy(body, strategy, mode);
        }
        (ReasoningControlKind::OpenaiEffort, _) => {
            let Some(effort) = openai_effort_for_mode(&endpoint.model, mode) else {
                return;
            };

            if uses_responses_api {
                body["reasoning"] = serde_json::json!({ "effort": effort });
            } else {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
        }
        (ReasoningControlKind::XaiReasoningEffort, _) => {
            let Some(effort) = grok_effort_for_mode(&endpoint.model, mode) else {
                return;
            };

            if uses_responses_api {
                body["reasoning"] = serde_json::json!({ "effort": effort });
            } else {
                body["reasoning_effort"] = serde_json::json!(effort);
            }
        }
    }
}

pub fn strip_reasoning_controls(body: &mut serde_json::Value) {
    if let Some(map) = body.as_object_mut() {
        map.remove("reasoning");
        map.remove("reasoning_effort");
        map.remove("thinking");
        map.remove("chat_template_kwargs");
        map.remove("thinking_budget");
        map.remove("enable_thinking");
        map.remove("disable_reasoning");
    }
}
