use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;
use tauri_plugin_keyring::KeyringExt;

use crate::services::codex_oauth_service;
use crate::services::grok_build_oauth_service;
use crate::state::user_profile::{ApiFormat, CustomProvider, LlmProviderConfig, LlmReasoningMode};

mod endpoint;
mod reasoning;

pub use endpoint::*;
pub use reasoning::*;

/// Get the model list URL
pub fn models_url(config: &LlmProviderConfig, provider: &str, base_url: Option<&str>) -> String {
    if !is_preset(provider) {
        if let Some(cp) = config.custom_providers.iter().find(|p| p.id == provider) {
            let effective_url = base_url.unwrap_or(&cp.base_url);
            if cp.api_format == ApiFormat::Anthropic {
                return normalize_anthropic_models_url(effective_url);
            }
            return normalize_models_url(Some(effective_url), &cp.base_url);
        }
    }
    let (default_base_url, _, _) = default_endpoint_parts(provider);
    if provider == CUSTOM {
        normalize_models_url(base_url, default_base_url)
    } else {
        normalize_models_url(None, default_base_url)
    }
}

/// Normalize the Anthropic model list URL (`/v1/models`)
fn normalize_anthropic_models_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://api.anthropic.com/v1/models".to_string();
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("/v1/models") {
        return trimmed.to_string();
    }
    if lower.ends_with("/v1/messages") {
        return format!(
            "{}/models",
            trimmed[..trimmed.len() - "/messages".len()].trim_end_matches('/')
        );
    }
    if lower.ends_with("/v1") {
        return format!("{trimmed}/models");
    }
    format!("{trimmed}/v1/models")
}

/// Get the keyring user name (each backend stores its API key separately)
pub fn keyring_user_for_provider(provider: &str) -> String {
    match provider {
        OPENAI => "openai-api-key".to_string(),
        XAI => "xai-api-key".to_string(),
        DEEPSEEK => "deepseek-api-key".to_string(),
        SILICONFLOW => "siliconflow-api-key".to_string(),
        OPENROUTER => "openrouter-api-key".to_string(),
        CLAUDE => "claude-api-key".to_string(),
        MAGPIE => "magpie-api-key".to_string(),
        CUSTOM => "custom-api-key".to_string(),
        CEREBRAS => "cerebras-api-key".to_string(),
        id => format!("custom-{id}-api-key"),
    }
}

/// Build auth headers (branching on api_format)
pub fn build_auth_headers(
    api_format: &ApiFormat,
    api_key: &str,
) -> Result<reqwest::header::HeaderMap, String> {
    let api_key = api_key.trim();
    let mut headers = reqwest::header::HeaderMap::new();
    let parse = |v: &str| {
        v.parse::<reqwest::header::HeaderValue>().map_err(|_| {
            "API Key contains invalid characters and cannot be used as an HTTP header".to_string()
        })
    };
    match api_format {
        ApiFormat::Anthropic => {
            // A Claude subscription token authenticates with a bearer token; a
            // plain Anthropic API key uses x-api-key.
            match crate::services::claude_oauth_service::decode_claude_oauth_access_token(api_key) {
                Some(token) => {
                    headers.insert("authorization", parse(&format!("Bearer {token}"))?);
                }
                None => {
                    headers.insert("x-api-key", parse(api_key)?);
                }
            }
            headers.insert("anthropic-version", parse("2023-06-01")?);
            headers.insert("content-type", parse("application/json")?);
        }
        ApiFormat::OpenaiCompat => {
            if let Some(token) = codex_oauth_service::decode_chatgpt_bearer_token(api_key) {
                headers.insert(
                    "Authorization",
                    parse(&format!("Bearer {}", token.access_token))?,
                );
                if !codex_oauth_service::is_chatgpt_plan_auth(api_key) {
                    if let Some(account_id) = token
                        .account_id
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                    {
                        headers.insert("ChatGPT-Account-ID", parse(account_id)?);
                    }
                    headers.insert("originator", parse(codex_oauth_service::ORIGINATOR)?);
                    headers.insert(
                        "User-Agent",
                        parse(codex_oauth_service::CHATGPT_BEARER_USER_AGENT)?,
                    );
                }
            } else if let Some(token) =
                grok_build_oauth_service::decode_grok_build_oauth_access_token(api_key)
            {
                return grok_build_oauth_service::grok_cli_request_headers(&token);
            } else {
                let bearer_api_key = codex_oauth_service::decode_oauth_api_key(api_key)
                    .unwrap_or_else(|| api_key.to_string());
                headers.insert("Authorization", parse(&format!("Bearer {bearer_api_key}"))?);
            }
            headers.insert("Content-Type", parse("application/json")?);
        }
    }
    Ok(headers)
}

/// Save or delete an API key: write to the keyring if non-empty, delete if empty
pub fn save_or_delete_api_key<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    keyring_user: &str,
    api_key: &str,
) -> Result<(), String> {
    if !api_key.is_empty() {
        keyring_write_result(app_handle.keyring().set_password(
            KEYRING_SERVICE,
            keyring_user,
            api_key,
        ))
    } else {
        keyring_delete_result(
            app_handle
                .keyring()
                .delete_password(KEYRING_SERVICE, keyring_user),
        )
    }
}

fn keyring_write_result(result: keyring::Result<()>) -> Result<(), String> {
    result.map_err(|_| "Failed to save API Key to system keyring".to_string())
}

pub(crate) fn keyring_delete_result(result: keyring::Result<()>) -> Result<(), String> {
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Failed to delete API Key from system keyring".to_string()),
    }
}

#[cfg(test)]
mod credential_persistence_tests {
    use super::{keyring_delete_result, keyring_write_result};

    #[test]
    fn failed_key_write_is_reported_without_exposing_payload() {
        let result = keyring_write_result(Err(keyring::Error::BadEncoding(
            b"FAKE_TEST_VALUE".to_vec(),
        )));
        let error = result.expect_err("credential storage failure must reach the caller");
        assert!(!error.contains("FAKE_TEST_VALUE"));
        assert!(keyring_write_result(Ok(())).is_ok());
    }

    #[test]
    fn missing_key_delete_is_idempotent_but_real_failures_are_reported() {
        assert!(keyring_delete_result(Err(keyring::Error::NoEntry)).is_ok());
        assert!(keyring_delete_result(Ok(())).is_ok());
        assert!(keyring_delete_result(Err(keyring::Error::BadEncoding(vec![0xff]))).is_err());
    }
}

pub fn load_api_key_for_provider<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    provider: &str,
) -> String {
    let keyring_user = keyring_user_for_provider(provider);
    app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, &keyring_user)
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
