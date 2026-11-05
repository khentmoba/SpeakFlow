use super::*;

#[test]
fn current_builtin_defaults_apply_to_polish_assistant_and_preview() {
    for (provider, model, api_url) in [
        (OPENAI, "gpt-6-luna", "https://api.openai.com/v1/responses"),
        (XAI, "grok-4.7", "https://api.x.ai/v1/responses"),
        (
            CEREBRAS,
            "qwen-3.8-27b",
            "https://api.cerebras.ai/v1/chat/completions",
        ),
        (
            SILICONFLOW,
            "Qwen/Qwen3.8-27B",
            "https://api.siliconflow.cn/v1/chat/completions",
        ),
    ] {
        let config = LlmProviderConfig {
            active: provider.to_string(),
            assistant_use_separate_model: true,
            assistant_provider: Some(provider.to_string()),
            ..LlmProviderConfig::default()
        };

        for endpoint in [
            endpoint_for_config(&config),
            assistant_endpoint_for_config(&config),
            endpoint_for_preview(provider, None, None, ApiFormat::OpenaiCompat),
        ] {
            assert_eq!(endpoint.provider, provider);
            assert_eq!(endpoint.model, model, "provider={provider}");
            assert_eq!(endpoint.api_url, api_url, "provider={provider}");
            assert_eq!(endpoint.api_format, ApiFormat::OpenaiCompat);
        }
    }
}

#[test]
fn current_builtin_defaults_preserve_explicitly_saved_models() {
    for (provider, model) in [
        (OPENAI, "gpt-4.1-mini"),
        (XAI, "grok-4.6"),
        (CEREBRAS, "gpt-oss-20b"),
        (SILICONFLOW, "Qwen/Qwen3-32B"),
    ] {
        let config = LlmProviderConfig {
            active: provider.to_string(),
            custom_model: Some(model.to_string()),
            assistant_use_separate_model: true,
            assistant_provider: Some(provider.to_string()),
            assistant_model: Some(model.to_string()),
            ..LlmProviderConfig::default()
        };

        assert_eq!(endpoint_for_config(&config).model, model);
        assert_eq!(assistant_endpoint_for_config(&config).model, model);
        assert_eq!(
            endpoint_for_preview(provider, None, Some(model), ApiFormat::OpenaiCompat).model,
            model
        );
    }
}

#[test]
fn empty_custom_models_use_current_openai_default_on_chat_endpoints() {
    for (provider, base_url, api_url) in [
        (CUSTOM, None, "http://127.0.0.1:8000/v1/chat/completions"),
        (
            "custom-gateway",
            Some("https://gateway.example/custom/v1"),
            "https://gateway.example/custom/v1/chat/completions",
        ),
    ] {
        for model in [None, Some(" ")] {
            let endpoint = endpoint_for_preview(provider, base_url, model, ApiFormat::OpenaiCompat);

            assert_eq!(endpoint.provider, provider);
            assert_eq!(endpoint.model, "gpt-6-luna", "provider={provider}");
            assert_eq!(endpoint.api_url, api_url, "provider={provider}");
            assert!(!endpoint_uses_responses_api(&endpoint));
        }
    }
}

#[test]
fn deepseek_defaults_to_canonical_flash_for_polish_and_separate_assistant() {
    let polish_config = LlmProviderConfig {
        active: DEEPSEEK.to_string(),
        ..LlmProviderConfig::default()
    };
    let assistant_config = LlmProviderConfig {
        assistant_use_separate_model: true,
        assistant_provider: Some(DEEPSEEK.to_string()),
        ..LlmProviderConfig::default()
    };

    for endpoint in [
        endpoint_for_config(&polish_config),
        assistant_endpoint_for_config(&assistant_config),
    ] {
        assert_eq!(endpoint.provider, DEEPSEEK);
        assert_eq!(endpoint.model, "deepseek-flash");
        assert_eq!(endpoint.api_format, ApiFormat::OpenaiCompat);
    }
}

#[test]
fn deepseek_endpoints_preserve_explicitly_saved_models() {
    for model in [
        "deepseek-v4-flash",
        "deepseek-v4-pro",
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-future",
    ] {
        let config = LlmProviderConfig {
            active: DEEPSEEK.to_string(),
            custom_model: Some(model.to_string()),
            assistant_use_separate_model: true,
            assistant_provider: Some(DEEPSEEK.to_string()),
            assistant_model: Some(model.to_string()),
            ..LlmProviderConfig::default()
        };

        assert_eq!(endpoint_for_config(&config).model, model);
        assert_eq!(assistant_endpoint_for_config(&config).model, model);
    }
}

#[test]
fn named_presets_ignore_custom_endpoint_overrides() {
    let config = LlmProviderConfig {
        active: CEREBRAS.to_string(),
        custom_base_url: Some("https://example.com".to_string()),
        custom_model: Some("gpt-oss-20b".to_string()),
        reasoning_mode: Default::default(),
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
    };

    let endpoint = endpoint_for_config(&config);

    assert_eq!(endpoint.provider, CEREBRAS);
    assert_eq!(
        endpoint.api_url,
        "https://api.cerebras.ai/v1/chat/completions"
    );
    assert_eq!(endpoint.model, "gpt-oss-20b");
}

#[test]
fn named_presets_preserve_manual_model_override() {
    let config = LlmProviderConfig {
        active: CEREBRAS.to_string(),
        custom_base_url: None,
        custom_model: Some("openai/gpt-5.3-chat-latest".to_string()),
        reasoning_mode: Default::default(),
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
    };

    let endpoint = endpoint_for_config(&config);

    assert_eq!(endpoint.provider, CEREBRAS);
    assert_eq!(endpoint.model, "openai/gpt-5.3-chat-latest");
}

#[test]
fn custom_preset_keeps_custom_endpoint_and_model() {
    let config = LlmProviderConfig {
        active: CUSTOM.to_string(),
        custom_base_url: Some("https://example.com".to_string()),
        custom_model: Some("foo-model".to_string()),
        reasoning_mode: Default::default(),
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
    };

    let endpoint = endpoint_for_config(&config);

    assert_eq!(endpoint.provider, CUSTOM);
    assert_eq!(endpoint.api_url, "https://example.com/v1/chat/completions");
    assert_eq!(endpoint.model, "foo-model");
}

#[test]
fn wrapped_oauth_openai_api_key_builds_plain_bearer_header() {
    let wrapped =
        codex_oauth_service::encode_oauth_api_key("sk-oauth-session").expect("wrapped key");

    let headers = build_auth_headers(&ApiFormat::OpenaiCompat, &wrapped)
        .expect("headers should build for wrapped OAuth key");

    assert_eq!(
        headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer sk-oauth-session")
    );
    assert!(headers.get("ChatGPT-Account-ID").is_none());
    assert!(headers.get("originator").is_none());
    assert!(headers.get("User-Agent").is_none());
}

#[test]
fn invalid_active_provider_falls_back_to_latest_custom_provider() {
    let config = LlmProviderConfig {
        active: "custom_missing".to_string(),
        custom_base_url: None,
        custom_model: None,
        reasoning_mode: Default::default(),
        polish_reasoning_mode: None,
        assistant_reasoning_mode: None,
        assistant_use_separate_model: false,
        assistant_model: None,
        assistant_provider: None,
        selection_reasoning_mode: None,
        selection_use_separate_model: false,
        selection_model: None,
        selection_provider: None,
        custom_providers: vec![
            crate::state::user_profile::CustomProvider {
                id: "custom_a".to_string(),
                name: "A".to_string(),
                base_url: "https://a.example.com".to_string(),
                model: "model-a".to_string(),
                api_format: ApiFormat::OpenaiCompat,
            },
            crate::state::user_profile::CustomProvider {
                id: "custom_b".to_string(),
                name: "B".to_string(),
                base_url: "https://b.example.com".to_string(),
                model: "model-b".to_string(),
                api_format: ApiFormat::OpenaiCompat,
            },
        ],
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
    };

    let endpoint = endpoint_for_config(&config);

    assert_eq!(endpoint.provider, "custom_b");
    assert_eq!(
        endpoint.api_url,
        "https://b.example.com/v1/chat/completions"
    );
    assert_eq!(endpoint.model, "model-b");
}

#[test]
fn assistant_endpoint_uses_separate_model_for_builtin_provider() {
    let config = LlmProviderConfig {
        active: CEREBRAS.to_string(),
        custom_base_url: None,
        custom_model: Some("gpt-oss-120b".to_string()),
        reasoning_mode: Default::default(),
        polish_reasoning_mode: None,
        assistant_reasoning_mode: None,
        assistant_use_separate_model: true,
        assistant_model: Some("gpt-oss-20b".to_string()),
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
    };

    let endpoint = assistant_endpoint_for_config(&config);

    assert_eq!(endpoint.provider, CEREBRAS);
    assert_eq!(endpoint.model, "gpt-oss-20b");
    assert_eq!(
        endpoint.api_url,
        "https://api.cerebras.ai/v1/chat/completions"
    );
}

#[test]
fn assistant_endpoint_uses_separate_model_for_custom_provider() {
    let config = LlmProviderConfig {
        active: "custom_a".to_string(),
        custom_base_url: None,
        custom_model: None,
        reasoning_mode: Default::default(),
        polish_reasoning_mode: None,
        assistant_reasoning_mode: None,
        assistant_use_separate_model: true,
        assistant_model: Some("assistant-model".to_string()),
        assistant_provider: None,
        selection_reasoning_mode: None,
        selection_use_separate_model: false,
        selection_model: None,
        selection_provider: None,
        custom_providers: vec![crate::state::user_profile::CustomProvider {
            id: "custom_a".to_string(),
            name: "Custom A".to_string(),
            base_url: "https://example.com".to_string(),
            model: "polish-model".to_string(),
            api_format: ApiFormat::OpenaiCompat,
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
    };

    let endpoint = assistant_endpoint_for_config(&config);

    assert_eq!(endpoint.provider, "custom_a");
    assert_eq!(endpoint.model, "assistant-model");
    assert_eq!(endpoint.api_url, "https://example.com/v1/chat/completions");
}

#[test]
fn recognizes_image_unsupported_errors() {
    assert!(looks_like_image_input_unsupported_error(
        "API 返回错误 400: model does not support image input"
    ));
    assert!(looks_like_image_input_unsupported_error(
        "unsupported content type: input_image"
    ));
    assert!(looks_like_image_input_unsupported_error(
            "API 返回错误 400 Bad Request: Failed to deserialize the JSON body into the target type: messages[1]: unknown variant `image_url`, expected `text` at line 1 column 127501 (code: invalid_request_error)"
        ));
    assert!(!looks_like_image_input_unsupported_error(
        "API 返回错误 401: invalid api key"
    ));
}

#[test]
fn builds_cerebras_image_support_probe_url() {
    let endpoint = endpoint_for_preview(
        CUSTOM,
        Some("https://gateway.ai.cloudflare.com/v1/account/openai/compat"),
        Some("Qwen/Qwen3-32B"),
        ApiFormat::OpenaiCompat,
    );

    assert_eq!(
        image_support_probe_url(&endpoint).as_deref(),
        Some(
            "https://gateway.ai.cloudflare.com/v1/account/openai/compat/v1/models/Qwen%2FQwen3-32B"
        )
    );
}

#[test]
fn builds_cerebras_public_model_probe_url() {
    assert_eq!(
        cerebras_public_model_probe_url("Qwen/Qwen3-32B").as_deref(),
        Some("https://api.cerebras.ai/public/v1/models/Qwen%2FQwen3-32B?format=openrouter")
    );
}

#[test]
fn only_cerebras_like_endpoints_use_public_model_probe() {
    let openai_endpoint =
        endpoint_for_preview(OPENAI, None, Some("gpt-4.1"), ApiFormat::OpenaiCompat);
    let cerebras_endpoint = endpoint_for_preview(
        CEREBRAS,
        None,
        Some("gpt-oss-120b"),
        ApiFormat::OpenaiCompat,
    );

    assert!(!should_probe_cerebras_public_model_metadata(
        &openai_endpoint
    ));
    assert!(should_probe_cerebras_public_model_metadata(
        &cerebras_endpoint
    ));
}

#[test]
fn parses_text_only_model_metadata_as_no_image_support() {
    let payload = serde_json::json!({
        "id": "gpt-oss-120b",
        "input_modalities": ["text"]
    });

    assert_eq!(
        parse_image_input_support_from_model_metadata(&payload),
        Some(false)
    );
}

#[test]
fn parses_multimodal_model_metadata_as_image_supported() {
    let payload = serde_json::json!({
        "id": "vision-model",
        "input_modalities": ["text", "image"]
    });

    assert_eq!(
        parse_image_input_support_from_model_metadata(&payload),
        Some(true)
    );
}

#[test]
fn recognizes_json_output_unsupported_errors() {
    assert!(looks_like_json_output_unsupported_error(
            "API 返回错误 400 Bad Request: {\"error\":{\"code\":\"InvalidParameter\",\"message\":\"The parameter `response_format.type` specified in the request are not valid: `json_object` is not supported by this model.\",\"param\":\"response_format.type\",\"type\":\"BadRequest\"}}"
        ));
    assert!(looks_like_json_output_unsupported_error(
        "unsupported structured output: json schema is not supported"
    ));
    assert!(!looks_like_json_output_unsupported_error(
        "API 返回错误 401: invalid api key"
    ));
}

#[test]
fn recognizes_reasoning_unsupported_errors() {
    assert!(looks_like_reasoning_unsupported_error(
        "API 返回错误 400: The parameter `thinking.type` is not supported by this model"
    ));
    assert!(looks_like_reasoning_unsupported_error(
        "unknown parameter: reasoning_effort"
    ));
    assert!(looks_like_reasoning_unsupported_error(
        "Extra inputs are not permitted (param: chat_template_kwargs)"
    ));
    assert!(!looks_like_reasoning_unsupported_error(
        "API 返回错误 401: invalid api key"
    ));
}

#[test]
fn preview_endpoint_preserves_custom_provider_format() {
    let endpoint = endpoint_for_preview(
        "foo",
        Some("https://api.anthropic.com"),
        Some("claude-3-7-sonnet-latest"),
        ApiFormat::Anthropic,
    );

    assert_eq!(endpoint.api_format, ApiFormat::Anthropic);
    assert_eq!(endpoint.api_url, "https://api.anthropic.com/v1/messages");
}

#[test]
fn responses_api_detection_is_case_insensitive_and_exact() {
    let responses = endpoint_for_preview(
        "responses-case",
        Some("https://example.com/V1/RESPONSES"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );
    let responses_suffix = endpoint_for_preview(
        "responses-suffix",
        Some("https://example.com/v1/responses-extra"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );

    assert!(endpoint_uses_responses_api(&responses));
    assert!(!endpoint_uses_responses_api(&responses_suffix));
}

#[test]
fn volcan_seed_2_models_report_reasoning_support() {
    let endpoint = endpoint_for_preview(
        CUSTOM,
        Some("https://ark.cn-beijing.volces.com/api/v3"),
        Some("doubao-seed-2-0-mini-260215"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("volcengine_thinking_type")
    );
}

#[test]
fn volcengine_like_unknown_model_reports_auto_reasoning_support() {
    let endpoint = endpoint_for_preview(
        CUSTOM,
        Some("https://ark.cn-beijing.volces.com/api/v3"),
        Some("doubao-future-reasoner"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe")
    );
}

#[test]
fn openai_non_reasoning_models_report_unsupported() {
    let endpoint =
        endpoint_for_preview(OPENAI, None, Some("gpt-4.1-mini"), ApiFormat::OpenaiCompat);

    let support = reasoning_support(&endpoint, false);

    assert!(!support.supported);
    assert!(support.summary.contains("unavailable"));
}

#[test]
fn unknown_openai_compatible_reports_auto_reasoning_support() {
    let endpoint = endpoint_for_preview(
        "fau",
        Some("https://hub.nhr.fau.de/api/llmgw/v1"),
        Some("moonshotai/Kimi-K2.6"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe")
    );
}

#[test]
fn custom_gateway_path_containing_openai_host_still_uses_auto_reasoning() {
    let endpoint = endpoint_for_preview(
        "custom-gateway",
        Some("https://gateway.example/api.openai.com/v1"),
        Some("gpt-5.2"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe"),
        "only the actual URL host should identify the official OpenAI API"
    );
}

#[test]
fn openrouter_preset_defaults_to_chat_completions_endpoint() {
    let endpoint = endpoint_for_preview("openrouter", None, None, ApiFormat::OpenaiCompat);

    assert_eq!(
        endpoint.api_url,
        "https://openrouter.ai/api/v1/chat/completions"
    );
    assert_eq!(endpoint.model, "~deepseek/deepseek-flash-latest");
    assert_eq!(
        reasoning_support(&endpoint, false).strategy.as_deref(),
        Some("openrouter_reasoning")
    );
}

#[test]
fn openrouter_off_sends_reasoning_enabled_false() {
    let endpoint = endpoint_for_preview("openrouter", None, None, ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["reasoning"], serde_json::json!({ "enabled": false }));
    assert!(body.get("thinking").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn openrouter_tiers_map_to_reasoning_effort() {
    let endpoint = endpoint_for_preview("openrouter", None, None, ApiFormat::OpenaiCompat);

    for (mode, effort) in [
        (LlmReasoningMode::Light, "low"),
        (LlmReasoningMode::Balanced, "medium"),
        (LlmReasoningMode::Deep, "high"),
    ] {
        let mut body = serde_json::json!({});
        apply_reasoning_controls(&endpoint, false, &mut body, mode);
        assert_eq!(body["reasoning"], serde_json::json!({ "effort": effort }));
    }

    let mut default_body = serde_json::json!({});
    apply_reasoning_controls(
        &endpoint,
        false,
        &mut default_body,
        LlmReasoningMode::ProviderDefault,
    );
    assert!(default_body.get("reasoning").is_none());
}

#[test]
fn openrouter_custom_host_wins_over_model_name_heuristics() {
    // doubao/seed-* 会命中火山启发式；按 host 识别的 OpenRouter 必须优先。
    let endpoint = endpoint_for_preview(
        "custom-openrouter",
        Some("https://openrouter.ai/api"),
        Some("doubao-seed-2-0-pro"),
        ApiFormat::OpenaiCompat,
    );

    assert_eq!(
        reasoning_support(&endpoint, false).strategy.as_deref(),
        Some("openrouter_reasoning")
    );
}

#[test]
fn auto_openai_compatible_starts_with_chat_reasoning_effort() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_chat_start",
        Some("https://auto-reasoning-start.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

    assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
    assert!(body.get("thinking").is_none());
    assert!(body.get("chat_template_kwargs").is_none());
}

#[test]
fn auto_openai_compatible_off_starts_with_thinking_disabled() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_chat_off",
        Some("https://auto-reasoning-off.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["thinking"], serde_json::json!({ "type": "disabled" }));
    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("reasoning").is_none());
}

#[test]
fn auto_openai_compatible_fallbacks_try_one_standard_alternate() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_fallback",
        Some("https://auto-reasoning-fallback.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );
    let request_body = serde_json::json!({
        "model": "future-model",
        "messages": [],
        "reasoning_effort": "low",
    });

    let fallbacks =
        auto_reasoning_fallback_bodies(&endpoint, false, &request_body, LlmReasoningMode::Deep);

    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].0, AutoReasoningStrategy::TopLevelThinking);
    assert_eq!(
        fallbacks[0].1["thinking"],
        serde_json::json!({ "type": "enabled" })
    );
    assert!(fallbacks
        .iter()
        .all(|(_, body)| body.get("reasoning_effort").is_none()));
}

#[test]
fn remembered_auto_strategy_is_reused_for_endpoint() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_cached",
        Some("https://auto-reasoning-cache.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );

    remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Balanced,
        AutoReasoningStrategy::OpenaiResponsesReasoning,
    );

    let mut body = serde_json::json!({});
    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Balanced);

    assert_eq!(body["reasoning"], serde_json::json!({ "effort": "medium" }));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn remembered_no_controls_strategy_skips_future_auto_params() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_no_controls",
        Some("https://auto-reasoning-no-controls.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );

    remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let mut body = serde_json::json!({});
    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn auto_support_for_mode_reports_no_controls_cache_as_unsupported() {
    let endpoint = endpoint_for_preview(
        "auto_reasoning_no_controls_support",
        Some("https://auto-reasoning-no-controls-support.example/v1"),
        Some("future-model"),
        ApiFormat::OpenaiCompat,
    );

    remember_auto_reasoning_strategy(
        &endpoint,
        false,
        LlmReasoningMode::Deep,
        AutoReasoningStrategy::NoControls,
    );

    let default_support =
        reasoning_support_for_mode(&endpoint, false, LlmReasoningMode::ProviderDefault);
    let deep_support = reasoning_support_for_mode(&endpoint, false, LlmReasoningMode::Deep);

    assert!(default_support.supported);
    assert!(!deep_support.supported);
    assert_eq!(
        deep_support.strategy.as_deref(),
        Some("auto_no_reasoning_controls")
    );
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn reasoning_efforts_for_modes(endpoint: &LlmEndpoint) -> Vec<String> {
    [
        LlmReasoningMode::Off,
        LlmReasoningMode::Light,
        LlmReasoningMode::Balanced,
        LlmReasoningMode::Deep,
    ]
    .into_iter()
    .map(|mode| {
        let mut body = serde_json::json!({});
        apply_reasoning_controls(endpoint, true, &mut body, mode);
        body["reasoning"]["effort"]
            .as_str()
            .expect("reasoning.effort should be set")
            .to_string()
    })
    .collect()
}

#[test]
fn openai_gpt5_4_off_maps_reasoning_effort_to_none() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.4"), ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, true, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("none"));
}

#[test]
fn openai_gpt5_mini_keeps_minimal_family_mapping() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5-mini"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["minimal", "low", "medium", "high"])
    );
}

#[test]
fn openai_gpt5_2_off_maps_reasoning_effort_to_none() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.2"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["none", "low", "medium", "high"])
    );
}

#[test]
fn openai_gpt5_pro_uses_high_only() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5-pro"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["high", "high", "high", "high"])
    );
}

#[test]
fn openai_gpt5_2_codex_clamps_to_low_then_rises() {
    let endpoint =
        endpoint_for_preview(OPENAI, None, Some("gpt-5.2-codex"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["low", "medium", "high", "xhigh"])
    );
}

#[test]
fn openai_gpt5_5_reaches_xhigh() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-5.5"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["low", "medium", "high", "xhigh"])
    );
    assert_eq!(
        reasoning_support(&endpoint, true).strategy.as_deref(),
        Some("openai_reasoning_effort")
    );
}

#[test]
fn openai_gpt5_6_family_supports_off_through_deep() {
    for model in ["gpt-5.6", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
        let endpoint = endpoint_for_preview(OPENAI, None, Some(model), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["low", "medium", "high", "xhigh"]),
            "unexpected reasoning mapping for {model}"
        );
        assert_eq!(
            reasoning_support(&endpoint, true).strategy.as_deref(),
            Some("openai_reasoning_effort")
        );
    }
}

#[test]
fn openai_gpt6_astra_matches_gpt5_6_effort_mapping() {
    let endpoint = endpoint_for_preview(OPENAI, None, Some("gpt-6-astra"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["low", "medium", "high", "xhigh"])
    );
    assert_eq!(
        reasoning_support(&endpoint, true).strategy.as_deref(),
        Some("openai_reasoning_effort")
    );
}

#[test]
fn openai_gpt6_sol_and_luna_match_codex_effort_mapping() {
    for model in ["gpt-6-sol", "gpt-6.1-sol", "gpt-6-luna"] {
        let endpoint = endpoint_for_preview(OPENAI, None, Some(model), ApiFormat::OpenaiCompat);

        assert_eq!(
            reasoning_efforts_for_modes(&endpoint),
            strings(&["low", "medium", "high", "xhigh"]),
            "unexpected reasoning mapping for {model}"
        );
        assert_eq!(
            reasoning_support(&endpoint, true).strategy.as_deref(),
            Some("openai_reasoning_effort")
        );
    }
}

#[test]
fn openai_gpt5_1_codex_max_reaches_xhigh() {
    let endpoint = endpoint_for_preview(
        OPENAI,
        None,
        Some("gpt-5.1-codex-max"),
        ApiFormat::OpenaiCompat,
    );

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["none", "medium", "high", "xhigh"])
    );
}

#[test]
fn deepseek_reasoner_reports_reasoning_support() {
    let endpoint = endpoint_for_preview(
        DEEPSEEK,
        None,
        Some("deepseek-reasoner"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(support.strategy.as_deref(), Some("deepseek_thinking"));
}

#[test]
fn deepseek_flash_and_previous_models_report_reasoning_support() {
    for model in [
        "deepseek-flash",
        "deepseek-v4-flash",
        "deepseek-v4-pro",
        "deepseek-chat",
    ] {
        let endpoint = endpoint_for_preview(DEEPSEEK, None, Some(model), ApiFormat::OpenaiCompat);

        let support = reasoning_support(&endpoint, false);

        assert!(support.supported, "model={model}");
        assert_eq!(
            support.strategy.as_deref(),
            Some("deepseek_thinking"),
            "model={model}"
        );
    }
}

#[test]
fn known_openai_compatible_provider_unknown_model_reports_auto_reasoning_support() {
    let endpoint = endpoint_for_preview(
        DEEPSEEK,
        None,
        Some("deepseek-future-reasoner"),
        ApiFormat::OpenaiCompat,
    );

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("auto_openai_compat_probe")
    );
}

#[test]
fn known_openai_compatible_provider_unknown_model_starts_with_auto_probe() {
    let endpoint = endpoint_for_preview(
        SILICONFLOW,
        None,
        Some("Future/Reasoner-Next"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Deep);

    assert_eq!(body["reasoning_effort"], serde_json::json!("high"));
    assert!(body.get("enable_thinking").is_none());
    assert!(body.get("thinking_budget").is_none());
}

#[test]
fn deepseek_v4_flash_off_disables_thinking() {
    let endpoint = endpoint_for_preview(
        DEEPSEEK,
        None,
        Some("deepseek-v4-flash"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["thinking"]["type"], serde_json::json!("disabled"));
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn siliconflow_off_sends_enable_thinking_false() {
    let endpoint = endpoint_for_preview(
        SILICONFLOW,
        None,
        Some("Qwen/Qwen3-32B"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["enable_thinking"], serde_json::json!(false));
    assert!(body.get("thinking_budget").is_none());
}

#[test]
fn siliconflow_reasoning_budget_enables_thinking() {
    let endpoint = endpoint_for_preview(
        SILICONFLOW,
        None,
        Some("Qwen/Qwen3-32B"),
        ApiFormat::OpenaiCompat,
    );
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Balanced);

    assert_eq!(body["enable_thinking"], serde_json::json!(true));
    assert_eq!(body["thinking_budget"], serde_json::json!(4096));
}

#[test]
fn siliconflow_qwen_models_keep_native_thinking_controls() {
    for model in ["Qwen/Qwen3.8-27B", "Qwen/Qwen3-32B"] {
        let endpoint =
            endpoint_for_preview(SILICONFLOW, None, Some(model), ApiFormat::OpenaiCompat);
        let support = reasoning_support(&endpoint, false);

        assert!(support.supported, "model={model}");
        assert_eq!(
            support.strategy.as_deref(),
            Some("siliconflow_thinking_budget"),
            "model={model}"
        );
        for (mode, expected) in [
            (LlmReasoningMode::ProviderDefault, serde_json::json!({})),
            (
                LlmReasoningMode::Off,
                serde_json::json!({ "enable_thinking": false }),
            ),
            (
                LlmReasoningMode::Light,
                serde_json::json!({ "enable_thinking": true, "thinking_budget": 1024 }),
            ),
            (
                LlmReasoningMode::Balanced,
                serde_json::json!({ "enable_thinking": true, "thinking_budget": 4096 }),
            ),
            (
                LlmReasoningMode::Deep,
                serde_json::json!({ "enable_thinking": true, "thinking_budget": 8192 }),
            ),
        ] {
            let mut body = serde_json::json!({});
            apply_reasoning_controls(&endpoint, false, &mut body, mode);

            assert_eq!(body, expected, "model={model}, mode={mode:?}");
        }
    }
}

#[test]
fn cerebras_qwen_and_gpt_oss_keep_native_reasoning_modes() {
    for (model, expected) in [
        (
            "qwen-3.8-27b",
            serde_json::json!(["low", "none", "low", "medium", "high"]),
        ),
        (
            "gpt-oss-120b",
            serde_json::json!(["low", null, "low", "medium", "high"]),
        ),
    ] {
        let endpoint = endpoint_for_preview(CEREBRAS, None, Some(model), ApiFormat::OpenaiCompat);
        let support = reasoning_support(&endpoint, false);

        assert!(support.supported, "model={model}");
        assert_eq!(
            support.strategy.as_deref(),
            Some("cerebras_reasoning_effort"),
            "model={model}"
        );
        let efforts: Vec<Value> = [
            LlmReasoningMode::ProviderDefault,
            LlmReasoningMode::Off,
            LlmReasoningMode::Light,
            LlmReasoningMode::Balanced,
            LlmReasoningMode::Deep,
        ]
        .into_iter()
        .map(|mode| {
            let mut body = serde_json::json!({});
            apply_reasoning_controls(&endpoint, false, &mut body, mode);
            if model == "gpt-oss-120b" && mode == LlmReasoningMode::Off {
                assert!(body.get("reasoning_effort").is_none());
            }
            body["reasoning_effort"].clone()
        })
        .collect();

        assert_eq!(serde_json::json!(efforts), expected, "model={model}");
    }
}

#[test]
fn cerebras_glm_reports_reasoning_support() {
    let endpoint =
        endpoint_for_preview(CEREBRAS, None, Some("zai-glm-4.7"), ApiFormat::OpenaiCompat);

    let support = reasoning_support(&endpoint, false);

    assert!(support.supported);
    assert_eq!(
        support.strategy.as_deref(),
        Some("cerebras_disable_reasoning")
    );
}

#[test]
fn resolves_independent_screen_vision_endpoint() {
    let config = LlmProviderConfig {
        active: DEEPSEEK.to_string(),
        custom_model: Some("deepseek-v4-pro".to_string()),
        screen_vision_enabled: true,
        screen_vision_provider: Some(OPENAI.to_string()),
        screen_vision_model: Some("gpt-4.1-mini".to_string()),
        ..Default::default()
    };

    let endpoint = screen_vision_endpoint_for_config(&config).expect("vision endpoint");

    assert_eq!(endpoint.provider, OPENAI);
    assert_eq!(endpoint.model, "gpt-4.1-mini");
    assert_eq!(endpoint.api_url, "https://api.openai.com/v1/responses");
}

#[test]
fn disabled_screen_vision_has_no_endpoint() {
    let config = LlmProviderConfig {
        screen_vision_provider: Some(OPENAI.to_string()),
        screen_vision_model: Some("gpt-4.1-mini".to_string()),
        ..Default::default()
    };

    assert!(screen_vision_endpoint_for_config(&config).is_none());
}

#[test]
fn grok_4_6_reports_xai_reasoning_support_instead_of_auto_probe() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.6"), ApiFormat::OpenaiCompat);

    let support = reasoning_support(&endpoint, true);

    assert!(support.supported);
    assert_eq!(support.strategy.as_deref(), Some("xai_reasoning_effort"));
}

#[test]
fn grok_4_6_off_sends_reasoning_effort_low_on_responses() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.6"), ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, true, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("low"));
    assert!(body.get("thinking").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn grok_4_6_reasoning_modes_map_to_xai_efforts() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.6"), ApiFormat::OpenaiCompat);

    assert_eq!(
        reasoning_efforts_for_modes(&endpoint),
        strings(&["low", "low", "medium", "high"])
    );
}

#[test]
fn grok_4_6_chat_completions_use_top_level_reasoning_effort() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.6"), ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, false, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["reasoning_effort"], serde_json::json!("low"));
    assert!(body.get("reasoning").is_none());
    assert!(body.get("thinking").is_none());
}

#[test]
fn grok_4_3_off_can_disable_reasoning() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.3"), ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(&endpoint, true, &mut body, LlmReasoningMode::Off);

    assert_eq!(body["reasoning"]["effort"], serde_json::json!("none"));
}

#[test]
fn grok_4_6_provider_default_omits_effort() {
    let endpoint = endpoint_for_preview(XAI, None, Some("grok-4.6"), ApiFormat::OpenaiCompat);
    let mut body = serde_json::json!({});

    apply_reasoning_controls(
        &endpoint,
        true,
        &mut body,
        LlmReasoningMode::ProviderDefault,
    );

    assert!(body.get("reasoning").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

async fn probe_mock_image_metadata(
    provider: &str,
    model: &str,
    responses: Vec<(u16, String)>,
) -> (Vec<Option<bool>>, Vec<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("metadata server should bind");
    let address = listener.local_addr().expect("metadata server address");
    let response_count = responses.len();
    let mut server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut socket, _) = listener.accept().await.expect("metadata request");
            let mut request = Vec::new();
            loop {
                let mut chunk = [0_u8; 1024];
                let count = socket
                    .read(&mut chunk)
                    .await
                    .expect("read metadata request");
                assert!(count > 0, "metadata request must include complete headers");
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            requests.push(String::from_utf8(request).expect("HTTP request headers"));
            let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write metadata response");
        }
        requests
    });
    let mut endpoint = endpoint_for_preview(provider, None, Some(model), ApiFormat::OpenaiCompat);
    endpoint.api_url = format!("http://{address}/v1/chat/completions");
    endpoint.model = model.to_string();
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("local metadata client");
    let mut support = Vec::new();
    for _ in 0..response_count {
        support.push(
            probe_image_support_from_provider_metadata(
                &client,
                &endpoint,
                "FAKE_METADATA_TEST_KEY",
            )
            .await,
        );
    }
    let requests = match tokio::time::timeout(std::time::Duration::from_secs(1), &mut server).await
    {
        Ok(result) => result.expect("metadata server should complete"),
        Err(_) => {
            server.abort();
            let _ = server.await;
            Vec::new()
        }
    };
    (support, requests)
}

fn assert_image_metadata_requests(requests: &[String], path: &str, count: usize) {
    assert_eq!(
        requests.len(),
        count,
        "each probe must read upstream metadata"
    );
    for request in requests {
        assert_eq!(
            request.lines().next(),
            Some(format!("GET {path} HTTP/1.1").as_str())
        );
        let authorization = request.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim())
        });
        assert_eq!(authorization, Some("Bearer FAKE_METADATA_TEST_KEY"));
    }
}

#[tokio::test]
async fn deepseek_image_support_follows_metadata_for_legacy_current_and_future_models() {
    for (model, modality) in [
        ("deepseek-flash", "image"),
        ("deepseek-chat", "image"),
        ("deepseek-v4-pro", "input_image"),
        ("deepseek-future-vision", "image"),
    ] {
        let payload = serde_json::json!({
            "data": [{"id": model, "input_modalities": ["text", modality]}]
        });
        let (support, requests) =
            probe_mock_image_metadata(DEEPSEEK, model, vec![(200, payload.to_string())]).await;

        assert_eq!(support, vec![Some(true)], "model={model}");
        assert_image_metadata_requests(&requests, "/v1/models", 1);
    }
}

#[tokio::test]
async fn deepseek_image_support_selects_exact_model_and_follows_upstream_changes() {
    let responses = [
        serde_json::json!({"data": [
            {"id": "deepseek-v4-pro-preview", "input_modalities": ["text", "image"]},
            {"id": "deepseek-v4-pro", "input_modalities": ["text"]}
        ]}),
        serde_json::json!({"data": [
            {"id": "deepseek-v4-pro-preview", "input_modalities": ["text"]},
            {"id": "deepseek-v4-pro", "input_modalities": ["text", "image"]}
        ]}),
    ];
    let (support, requests) = probe_mock_image_metadata(
        DEEPSEEK,
        " deepseek-v4-pro ",
        responses
            .into_iter()
            .map(|payload| (200, payload.to_string()))
            .collect(),
    )
    .await;

    assert_eq!(support, vec![Some(false), Some(true)]);
    assert_image_metadata_requests(&requests, "/v1/models", 2);
}

#[tokio::test]
async fn deepseek_image_support_keeps_unknown_metadata_and_http_errors_unknown() {
    let model = "deepseek-v4-pro";
    let mut responses = [
        serde_json::json!({"data": [{"id": "another-model", "input_modalities": ["image"]}]}),
        serde_json::json!({"data": [{"id": model}]}),
        serde_json::json!({"data": [{"id": model, "input_modalities": []}]}),
        serde_json::json!({"data": [{"id": model, "input_modalities": "image"}]}),
        serde_json::json!({"data": [{"id": model, "input_modalities": ["audio"]}]}),
        serde_json::json!({"id": model, "input_modalities": ["image"]}),
    ]
    .into_iter()
    .map(|payload| (200, payload.to_string()))
    .collect::<Vec<_>>();
    responses.push((200, "not valid JSON".to_string()));
    responses.push((
        503,
        serde_json::json!({"data": [{"id": model, "input_modalities": ["image"]}]}).to_string(),
    ));
    let response_count = responses.len();
    let (support, requests) = probe_mock_image_metadata(DEEPSEEK, model, responses).await;

    assert_eq!(support, vec![None; response_count]);
    assert_image_metadata_requests(&requests, "/v1/models", response_count);
}

#[test]
fn deepseek_image_support_canonical_flash_body_preserves_responses_image_content() {
    use crate::services::llm_client::{
        build_llm_body, LlmImageInput, LlmRequestOptions, LlmUserInput,
    };

    let endpoint = endpoint_for_preview(
        DEEPSEEK,
        None,
        Some("deepseek-flash"),
        ApiFormat::OpenaiCompat,
    );
    let image_base64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9Zl1sAAAAASUVORK5CYII=";
    let input = LlmUserInput {
        text: "Describe the synthetic image".to_string(),
        images: vec![LlmImageInput {
            mime_type: "image/png".to_string(),
            data_base64: image_base64.to_string(),
        }],
    };
    let body = build_llm_body(&endpoint, "system", &input, LlmRequestOptions::default());
    let user = body["input"]
        .as_array()
        .expect("Responses request input")
        .iter()
        .find(|message| message["role"] == "user")
        .expect("Responses user message");
    let image = user["content"]
        .as_array()
        .expect("Responses user content")
        .iter()
        .find(|item| item["type"] == "input_image")
        .expect("Responses image input");

    assert_eq!(
        image["image_url"],
        format!("data:image/png;base64,{image_base64}")
    );
    assert!(body.get("messages").is_none());
}

#[tokio::test]
async fn custom_image_support_retains_model_detail_probe() {
    let payload = serde_json::json!({
        "id": "deepseek-v4-pro",
        "input_modalities": ["text", "image"]
    });
    let (support, requests) =
        probe_mock_image_metadata(CUSTOM, "deepseek-v4-pro", vec![(200, payload.to_string())])
            .await;

    assert_eq!(support, vec![Some(true)]);
    assert_image_metadata_requests(&requests, "/v1/models/deepseek-v4-pro", 1);
}

#[test]
fn magpie_points_at_the_local_gateway() {
    let config = LlmProviderConfig {
        active: "magpie".to_string(),
        custom_model: Some("claude/claude-haiku-5-5".to_string()),
        ..LlmProviderConfig::default()
    };

    let endpoint = endpoint_for_config(&config);
    assert_eq!(endpoint.provider, "magpie");
    assert_eq!(
        endpoint.api_url,
        "http://127.0.0.1:3425/v1/chat/completions"
    );
    assert_eq!(endpoint.model, "claude/claude-haiku-5-5");
    assert_eq!(endpoint.api_format, ApiFormat::OpenaiCompat);

    assert_eq!(
        models_url(&config, "magpie", None),
        "http://127.0.0.1:3425/v1/models"
    );
}

#[test]
fn magpie_requests_carry_the_gateway_bearer() {
    let headers = build_auth_headers(
        &ApiFormat::OpenaiCompat,
        crate::services::magpie_service::MAGPIE_GATEWAY_KEY,
    )
    .expect("headers build");

    assert_eq!(
        headers
            .get("Authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer magpie")
    );
}

/// End-to-end check against a running Magpie. Ignored because CI has no
/// gateway; run it on a machine with Magpie to confirm the wire contract.
#[tokio::test]
#[ignore = "needs a running Magpie gateway on 127.0.0.1:3425"]
async fn live_magpie_gateway_answers_the_built_request() {
    let config = LlmProviderConfig {
        active: "magpie".to_string(),
        custom_model: Some("claude/claude-haiku-5-5".to_string()),
        ..LlmProviderConfig::default()
    };
    let endpoint = endpoint_for_config(&config);
    let body = crate::services::llm_client::build_llm_body(
        &endpoint,
        "You reply with one short word.",
        &crate::services::llm_client::LlmUserInput::from("Reply with the single word: ok"),
        crate::services::llm_client::LlmRequestOptions::default(),
    );
    let answer = crate::services::llm_client::send_llm_request(
        &reqwest::Client::new(),
        &endpoint,
        crate::services::magpie_service::MAGPIE_GATEWAY_KEY,
        &body,
        32,
        None,
        crate::services::llm_client::LlmRequestOptions::default(),
    )
    .await
    .expect("magpie answered");
    assert!(!answer.trim().is_empty());
}
