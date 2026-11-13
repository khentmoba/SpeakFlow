use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_ai_polish_request(
    state: &AppState,
    endpoint: &llm_provider::LlmEndpoint,
    api_key: &str,
    system_prompt: &str,
    user_input: &llm_client::LlmUserInput,
    user_content_len: usize,
    app_handle: Option<&tauri::AppHandle>,
    options: LlmRequestOptions<'_>,
) -> Result<String, llm_client::LlmError> {
    let body = llm_client::build_llm_body(endpoint, system_prompt, user_input, options);
    llm_client::send_llm_request(
        &state.http_client,
        endpoint,
        api_key,
        &body,
        user_content_len,
        app_handle,
        options,
    )
    .await
}

/// AI 润色的四段传输计划。
///
/// `prefer_streaming_after_partial` 只影响 stage1 已经吐出 chunk/token 后的后续顺序。
pub(crate) fn ai_polish_transport_plan(
    reasoning_mode: LlmReasoningMode,
    session_id: u64,
    prefer_streaming_after_partial: bool,
) -> [LlmRequestOptions<'static>; 4] {
    let stage1 = LlmRequestOptions {
        auth_context: None,
        stream: true,
        json_output: true,
        reasoning_mode,
        stream_event: Some("ai-polish-status"),
        session_id: Some(session_id),
        web_search: false,
        openai_fast_mode: false,
        stream_progress_timeout_secs: Some(llm_client::AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS),
        stream_total_timeout_secs: Some(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS),
    };
    let stream_nojson = LlmRequestOptions {
        auth_context: None,
        stream: true,
        json_output: false,
        reasoning_mode,
        stream_event: Some("ai-polish-status"),
        session_id: Some(session_id),
        web_search: false,
        openai_fast_mode: false,
        stream_progress_timeout_secs: Some(llm_client::AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS),
        stream_total_timeout_secs: Some(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS),
    };
    let nostream_json = LlmRequestOptions {
        auth_context: None,
        stream: false,
        json_output: true,
        reasoning_mode,
        stream_event: None,
        session_id: Some(session_id),
        web_search: false,
        openai_fast_mode: false,
        stream_progress_timeout_secs: Some(llm_client::AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS),
        stream_total_timeout_secs: Some(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS),
    };
    let nostream_nojson = LlmRequestOptions {
        auth_context: None,
        stream: false,
        json_output: false,
        reasoning_mode,
        stream_event: None,
        session_id: Some(session_id),
        web_search: false,
        openai_fast_mode: false,
        stream_progress_timeout_secs: Some(llm_client::AI_POLISH_STREAM_PROGRESS_TIMEOUT_SECS),
        stream_total_timeout_secs: Some(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS),
    };

    if prefer_streaming_after_partial {
        [stage1, stream_nojson, nostream_json, nostream_nojson]
    } else {
        [stage1, nostream_json, stream_nojson, nostream_nojson]
    }
}

pub(super) fn ai_polish_transport_label(options: &LlmRequestOptions<'_>) -> &'static str {
    match (options.stream, options.json_output) {
        (true, true) => "stream+json",
        (true, false) => "stream+nojson",
        (false, true) => "nostream+json",
        (false, false) => "nostream+nojson",
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_llm_request_with_transport_fallback(
    state: &AppState,
    endpoint: &llm_provider::LlmEndpoint,
    api_key: &str,
    system_prompt: &str,
    user_input: &llm_client::LlmUserInput,
    user_content_len: usize,
    app_handle: &tauri::AppHandle,
    session_id: u64,
    emit_status: bool,
) -> Result<String, String> {
    let (reasoning_mode, fast_mode) = state.with_profile(|profile| {
        (
            profile.llm_provider.polish_reasoning_mode(),
            profile.llm_provider.openai_fast_mode,
        )
    });
    let apply_fast = |mut stage: LlmRequestOptions<'static>| {
        stage.openai_fast_mode = fast_mode;
        if !emit_status {
            stage.stream_event = None;
        }
        LlmRequestOptions {
            auth_context: Some((app_handle, state)),
            ..stage
        }
    };
    let _ = state.take_ai_polish_stream_started(session_id);
    let [stage1, _, _, _] = ai_polish_transport_plan(reasoning_mode, session_id, false);
    let stage1 = apply_fast(stage1);
    match send_ai_polish_request(
        state,
        endpoint,
        api_key,
        system_prompt,
        user_input,
        user_content_len,
        emit_status.then_some(app_handle),
        stage1,
    )
    .await
    .map_err(|err| (err.is_terminal(), String::from(err)))
    {
        Ok(content) => {
            let _ = state.take_ai_polish_stream_started(session_id);
            return Ok(content);
        }
        Err((terminal, err)) => {
            // 空响应是模型/prompt 层面的问题，换 transport 大概率仍然回空——
            // 直接抛错，省下 3 次 LLM 请求；用户能从错误里看到 provider/model
            // 诊断信息。
            if terminal || llm_client::is_empty_llm_response_error(&err) {
                let _ = state.take_ai_polish_stream_started(session_id);
                log::warn!(
                    "AI 润色 {} 遇到不可通过传输回退恢复的错误: {}",
                    ai_polish_transport_label(&stage1),
                    err
                );
                return Err(err);
            }
            log::warn!(
                "AI 润色 {} 失败: {}",
                ai_polish_transport_label(&stage1),
                err
            );
            if emit_status {
                emit_polish_status(app_handle, "fallback", "", "", &err, session_id);
            }
        }
    }

    let prefer_streaming_after_partial = state.take_ai_polish_stream_started(session_id);
    let [_, stage2, stage3, stage4] =
        ai_polish_transport_plan(reasoning_mode, session_id, prefer_streaming_after_partial);
    for (index, stage) in [stage2, stage3, stage4]
        .into_iter()
        .map(apply_fast)
        .enumerate()
    {
        let is_last_stage = index == 2;
        let stage_label = ai_polish_transport_label(&stage);
        match send_ai_polish_request(
            state,
            endpoint,
            api_key,
            system_prompt,
            user_input,
            user_content_len,
            if stage.stream && emit_status {
                Some(app_handle)
            } else {
                None
            },
            stage,
        )
        .await
        .map_err(|err| (err.is_terminal(), String::from(err)))
        {
            Ok(content) => {
                let _ = state.take_ai_polish_stream_started(session_id);
                return Ok(content);
            }
            Err((terminal, err)) if terminal || llm_client::is_empty_llm_response_error(&err) => {
                let _ = state.take_ai_polish_stream_started(session_id);
                log::warn!(
                    "AI 润色 {} 遇到不可通过传输回退恢复的错误，停止剩余 fallback: {}",
                    stage_label,
                    err
                );
                return Err(err);
            }
            Err((_, err)) if stage.json_output => {
                if !llm_provider::looks_like_json_output_unsupported_error(&err) {
                    let _ = state.take_ai_polish_stream_started(session_id);
                    return Err(format!("AI polish {} failed: {}", stage_label, err));
                }

                log::warn!("AI 润色确认 JSON 格式不被支持，降级到 prompt 约束: {}", err);
                if emit_status {
                    emit_polish_status(
                        app_handle,
                        "fallback",
                        "",
                        "",
                        "The current model does not support response_format; retrying with prompt-constrained output",
                        session_id,
                    );
                }
                if is_last_stage {
                    let _ = state.take_ai_polish_stream_started(session_id);
                    return Err(format!("All transport strategies failed: {}", err));
                }
            }
            Err((_, err)) => {
                if is_last_stage {
                    let _ = state.take_ai_polish_stream_started(session_id);
                    return Err(format!("All transport strategies failed: {}", err));
                }

                log::warn!("AI 润色 {} 失败，继续回退: {}", stage_label, err);
                if emit_status {
                    emit_polish_status(app_handle, "fallback", "", "", &err, session_id);
                }
            }
        }
    }

    unreachable!("transport fallback plan must always end in a return")
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn send_llm_request_with_fallback(
    state: &AppState,
    endpoint: &llm_provider::LlmEndpoint,
    api_key: &str,
    system_prompt: &str,
    user_input: &llm_client::LlmUserInput,
    user_content_len: usize,
    app_handle: &tauri::AppHandle,
    session_id: u64,
    emit_status: bool,
) -> Result<String, String> {
    let cache_key = llm_provider::image_support_cache_key(endpoint);
    match send_llm_request_with_transport_fallback(
        state,
        endpoint,
        api_key,
        system_prompt,
        user_input,
        user_content_len,
        app_handle,
        session_id,
        emit_status,
    )
    .await
    {
        Ok(content) => {
            if !user_input.images.is_empty() {
                state.set_assistant_image_support(cache_key, true);
            }
            Ok(content)
        }
        Err(err)
            if !user_input.images.is_empty()
                && llm_provider::looks_like_image_input_unsupported_error(&err) =>
        {
            log::warn!(
                "当前 AI 润色模型不支持图片输入，回退到纯文本请求: provider={}, model={}, err={}",
                endpoint.provider,
                endpoint.model,
                err
            );
            state.set_assistant_image_support(cache_key, false);
            if emit_status {
                emit_polish_status(
                    app_handle,
                    "fallback",
                    "",
                    "",
                    "The current model does not support image input; retrying with text only",
                    session_id,
                );
            }

            let fallback_text =
                match screen_vision_service::describe_images(state, app_handle, &user_input.images)
                    .await
                {
                    Ok(description) => {
                        log::info!("AI 润色已通过视觉代理保留屏幕上下文");
                        screen_vision_service::with_screen_description(
                            &user_input.text,
                            &description,
                        )
                    }
                    Err(vision_err) => {
                        log::warn!(
                            "AI 润色视觉代理不可用，继续无屏幕上下文请求: {}",
                            vision_err
                        );
                        screen_vision_service::without_screen_context(&user_input.text)
                    }
                };
            let fallback_input = llm_client::LlmUserInput {
                text: fallback_text,
                images: Vec::new(),
            };
            send_llm_request_with_transport_fallback(
                state,
                endpoint,
                api_key,
                system_prompt,
                &fallback_input,
                user_content_len,
                app_handle,
                session_id,
                emit_status,
            )
            .await
        }
        Err(err) => Err(err),
    }
}
