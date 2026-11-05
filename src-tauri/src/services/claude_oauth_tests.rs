use super::claude_oauth_service::{
    decode_claude_oauth_access_token, effective_claude_auth_mode, encode_claude_oauth_access_token,
    is_claude_oauth_token, parse_pasted_code, session_from_json, CLAUDE_OAUTH_PREFIX,
};
use super::llm_provider::{build_auth_headers, default_endpoint_parts};
use crate::state::user_profile::{ApiFormat, ClaudeAuthMode};

#[test]
fn claude_oauth_token_encoding_roundtrips() {
    assert_eq!(CLAUDE_OAUTH_PREFIX, "claude-oauth:");

    assert!(encode_claude_oauth_access_token("").is_none());
    assert!(encode_claude_oauth_access_token("   ").is_none());

    let token = "sk-ant-oat01-sample";
    let encoded = encode_claude_oauth_access_token(token).expect("token encodes");
    assert_eq!(encoded, "claude-oauth:sk-ant-oat01-sample");

    assert_eq!(
        decode_claude_oauth_access_token(&encoded),
        Some(token.to_string())
    );
    assert_eq!(
        decode_claude_oauth_access_token(token),
        Some(token.to_string())
    );
    assert_eq!(decode_claude_oauth_access_token("sk-ant-api03-key"), None);
    assert_eq!(decode_claude_oauth_access_token(""), None);
}

#[test]
fn subscription_tokens_are_told_apart_from_api_keys() {
    assert!(is_claude_oauth_token("sk-ant-oat01-abc"));
    assert!(!is_claude_oauth_token("sk-ant-api03-abc"));
    assert!(!is_claude_oauth_token(""));
}

#[test]
fn claude_auth_headers_use_bearer_for_subscriptions_and_x_api_key_otherwise() {
    let subscription =
        build_auth_headers(&ApiFormat::Anthropic, "sk-ant-oat01-sample").expect("headers build");
    assert_eq!(
        subscription
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer sk-ant-oat01-sample")
    );
    assert!(subscription.get("x-api-key").is_none());
    assert_eq!(
        subscription
            .get("anthropic-version")
            .and_then(|value| value.to_str().ok()),
        Some("2023-06-01")
    );

    let api_key =
        build_auth_headers(&ApiFormat::Anthropic, "sk-ant-api03-key").expect("headers build");
    assert_eq!(
        api_key
            .get("x-api-key")
            .and_then(|value| value.to_str().ok()),
        Some("sk-ant-api03-key")
    );
    assert!(api_key.get("authorization").is_none());
}

#[test]
fn claude_endpoint_points_at_the_anthropic_messages_api() {
    let (base, model, _) = default_endpoint_parts("claude");
    assert_eq!(base, "https://api.anthropic.com");
    assert_eq!(model, "claude-sonnet-4-5");
}

#[test]
fn pasted_code_accepts_the_code_hash_state_form() {
    assert_eq!(
        parse_pasted_code("abc123#st", "st").expect("code parses"),
        "abc123"
    );
    assert_eq!(
        parse_pasted_code("  abc123  ", "st").expect("code parses"),
        "abc123"
    );
    assert_eq!(
        parse_pasted_code("abc123#", "st").expect("empty state is ignored"),
        "abc123"
    );
}

#[test]
fn pasted_code_rejects_empty_or_foreign_state() {
    assert!(parse_pasted_code("", "st").is_err());
    assert!(parse_pasted_code("   ", "st").is_err());
    assert!(parse_pasted_code("#st", "st").is_err());
    assert!(parse_pasted_code("abc123#other", "st").is_err());
}

#[test]
fn claude_auth_mode_prefers_the_stored_choice_then_the_connection() {
    assert_eq!(
        effective_claude_auth_mode(Some(ClaudeAuthMode::ApiKey), true),
        ClaudeAuthMode::ApiKey
    );
    assert_eq!(
        effective_claude_auth_mode(Some(ClaudeAuthMode::Oauth), false),
        ClaudeAuthMode::Oauth
    );
    assert_eq!(
        effective_claude_auth_mode(None, true),
        ClaudeAuthMode::Oauth
    );
    assert_eq!(
        effective_claude_auth_mode(None, false),
        ClaudeAuthMode::ApiKey
    );
}

#[test]
fn reads_claude_code_credential_file() {
    let json = serde_json::json!({
        "claudeAiOauth": {
            "accessToken": "sk-ant-oat01-access",
            "refreshToken": "sk-ant-ort01-refresh",
            "expiresAt": 1_792_000_000_000u64,
            "subscriptionType": "pro"
        }
    });
    let session = session_from_json(&json).expect("claude code credentials parse");
    assert_eq!(session.access_token, "sk-ant-oat01-access");
    assert_eq!(session.refresh_token, "sk-ant-ort01-refresh");
    assert_eq!(session.expires_at_ms, Some(1_792_000_000_000));
    assert_eq!(session.plan_type.as_deref(), Some("pro"));
}

#[test]
fn reads_magpie_accounts_and_skips_switched_off_ones() {
    let json = serde_json::json!([
        {
            "agent": "claude",
            "on": false,
            "user": "off@example.com",
            "auth": { "claudeAiOauth": { "accessToken": "sk-ant-oat01-off", "refreshToken": "r-off" } }
        },
        {
            "agent": "antigravity",
            "on": true,
            "auth": { "accessToken": "ya29.other", "refreshToken": "r-other" }
        },
        {
            "agent": "claude",
            "on": true,
            "auth": { "claudeAiOauth": { "accessToken": "sk-ant-oat01-on", "refreshToken": "r-on" } },
            "profile": { "emailAddress": "on@example.com", "organizationName": "Org" }
        }
    ]);
    let session = session_from_json(&json).expect("magpie account parses");
    assert_eq!(session.access_token, "sk-ant-oat01-on");
    assert_eq!(session.email.as_deref(), Some("on@example.com"));
    assert_eq!(session.organization_name.as_deref(), Some("Org"));
}

#[test]
fn ignores_entries_without_a_usable_token_pair() {
    let json = serde_json::json!([
        { "agent": "claude", "auth": { "claudeAiOauth": { "accessToken": "sk-ant-oat01-only" } } }
    ]);
    assert!(session_from_json(&json).is_none());
    assert!(session_from_json(&serde_json::json!({})).is_none());
}
