use std::time::{Duration, Instant};

use base64::Engine;
use rand::{rngs::OsRng, Rng};
use serde::{Deserialize, Serialize};
use tauri_plugin_keyring::KeyringExt;
#[cfg(test)]
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[cfg(test)]
use super::oauth_callback::base64_url_encode;
use super::oauth_callback::{
    callback_html, decode_jwt_claims, delete_keyring_password, deserialize_interval_secs,
    generate_pkce_pair, generate_state, now_ms, respond_with_html,
};
use crate::services::claude_oauth_service;
use crate::services::grok_build_oauth_service;
use crate::services::llm_provider::KEYRING_SERVICE;
use crate::services::magpie_service;
use crate::state::oauth_session::OAuthOperation;
use crate::state::user_profile::{ClaudeAuthMode, LlmProviderConfig, OpenaiAuthMode, XaiAuthMode};
use crate::state::AppState;
use crate::utils::paths;

#[path = "chatgpt_sign_in.rs"]
mod chatgpt_sign_in;
pub use chatgpt_sign_in::{ChatgptAccount, ChatgptRegistration};

const OPENAI_PROVIDER: &str = "openai";
const XAI_PROVIDER: &str = "xai";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
pub const ORIGINATOR: &str = "codex_cli_rs";
pub const CHATGPT_BEARER_USER_AGENT: &str = "codex-cli";
pub const CHATGPT_CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const CHATGPT_CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const DEFAULT_CALLBACK_PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
const SESSION_KEYRING_USER: &str = "openai-codex-oauth";
const SESSION_REFRESH_TOKEN_KEYRING_USER: &str = "openai-codex-oauth-refresh-token";
const OAUTH_TIMEOUT_SECS: u64 = 5 * 60;
const REFRESH_SKEW_SECS: u64 = 60;
// Bound headers and response-body reads while login/logout wait for refresh.
const TOKEN_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const CHATGPT_BEARER_PREFIX: &str = "openai-codex-chatgpt:";
const CHATGPT_PLAN_PREFIX: &str = "openai-chatgpt-plan:";
const OAUTH_API_KEY_PREFIX: &str = "openai-codex-oauth-api-key:";
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenaiCodexOauthSession {
    #[serde(default)]
    pub registration: Option<ChatgptRegistration>,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub api_key: String,
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenaiCodexOauthStatus {
    pub client_id: Option<String>,
    pub plan_usage_enabled: bool,
    pub saved_accounts: Vec<ChatgptAccount>,
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan_type: Option<String>,
    pub account_id: Option<String>,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenaiCodexOauthDeviceCodeChallenge {
    pub verification_url: String,
    pub user_code: String,
    pub device_auth_id: String,
    pub interval_secs: u64,
}

fn device_challenge_binding(challenge: &OpenaiCodexOauthDeviceCodeChallenge) -> String {
    format!("{}\0{}", challenge.device_auth_id, challenge.user_code)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DeviceCodeRequest {
    client_id: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(default, deserialize_with = "deserialize_interval_secs")]
    interval: u64,
}

#[derive(Debug, Serialize)]
struct DeviceCodePollRequest {
    device_auth_id: String,
    user_code: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeAuthorizationResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize, Default)]
struct JwtClaims {
    #[serde(default)]
    exp: Option<u64>,
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/profile", default)]
    profile: Option<ProfileClaims>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<AuthClaims>,
}

#[derive(Debug, Deserialize, Default)]
struct ProfileClaims {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AuthClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
}

#[derive(Debug)]
struct OAuthCallback {
    code: String,
    client_id: Option<String>,
    stream: tokio::net::TcpStream,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatgptBearerToken {
    pub access_token: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedOpenaiCodexOauthSession {
    #[serde(default)]
    registration: Option<ChatgptRegistration>,
    #[serde(default)]
    pub logged_out: bool,
    pub expires_at_ms: Option<u64>,
    pub account_id: Option<String>,
    pub email: Option<String>,
    pub plan_type: Option<String>,
}

struct CallbackListeners {
    ipv4: TcpListener,
    ipv6: Option<TcpListener>,
    port: u16,
}

fn make_status(session: Option<&OpenaiCodexOauthSession>) -> OpenaiCodexOauthStatus {
    if let Some(session) = session {
        return OpenaiCodexOauthStatus {
            client_id: session.registration.as_ref().map(|r| r.client_id.clone()),
            plan_usage_enabled: session
                .registration
                .as_ref()
                .is_none_or(|r| r.plan_usage_enabled()),
            saved_accounts: chatgpt_sign_in::saved_accounts(),
            logged_in: true,
            email: session.email.clone(),
            plan_type: session.plan_type.clone(),
            account_id: session.account_id.clone(),
            expires_at_ms: session.expires_at_ms,
        };
    }

    OpenaiCodexOauthStatus {
        saved_accounts: chatgpt_sign_in::saved_accounts(),
        ..Default::default()
    }
}

fn session_meta_path() -> std::path::PathBuf {
    paths::get_data_dir().join("openai_codex_oauth_session.json")
}

fn read_session_meta_at(
    path: &std::path::Path,
) -> Result<Option<PersistedOpenaiCodexOauthSession>, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => {
            let value: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|_| "Failed to read Codex OAuth metadata".to_string())?;
            let object = value
                .as_object()
                .ok_or_else(|| "Failed to read Codex OAuth metadata".to_string())?;
            if !object.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "logged_out" | "expires_at_ms" | "account_id" | "email" | "plan_type"
                )
            }) {
                return Err("Failed to read Codex OAuth metadata".to_string());
            }
            serde_json::from_value(value)
                .map(Some)
                .map_err(|_| "Failed to read Codex OAuth metadata".to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Failed to read Codex OAuth metadata".to_string()),
    }
}

fn read_session_meta() -> Result<Option<PersistedOpenaiCodexOauthSession>, String> {
    read_session_meta_at(&session_meta_path())
}

fn write_metadata_at(
    path: &std::path::Path,
    metadata: &PersistedOpenaiCodexOauthSession,
) -> Result<(), String> {
    let raw = serde_json::to_vec(metadata)
        .map_err(|_| "Failed to serialize Codex OAuth metadata".to_string())?;
    paths::atomic_write(path, &raw).map_err(|_| "Failed to save Codex OAuth metadata".to_string())
}

fn write_session_meta(session: &OpenaiCodexOauthSession) -> Result<(), String> {
    let persisted = PersistedOpenaiCodexOauthSession {
        registration: session.registration.clone(),
        logged_out: false,
        expires_at_ms: session.expires_at_ms,
        account_id: session.account_id.clone(),
        email: session.email.clone(),
        plan_type: session.plan_type.clone(),
    };
    write_metadata_at(&session_meta_path(), &persisted)
}

fn write_logout_tombstone() -> Result<(), String> {
    let metadata = PersistedOpenaiCodexOauthSession {
        logged_out: true,
        ..Default::default()
    };
    write_metadata_at(&session_meta_path(), &metadata)
}

fn load_session_from_storage(app_handle: &tauri::AppHandle) -> Option<OpenaiCodexOauthSession> {
    let meta = match read_session_meta() {
        Ok(meta) => meta,
        Err(_) => {
            log::warn!("Ignoring unreadable Codex OAuth metadata");
            return None;
        }
    };
    if meta.as_ref().is_some_and(|meta| meta.logged_out) {
        return None;
    }
    let refresh_token = app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_REFRESH_TOKEN_KEYRING_USER)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty());

    if let (Some(meta), Some(refresh_token)) = (meta.as_ref(), refresh_token) {
        return Some(OpenaiCodexOauthSession {
            registration: meta.registration.clone(),
            id_token: String::new(),
            access_token: String::new(),
            refresh_token,
            api_key: String::new(),
            expires_at_ms: meta.expires_at_ms,
            account_id: meta.account_id.clone(),
            email: meta.email.clone(),
            plan_type: meta.plan_type.clone(),
        });
    }

    app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_KEYRING_USER)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<OpenaiCodexOauthSession>(&raw).ok())
        .filter(|session| {
            !session.id_token.trim().is_empty()
                && !session.access_token.trim().is_empty()
                && !session.refresh_token.trim().is_empty()
        })
}

fn save_session_to_storage(
    app_handle: &tauri::AppHandle,
    session: &OpenaiCodexOauthSession,
) -> Result<(), String> {
    write_logout_tombstone()?;
    app_handle
        .keyring()
        .set_password(
            KEYRING_SERVICE,
            SESSION_REFRESH_TOKEN_KEYRING_USER,
            &session.refresh_token,
        )
        .map_err(|_| "Failed to save Codex OAuth refresh token".to_string())?;
    delete_keyring_password(
        app_handle,
        SESSION_KEYRING_USER,
        "delete Codex OAuth legacy session",
    )?;
    if let Some(registration) = &session.registration {
        chatgpt_sign_in::remember_account(registration)?;
    }
    // Commit valid metadata last; any earlier failure leaves the intent marker.
    write_session_meta(session)
}

fn clear_session_from_storage(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(error) = write_logout_tombstone() {
        errors.push(error);
    }
    if let Err(error) = delete_keyring_password(
        app_handle,
        SESSION_KEYRING_USER,
        "delete Codex OAuth legacy session",
    ) {
        errors.push(error);
    }
    if let Err(error) = delete_keyring_password(
        app_handle,
        SESSION_REFRESH_TOKEN_KEYRING_USER,
        "delete Codex OAuth refresh token",
    ) {
        errors.push(error);
    }
    if errors.is_empty() {
        match std::fs::remove_file(session_meta_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => errors.push("Failed to delete Codex OAuth metadata".to_string()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn enrich_session_from_tokens(
    session: &mut OpenaiCodexOauthSession,
    id_token_fallback: Option<&str>,
) {
    let claims = decode_jwt_claims::<JwtClaims>(&session.id_token)
        .or_else(|| id_token_fallback.and_then(decode_jwt_claims::<JwtClaims>))
        .or_else(|| decode_jwt_claims::<JwtClaims>(&session.access_token));

    if let Some(claims) = claims {
        session.email = claims
            .email
            .or_else(|| claims.profile.and_then(|profile| profile.email));
        if let Some(auth) = claims.auth {
            session.account_id = auth
                .chatgpt_account_id
                .or_else(|| session.account_id.take());
            session.plan_type = auth.chatgpt_plan_type.or_else(|| session.plan_type.take());
        }
        if session.expires_at_ms.is_none() {
            session.expires_at_ms = claims.exp.map(|exp| exp.saturating_mul(1000));
        }
    }
}

pub(crate) fn encode_chatgpt_bearer_token(token: &ChatgptBearerToken) -> Option<String> {
    let raw = serde_json::to_vec(token).ok()?;
    Some(format!(
        "{CHATGPT_BEARER_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
    ))
}

pub fn decode_chatgpt_bearer_token(input: &str) -> Option<ChatgptBearerToken> {
    let input = input.trim();
    let payload = input
        .strip_prefix(CHATGPT_BEARER_PREFIX)
        .or_else(|| input.strip_prefix(CHATGPT_PLAN_PREFIX))?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<ChatgptBearerToken>(&raw).ok()
}

pub fn is_chatgpt_plan_auth(input: &str) -> bool {
    input.trim().starts_with(CHATGPT_PLAN_PREFIX) && decode_chatgpt_bearer_token(input).is_some()
}

pub fn encode_oauth_api_key(api_key: &str) -> Option<String> {
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return None;
    }

    Some(format!("{OAUTH_API_KEY_PREFIX}{api_key}"))
}

pub fn decode_oauth_api_key(input: &str) -> Option<String> {
    let payload = input.trim().strip_prefix(OAUTH_API_KEY_PREFIX)?;
    if payload.trim().is_empty() {
        return None;
    }
    Some(payload.to_string())
}

pub fn is_oauth_origin_auth(input: &str) -> bool {
    decode_chatgpt_bearer_token(input).is_some() || decode_oauth_api_key(input).is_some()
}

pub struct ResolvedProviderAuth {
    pub api_key: String,
    pub chatgpt_token: Option<ChatgptBearerToken>,
}

impl ResolvedProviderAuth {
    fn api_key(api_key: String) -> Self {
        Self {
            api_key,
            chatgpt_token: None,
        }
    }
}

fn auth_from_openai_session(
    session: OpenaiCodexOauthSession,
) -> Result<ResolvedProviderAuth, String> {
    let access_token = session.access_token.trim();
    let chatgpt_token = (!access_token.is_empty()).then(|| ChatgptBearerToken {
        access_token: access_token.to_string(),
        account_id: session
            .registration
            .as_ref()
            .map(|r| r.client_id.clone())
            .or(session.account_id),
    });
    let api_key = if let Some(registration) = &session.registration {
        if !registration.plan_usage_enabled() {
            return Err("ChatGPT plan usage is not authorized. Please sign in again and allow SpeakFlow to use plan quota.".into());
        }
        let token = chatgpt_token
            .as_ref()
            .ok_or("ChatGPT sign-in is missing an access token. Please sign in again.")?;
        let encoded =
            encode_chatgpt_bearer_token(token).ok_or("Failed to encode ChatGPT session")?;
        encoded.replacen(CHATGPT_BEARER_PREFIX, CHATGPT_PLAN_PREFIX, 1)
    } else if !session.api_key.trim().is_empty() {
        encode_oauth_api_key(&session.api_key)
            .ok_or_else(|| "Failed to wrap OpenAI OAuth API Key".to_string())?
    } else if let Some(token) = &chatgpt_token {
        encode_chatgpt_bearer_token(token)
            .ok_or_else(|| "Failed to encode OpenAI Codex bearer session".to_string())?
    } else {
        String::new()
    };
    Ok(ResolvedProviderAuth {
        api_key,
        chatgpt_token,
    })
}

fn oauth_error_message(error_code: &str, error_description: Option<&str>) -> String {
    if error_code == "access_denied"
        && error_description
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("missing_codex_entitlement")
    {
        return "The current ChatGPT workspace does not have Codex permissions. Please contact your administrator.".to_string();
    }

    if let Some(description) = error_description.filter(|value| !value.trim().is_empty()) {
        return format!("OpenAI Codex OAuth sign-in failed: {description}");
    }

    format!("OpenAI Codex OAuth sign-in failed: {error_code}")
}

fn is_ipv6_loopback_unavailable(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
    ) || matches!(err.raw_os_error(), Some(10047) | Some(10049))
}

async fn bind_callback_listeners() -> Result<CallbackListeners, String> {
    for preferred_port in [Some(DEFAULT_CALLBACK_PORT), None] {
        for _attempt in 0..8 {
            let ipv4 = match TcpListener::bind(("127.0.0.1", preferred_port.unwrap_or(0))).await {
                Ok(listener) => listener,
                Err(err)
                    if err.kind() == std::io::ErrorKind::AddrInUse && preferred_port.is_some() =>
                {
                    break
                }
                Err(err) => {
                    return Err(format!(
                        "Failed to start OAuth IPv4 callback listener: {err}"
                    ))
                }
            };
            let actual_port = ipv4
                .local_addr()
                .map_err(|err| format!("Failed to read OAuth callback port: {err}"))?
                .port();

            match TcpListener::bind(("::1", actual_port)).await {
                Ok(ipv6) => {
                    return Ok(CallbackListeners {
                        ipv4,
                        ipv6: Some(ipv6),
                        port: actual_port,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
                    drop(ipv4);
                    continue;
                }
                Err(err) if is_ipv6_loopback_unavailable(&err) => {
                    return Ok(CallbackListeners {
                        ipv4,
                        ipv6: None,
                        port: actual_port,
                    });
                }
                Err(err) => {
                    return Err(format!(
                        "Failed to start OAuth IPv6 callback listener: {err}"
                    ));
                }
            }
        }
    }

    Err("Failed to start OAuth callback listener: unable to bind local callback port on IPv4 and IPv6 simultaneously.".to_string())
}

async fn accept_callback_connection(
    listeners: &CallbackListeners,
) -> Result<(tokio::net::TcpStream, std::net::SocketAddr), String> {
    let CallbackListeners { ipv4, ipv6, .. } = listeners;
    if let Some(ipv6) = ipv6 {
        tokio::select! {
            result = ipv4.accept() => result.map_err(|err| format!("Failed to accept OpenAI OAuth IPv4 callback: {err}")),
            result = ipv6.accept() => result.map_err(|err| format!("Failed to accept OpenAI OAuth IPv6 callback: {err}")),
        }
    } else {
        ipv4.accept()
            .await
            .map_err(|err| format!("Failed to accept OpenAI OAuth callback: {err}"))
    }
}

async fn wait_for_callback(
    listeners: CallbackListeners,
    expected_state: String,
) -> Result<OAuthCallback, String> {
    wait_for_callback_with_timeout(
        listeners,
        expected_state,
        Duration::from_secs(OAUTH_TIMEOUT_SECS),
    )
    .await
}

async fn wait_for_callback_with_timeout(
    listeners: CallbackListeners,
    expected_state: String,
    timeout: Duration,
) -> Result<OAuthCallback, String> {
    tokio::time::timeout(timeout, async {
        loop {
            let (mut stream, _) = accept_callback_connection(&listeners).await?;
            let Some(url) = super::oauth_callback::read_callback_url(&mut stream).await else {
                continue;
            };

            if url.path() != CALLBACK_PATH {
                let html = callback_html("Invalid Callback", "Unexpected callback path.", false);
                let _ = respond_with_html(&mut stream, "404 Not Found", &html).await;
                continue;
            }

            let query = url
                .query_pairs()
                .into_owned()
                .collect::<std::collections::HashMap<_, _>>();
            let state = query.get("state").map(String::as_str).unwrap_or_default();
            if state != expected_state {
                let html = callback_html(
                    "State Mismatch",
                    "Login state does not match the original request.",
                    false,
                );
                let _ = respond_with_html(&mut stream, "400 Bad Request", &html).await;
                continue;
            }

            if let Some(error_code) = query.get("error") {
                let message = oauth_error_message(
                    error_code,
                    query.get("error_description").map(String::as_str),
                );
                let html = callback_html("Authorization Failed", &message, false);
                let _ = respond_with_html(&mut stream, "200 OK", &html).await;
                return Err(message);
            }

            let Some(code) = query
                .get("code")
                .cloned()
                .filter(|value| !value.trim().is_empty())
            else {
                let html = callback_html(
                    "Missing Code",
                    "Authorization code was not returned.",
                    false,
                );
                let _ = respond_with_html(&mut stream, "400 Bad Request", &html).await;
                continue;
            };

            return Ok(OAuthCallback {
                code,
                client_id: query.get("client_id").cloned(),
                stream,
            });
        }
    })
    .await
    .map_err(|_| "OpenAI OAuth callback timed out. Please try again.".to_string())?
}

async fn exchange_code_for_tokens(
    client: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .timeout(TOKEN_HTTP_TIMEOUT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(
            reqwest::Url::parse_with_params(
                "http://localhost",
                &[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", redirect_uri),
                    ("client_id", CLIENT_ID),
                    ("code_verifier", code_verifier),
                ],
            )
            .map_err(|err| format!("Failed to construct OAuth code exchange parameters: {err}"))?
            .query()
            .unwrap_or_default()
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("Failed to exchange OAuth authorization code for token: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "OAuth authorization code exchange failed ({status}): {body}"
        ));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("Failed to parse OAuth token response: {err}"))
}

async fn request_device_code(
    client: &reqwest::Client,
) -> Result<OpenaiCodexOauthDeviceCodeChallenge, String> {
    let response = client
        .post(format!("{ISSUER}/api/accounts/deviceauth/usercode"))
        .timeout(TOKEN_HTTP_TIMEOUT)
        .json(&DeviceCodeRequest {
            client_id: CLIENT_ID.to_string(),
        })
        .send()
        .await
        .map_err(|err| format!("Failed to request OpenAI Codex device code: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(
                "Device code sign-in is not enabled for the OpenAI Codex service. Please use browser sign-in.".to_string(),
            );
        }
        return Err(format!(
            "Failed to request OpenAI Codex device code ({status}): {body}"
        ));
    }

    let payload = response
        .json::<DeviceCodeResponse>()
        .await
        .map_err(|err| format!("Failed to parse OpenAI Codex device code response: {err}"))?;

    Ok(OpenaiCodexOauthDeviceCodeChallenge {
        verification_url: format!("{ISSUER}/codex/device"),
        user_code: payload.user_code,
        device_auth_id: payload.device_auth_id,
        interval_secs: payload.interval.max(1),
    })
}

async fn poll_device_code_authorization(
    client: &reqwest::Client,
    challenge: &OpenaiCodexOauthDeviceCodeChallenge,
) -> Result<DeviceCodeAuthorizationResponse, String> {
    let url = format!("{ISSUER}/api/accounts/deviceauth/token");
    let max_wait = Duration::from_secs(15 * 60);
    let interval = Duration::from_secs(challenge.interval_secs.clamp(1, 30));
    let start = Instant::now();

    loop {
        let response = client
            .post(&url)
            .timeout(TOKEN_HTTP_TIMEOUT)
            .json(&DeviceCodePollRequest {
                device_auth_id: challenge.device_auth_id.clone(),
                user_code: challenge.user_code.clone(),
            })
            .send()
            .await
            .map_err(|err| {
                format!("Failed to poll OpenAI Codex device code authorization: {err}")
            })?;

        let status = response.status();
        if status.is_success() {
            let payload = response
                .json::<DeviceCodeAuthorizationResponse>()
                .await
                .map_err(|err| {
                    format!(
                        "Failed to parse OpenAI Codex device code authorization response: {err}"
                    )
                })?;
            if payload.authorization_code.trim().is_empty()
                || payload.code_verifier.trim().is_empty()
            {
                return Err("OpenAI Codex device code authorization response is missing an authorization code. Please try again.".to_string());
            }
            return Ok(payload);
        }

        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            if start.elapsed() >= max_wait {
                return Err(
                    "OpenAI Codex device code sign-in timed out. Please start sign-in again."
                        .to_string(),
                );
            }
            let remaining = max_wait.saturating_sub(start.elapsed());
            tokio::time::sleep(interval.min(remaining)).await;
            continue;
        }

        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "OpenAI Codex device code authorization failed ({status}): {body}"
        ));
    }
}

async fn refresh_tokens(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, String> {
    refresh_tokens_at(
        client,
        &format!("{ISSUER}/oauth/token"),
        refresh_token,
        TOKEN_HTTP_TIMEOUT,
    )
    .await
}

async fn refresh_tokens_at(
    client: &reqwest::Client,
    endpoint: &str,
    refresh_token: &str,
    timeout: Duration,
) -> Result<TokenResponse, String> {
    let response = client
        .post(endpoint)
        .timeout(timeout)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(
            reqwest::Url::parse_with_params(
                "http://localhost",
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                    ("client_id", CLIENT_ID),
                ],
            )
            .map_err(|err| format!("Failed to construct OAuth refresh parameters: {err}"))?
            .query()
            .unwrap_or_default()
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("Failed to refresh Codex OAuth token: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let code = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v["error"]
                    .as_str()
                    .or_else(|| v["error"]["code"].as_str())
                    .map(str::to_string)
            });
        if status == reqwest::StatusCode::UNAUTHORIZED
            || code.as_deref().is_some_and(|c| {
                matches!(
                    c,
                    "invalid_grant"
                        | "invalid_refresh_token"
                        | "token_expired"
                        | "refresh_token_expired"
                        | "refresh_token_reused"
                        | "refresh_token_invalidated"
                )
            })
        {
            return Err(
                "CODEX_REAUTH_REQUIRED: Codex sign-in expired. Please sign in again.".into(),
            );
        }
        return Err(format!(
            "Failed to refresh Codex OAuth token ({status}). Please try again later."
        ));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("Failed to parse refresh token response: {err}"))
}

fn session_needs_refresh(session: &OpenaiCodexOauthSession) -> bool {
    match session.expires_at_ms {
        Some(expires_at_ms) => expires_at_ms <= now_ms().saturating_add(REFRESH_SKEW_SECS * 1000),
        None => false,
    }
}

fn session_has_runtime_auth_material(session: &OpenaiCodexOauthSession) -> bool {
    !session.api_key.trim().is_empty() || !session.access_token.trim().is_empty()
}

async fn session_from_token_response(
    token_response: TokenResponse,
) -> Result<OpenaiCodexOauthSession, String> {
    let id_token = match token_response.id_token.clone() {
        Some(id_token) if !id_token.trim().is_empty() => id_token,
        _ => return Err("OAuth response is missing id_token. Cannot continue.".to_string()),
    };
    let refresh_token = match token_response
        .refresh_token
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        Some(refresh_token) => refresh_token,
        None => return Err("OAuth response is missing refresh_token. Cannot continue.".to_string()),
    };
    let mut session = OpenaiCodexOauthSession {
        registration: None,
        id_token,
        access_token: token_response.access_token,
        refresh_token,
        api_key: String::new(),
        expires_at_ms: token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000)),
        account_id: None,
        email: None,
        plan_type: None,
    };
    enrich_session_from_tokens(&mut session, None);
    Ok(session)
}

fn persist_login_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    operation: OAuthOperation,
    session: OpenaiCodexOauthSession,
) -> Result<OpenaiCodexOauthStatus, String> {
    let mut status = make_status(Some(&session));
    let committed = state
        .openai_codex_oauth_state()
        .commit(operation, session, |session| {
            save_session_to_storage(app_handle, session)
        })?;
    if !committed {
        return Err(
            "OpenAI Codex OAuth sign-in superseded by a newer operation. Please try again."
                .to_string(),
        );
    }
    status.saved_accounts = chatgpt_sign_in::saved_accounts();
    Ok(status)
}

pub(crate) fn should_prewarm_runtime_session(
    provider: &str,
    config: &LlmProviderConfig,
    session: Option<&OpenaiCodexOauthSession>,
) -> bool {
    if provider != OPENAI_PROVIDER {
        return false;
    }

    let Some(session) = session else {
        return false;
    };

    let effective_mode = config.openai_auth_mode.unwrap_or(OpenaiAuthMode::Oauth);
    if effective_mode != OpenaiAuthMode::Oauth {
        return false;
    }

    session_needs_refresh(session) || !session_has_runtime_auth_material(session)
}

async fn refresh_session_if_needed(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<Option<OpenaiCodexOauthSession>, String> {
    refresh_session(app_handle, state, None).await
}

async fn refresh_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    rejected_auth: Option<&str>,
) -> Result<Option<OpenaiCodexOauthSession>, String> {
    let _refresh_guard = state.openai_codex_oauth_state().lock_refresh().await;
    let Some((operation, mut session)) = state.openai_codex_oauth_state().snapshot_for_refresh()
    else {
        return Ok(None);
    };
    let refresh_start = std::time::Instant::now();
    if let Some(rejected) = rejected_auth {
        let rejected_token = decode_chatgpt_bearer_token(rejected);
        let current = auth_from_openai_session(session.clone())?;
        if let (Some(old), Some(new)) = (rejected_token, current.chatgpt_token.as_ref()) {
            if old.account_id != new.account_id
                || is_chatgpt_plan_auth(rejected) != session.registration.is_some()
            {
                return Err("ChatGPT account switched. Please retry the current operation.".into());
            }
        } else if current.api_key != rejected {
            return Err(
                "ChatGPT authentication was updated. Please retry the current operation.".into(),
            );
        }
        if session_has_runtime_auth_material(&session)
            && auth_from_openai_session(session.clone())?.api_key != rejected
        {
            return Ok(Some(session));
        }
    }
    if rejected_auth.is_none()
        && !session_needs_refresh(&session)
        && session_has_runtime_auth_material(&session)
    {
        log::info!(
            "OpenAI Codex OAuth credentials ready ({}ms): api_key={}, bearer={}",
            refresh_start.elapsed().as_millis(),
            !session.api_key.trim().is_empty(),
            !session.access_token.trim().is_empty()
        );
        return Ok(Some(session));
    }

    let needs_rehydration =
        session.id_token.trim().is_empty() || session.access_token.trim().is_empty();

    if session.registration.is_some() {
        let refreshed = match chatgpt_sign_in::refresh(&state.http_client, &session).await {
            Ok(refreshed) => refreshed,
            Err(error) => {
                if error.starts_with("CHATGPT_REAUTH_REQUIRED:") {
                    state
                        .openai_codex_oauth_state()
                        .invalidate(Some(operation), || clear_session_from_storage(app_handle))?;
                }
                return Err(error);
            }
        };
        if !state
            .openai_codex_oauth_state()
            .commit(operation, refreshed.clone(), |s| {
                save_session_to_storage(app_handle, s)
            })?
        {
            return Err(
                "ChatGPT session superseded by a newer operation. Please try again.".into(),
            );
        }
        return Ok(Some(refreshed));
    }

    let refreshed = if session_needs_refresh(&session)
        || needs_rehydration
        || rejected_auth.is_some()
    {
        if session.refresh_token.trim().is_empty() {
            return Err(
                "OpenAI Codex OAuth session is missing a refresh token. Please sign in again."
                    .to_string(),
            );
        }
        let token_refresh_start = std::time::Instant::now();
        let token_response = match refresh_tokens(&state.http_client, &session.refresh_token).await
        {
            Ok(tokens) => tokens,
            Err(error) => {
                if error.starts_with("CODEX_REAUTH_REQUIRED:") {
                    state
                        .openai_codex_oauth_state()
                        .invalidate(Some(operation), || clear_session_from_storage(app_handle))?;
                }
                return Err(error);
            }
        };
        log::info!(
            "OpenAI Codex OAuth refresh completed ({}ms): rehydration={}, expired={}",
            token_refresh_start.elapsed().as_millis(),
            needs_rehydration,
            session_needs_refresh(&session)
        );
        let id_token = token_response
            .id_token
            .unwrap_or_else(|| session.id_token.clone());
        let access_token = token_response.access_token;
        let refresh_token = token_response
            .refresh_token
            .unwrap_or_else(|| session.refresh_token.clone());
        let expires_at_ms = token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000));

        OpenaiCodexOauthSession {
            registration: None,
            id_token,
            access_token,
            refresh_token,
            api_key: String::new(),
            expires_at_ms,
            account_id: session.account_id.take(),
            email: session.email.take(),
            plan_type: session.plan_type.take(),
        }
    } else {
        session
    };

    let mut refreshed = refreshed;
    enrich_session_from_tokens(&mut refreshed, None);
    let committed =
        state
            .openai_codex_oauth_state()
            .commit(operation, refreshed.clone(), |session| {
                save_session_to_storage(app_handle, session)
            })?;
    if !committed {
        return Err(
            "OpenAI Codex OAuth session superseded by a newer operation. Please try again."
                .to_string(),
        );
    }
    log::info!(
        "OpenAI Codex OAuth credentials rehydration completed ({}ms): api_key={}, bearer={}",
        refresh_start.elapsed().as_millis(),
        !refreshed.api_key.trim().is_empty(),
        !refreshed.access_token.trim().is_empty()
    );
    Ok(Some(refreshed))
}

pub fn sync_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Option<OpenaiCodexOauthSession> {
    let login = state.openai_codex_oauth_state().begin_login();
    let operation = login.token();
    let loaded = load_session_from_storage(app_handle);
    let result = match loaded {
        Some(session) => state
            .openai_codex_oauth_state()
            .commit(operation, session, |_| Ok(())),
        None => Ok(false),
    };
    drop(login);
    if let Err(error) = result {
        log::warn!("Failed to restore OpenAI Codex OAuth session: {error}");
    }
    state.read_openai_codex_oauth_session()
}

pub fn status(state: &AppState) -> OpenaiCodexOauthStatus {
    make_status(state.read_openai_codex_oauth_session().as_ref())
}

pub async fn prewarm_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<(), String> {
    let provider = state.active_llm_provider();
    let config = state.llm_provider_config();
    let session = state.read_openai_codex_oauth_session();
    if !should_prewarm_runtime_session(&provider, &config, session.as_ref()) {
        log::info!(
            "OpenAI Codex OAuth startup prewarm skipped: provider={}, auth_mode={:?}, has_session={}",
            provider,
            config.openai_auth_mode,
            session.is_some()
        );
        return Ok(());
    }
    let session = session.expect("prewarm decision requires a session");
    let start = std::time::Instant::now();
    log::info!(
        "OpenAI Codex OAuth startup prewarm started: cached_api_key={}, cached_bearer={}, expires_at_ms={:?}",
        !session.api_key.trim().is_empty(),
        !session.access_token.trim().is_empty(),
        session.expires_at_ms
    );
    refresh_session_if_needed(app_handle, state).await?;
    log::info!(
        "OpenAI Codex OAuth startup prewarm completed ({}ms)",
        start.elapsed().as_millis()
    );
    Ok(())
}

pub async fn login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    client_id: Option<&str>,
    new_account: bool,
) -> Result<OpenaiCodexOauthStatus, String> {
    chatgpt_sign_in::login(app_handle, state, client_id, new_account).await
}

pub async fn start_device_code_login(
    state: &AppState,
) -> Result<OpenaiCodexOauthDeviceCodeChallenge, String> {
    let login = state
        .openai_codex_oauth_state()
        .begin_login_after_refresh()
        .await;
    let operation = login.token();
    let challenge = request_device_code(&state.http_client).await?;
    if challenge.device_auth_id.trim().is_empty() || challenge.user_code.trim().is_empty() {
        return Err("Invalid OpenAI Codex device code response. Please try again.".to_string());
    }
    let published = state
        .openai_codex_oauth_state()
        .publish_challenge(operation, device_challenge_binding(&challenge))?;
    if !published {
        return Err(
            "OpenAI Codex device code sign-in superseded by a newer operation. Please try again."
                .to_string(),
        );
    }
    drop(login);
    if let Err(err) = webbrowser::open(&challenge.verification_url) {
        log::warn!(
            "Failed to open OpenAI Codex device code verification page; UI will display URL: {}",
            err
        );
    }
    Ok(challenge)
}

pub async fn complete_device_code_login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    challenge: OpenaiCodexOauthDeviceCodeChallenge,
) -> Result<OpenaiCodexOauthStatus, String> {
    if challenge.device_auth_id.trim().is_empty() || challenge.user_code.trim().is_empty() {
        return Err("OpenAI Codex device code expired. Please start sign-in again.".to_string());
    }
    let binding = device_challenge_binding(&challenge);
    let login = state
        .openai_codex_oauth_state()
        .claim_challenge(&binding)
        .ok_or_else(|| {
            "OpenAI Codex device code expired. Please start sign-in again.".to_string()
        })?;
    let operation = login.token();
    let authorization = poll_device_code_authorization(&state.http_client, &challenge).await?;
    let redirect_uri = format!("{ISSUER}/deviceauth/callback");
    let token_response = exchange_code_for_tokens(
        &state.http_client,
        &authorization.authorization_code,
        &redirect_uri,
        &authorization.code_verifier,
    )
    .await?;
    let session = session_from_token_response(token_response).await?;
    let result = persist_login_session(app_handle, state, operation, session);
    drop(login);
    result
}

pub async fn logout(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    // Revoke the latest rotated credential and serialize a new sign-in with
    // remote revocation of the previous grant.
    let _refresh_guard = state.openai_codex_oauth_state().lock_refresh().await;
    let session = state.read_openai_codex_oauth_session();
    // Invalidate before awaiting revocation: an in-flight refresh must not revive a signed-out session.
    state
        .openai_codex_oauth_state()
        .invalidate(None, || clear_session_from_storage(app_handle))
        .map(|_| ())?;
    if let Some(session) = session.filter(|s| s.registration.is_some()) {
        chatgpt_sign_in::revoke(&state.http_client, &session).await?;
    }
    Ok(())
}

pub async fn remove_account(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    client_id: &str,
) -> Result<OpenaiCodexOauthStatus, String> {
    if client_id.trim().is_empty() {
        return Err("Please select a ChatGPT account to remove.".into());
    }
    let _refresh_guard = state.openai_codex_oauth_state().lock_refresh().await;
    let removed = state.openai_codex_oauth_state().invalidate_matching(
        |session| {
            session
                .registration
                .as_ref()
                .is_some_and(|r| r.client_id == client_id)
        },
        |active| {
            if active {
                clear_session_from_storage(app_handle)?;
            }
            chatgpt_sign_in::forget_account(client_id)
        },
    )?;
    if let Some(session) = removed {
        // Local removal remains effective if remote revocation fails.
        chatgpt_sign_in::revoke(&state.http_client, &session).await?;
    }
    Ok(status(state))
}

pub async fn recover_rejected_auth(
    app: &tauri::AppHandle,
    state: &AppState,
    provider: &str,
    rejected: &str,
) -> Result<Option<String>, String> {
    if provider == XAI_PROVIDER
        && grok_build_oauth_service::is_grok_build_oauth_origin_auth(rejected)
    {
        return grok_build_oauth_service::recover_rejected_auth(app, state, rejected)
            .await
            .map(Some);
    }
    if provider == claude_oauth_service::CLAUDE_PROVIDER {
        // A Claude subscription token can expire mid-session; refresh it once
        // instead of failing the request.
        if claude_oauth_service::decode_claude_oauth_access_token(rejected).is_none() {
            return Ok(None);
        }
        return claude_oauth_service::resolve_oauth_origin_api_key(app, state)
            .await
            .map(Some);
    }
    if provider != OPENAI_PROVIDER || !is_oauth_origin_auth(rejected) {
        return Ok(None);
    }
    let snapshot = state
        .read_openai_codex_oauth_session()
        .ok_or("ChatGPT has been signed out. Please sign in again.")?;
    let rejected_token = decode_chatgpt_bearer_token(rejected);
    let current = auth_from_openai_session(snapshot.clone())?;
    if let (Some(old), Some(new)) = (rejected_token, current.chatgpt_token) {
        if old.account_id != new.account_id
            || is_chatgpt_plan_auth(rejected) != snapshot.registration.is_some()
        {
            return Err("ChatGPT account switched. Please retry the current operation.".into());
        }
    }
    let session = refresh_session(app, state, Some(rejected))
        .await?
        .ok_or("ChatGPT sign-in is updating. Please try again.")?;
    let refreshed = auth_from_openai_session(session)?.api_key;
    if decode_oauth_api_key(rejected).is_some() != decode_oauth_api_key(&refreshed).is_some() {
        return Err(
            "ChatGPT authentication route updated. Please retry the current operation.".into(),
        );
    }
    Ok(Some(refreshed))
}

pub fn sync_from_system(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<OpenaiCodexOauthStatus, String> {
    let session = read_system_session()?
        .ok_or("No ChatGPT sign-in found in Codex or Magpie on this machine.")?;

    let login = state.openai_codex_oauth_state().begin_login();
    let operation = login.token();
    let status = make_status(Some(&session));
    let committed = state
        .openai_codex_oauth_state()
        .commit(operation, session, |session| {
            save_session_to_storage(app_handle, session)
        })?;
    if !committed {
        return Err("The ChatGPT sign-in changed while syncing. Please retry.".to_string());
    }
    Ok(status)
}

fn read_system_session() -> Result<Option<OpenaiCodexOauthSession>, String> {
    let home = dirs::home_dir().ok_or("Cannot locate user home directory")?;

    let magpie_logins = home.join(".config").join("magpie").join("logins.json");
    if let Ok(content) = std::fs::read_to_string(&magpie_logins) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(session) = session_from_magpie_logins(&json) {
                return Ok(Some(session));
            }
        }
    }

    let codex_auth = home.join(".codex").join("auth.json");
    if let Ok(content) = std::fs::read_to_string(&codex_auth) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(session) = session_from_codex_auth(&json) {
                return Ok(Some(session));
            }
        }
    }

    Ok(None)
}

fn session_from_magpie_logins(json: &serde_json::Value) -> Option<OpenaiCodexOauthSession> {
    for entry in json.as_array().into_iter().flatten() {
        if entry.get("agent").and_then(|value| value.as_str()) != Some("codex") {
            continue;
        }
        // Prefer an account that is switched on.
        if entry.get("on").and_then(|value| value.as_bool()) == Some(false) {
            continue;
        }
        let Some(tokens) = entry.get("auth").and_then(|auth| auth.get("tokens")) else {
            continue;
        };
        let Some(mut session) = session_from_codex_tokens(tokens) else {
            continue;
        };
        session.email = entry
            .get("user")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        return Some(session);
    }
    None
}

fn session_from_codex_auth(json: &serde_json::Value) -> Option<OpenaiCodexOauthSession> {
    session_from_codex_tokens(json.get("tokens")?)
}

fn session_from_codex_tokens(tokens: &serde_json::Value) -> Option<OpenaiCodexOauthSession> {
    let access_token = tokens.get("access_token")?.as_str()?.trim().to_string();
    let refresh_token = tokens.get("refresh_token")?.as_str()?.trim().to_string();
    if access_token.is_empty() || refresh_token.is_empty() {
        return None;
    }
    let mut session = OpenaiCodexOauthSession {
        registration: None,
        id_token: tokens
            .get("id_token")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string(),
        access_token,
        refresh_token,
        api_key: String::new(),
        expires_at_ms: None,
        account_id: tokens
            .get("account_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        email: None,
        plan_type: None,
    };
    enrich_session_from_tokens(&mut session, None);
    Some(session)
}

pub async fn resolve_api_key_for_provider_with_auth_mode(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    provider: &str,
    manual_api_key: &str,
    auth_mode_override: Option<OpenaiAuthMode>,
    xai_auth_mode_override: Option<XaiAuthMode>,
    claude_auth_mode_override: Option<ClaudeAuthMode>,
) -> Result<String, String> {
    resolve_provider_auth_with_auth_mode(
        app_handle,
        state,
        provider,
        manual_api_key,
        auth_mode_override,
        xai_auth_mode_override,
        claude_auth_mode_override,
    )
    .await
    .map(|auth| auth.api_key)
}

/// Modes are captured by the caller. None means infer from login state,
/// so a later profile edit cannot change an already-started request's choice.
pub async fn resolve_provider_auth_with_auth_mode(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    provider: &str,
    manual_api_key: &str,
    auth_mode: Option<OpenaiAuthMode>,
    xai_auth_mode: Option<XaiAuthMode>,
    claude_auth_mode: Option<ClaudeAuthMode>,
) -> Result<ResolvedProviderAuth, String> {
    let manual_api_key = manual_api_key.trim();

    if magpie_service::is_magpie_provider(provider) {
        // Magpie's gateway is local and takes any bearer, so there is no key
        // to store or prompt for.
        return Ok(ResolvedProviderAuth::api_key(
            if manual_api_key.is_empty() {
                magpie_service::MAGPIE_GATEWAY_KEY.to_string()
            } else {
                manual_api_key.to_string()
            },
        ));
    }

    if provider == XAI_PROVIDER {
        let effective_mode = grok_build_oauth_service::effective_xai_auth_mode(
            xai_auth_mode,
            state.read_grok_build_oauth_session().is_some(),
        );
        return match effective_mode {
            XaiAuthMode::ApiKey => Ok(ResolvedProviderAuth::api_key(manual_api_key.to_string())),
            XaiAuthMode::Oauth => {
                grok_build_oauth_service::resolve_oauth_origin_api_key(app_handle, state)
                    .await
                    .map(ResolvedProviderAuth::api_key)
            }
        };
    }

    if provider == claude_oauth_service::CLAUDE_PROVIDER {
        let effective_mode = claude_oauth_service::effective_claude_auth_mode(
            claude_auth_mode,
            state.read_claude_oauth_session().is_some(),
        );
        return match effective_mode {
            ClaudeAuthMode::ApiKey => Ok(ResolvedProviderAuth::api_key(manual_api_key.to_string())),
            ClaudeAuthMode::Oauth => {
                claude_oauth_service::resolve_oauth_origin_api_key(app_handle, state)
                    .await
                    .map(ResolvedProviderAuth::api_key)
            }
        };
    }

    // Non-OpenAI provider: return the manually entered key directly
    if provider != OPENAI_PROVIDER {
        return Ok(ResolvedProviderAuth::api_key(manual_api_key.to_string()));
    }

    // OpenAI: branch by user-selected authentication mode.
    // If not explicitly set, infer from OAuth login status.
    let effective_mode = auth_mode.unwrap_or_else(|| {
        if state.read_openai_codex_oauth_session().is_some() {
            OpenaiAuthMode::Oauth
        } else {
            OpenaiAuthMode::ApiKey
        }
    });

    match effective_mode {
        OpenaiAuthMode::ApiKey => {
            // Explicitly API Key: use manual key only
            Ok(ResolvedProviderAuth::api_key(manual_api_key.to_string()))
        }
        OpenaiAuthMode::Oauth => {
            // Explicitly OAuth: read session, ignore manual key
            if state.read_openai_codex_oauth_session().is_none() {
                return Ok(ResolvedProviderAuth::api_key(String::new()));
            }

            let Some(session) = refresh_session_if_needed(app_handle, state).await? else {
                return Ok(ResolvedProviderAuth::api_key(String::new()));
            };
            auth_from_openai_session(session)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_deadline_releases_login_waiter_for_stalled_headers_and_body() {
        use tokio::io::AsyncReadExt;
        for send_headers in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 2048];
                assert!(socket.read(&mut buffer).await.unwrap() > 0);
                if send_headers {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 256\r\n\r\n{").await.unwrap();
                }
                std::future::pending::<()>().await;
            });
            let coordinator = crate::state::oauth_session::OAuthSessionState::default();
            coordinator.restore(Some("original-refresh-token".to_string()));
            let guard = coordinator.lock_refresh().await;
            let client = reqwest::Client::new();
            let outcome = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(
                    async {
                        let result = refresh_tokens_at(
                            &client,
                            &endpoint,
                            "test-refresh",
                            Duration::from_millis(50),
                        )
                        .await;
                        drop(guard);
                        result
                    },
                    coordinator.begin_login_after_refresh()
                )
            })
            .await;
            server.abort();
            let (result, login) = outcome.expect("refresh deadline must release the login waiter");
            let error = result.unwrap_err();
            assert!(
                !error.starts_with("CODEX_REAUTH_REQUIRED:"),
                "network timeout must preserve credentials"
            );
            drop(login);
            assert_eq!(
                coordinator.read().as_deref(),
                Some("original-refresh-token")
            );
        }
    }

    #[tokio::test]
    async fn browser_callback_uses_an_available_port_when_codex_is_running() {
        let occupied = TcpListener::bind(("127.0.0.1", DEFAULT_CALLBACK_PORT)).await;
        let listeners = bind_callback_listeners()
            .await
            .expect("busy preferred port must fall back");
        assert_ne!(listeners.port, DEFAULT_CALLBACK_PORT);
        drop(occupied);
    }

    #[tokio::test]
    async fn callback_total_deadline_includes_a_stalled_request_line() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let port = listener.local_addr().unwrap().port();
        let listeners = CallbackListeners {
            ipv4: listener,
            ipv6: None,
            port,
        };
        let mut stalled = tokio::net::TcpStream::connect(address).await.unwrap();
        stalled.write_all(b"GET /unfinished").await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(600),
            wait_for_callback_with_timeout(
                listeners,
                "expected".into(),
                Duration::from_millis(200),
            ),
        )
        .await;
        let Err(error) = result.expect("whole callback deadline must include accepted connections")
        else {
            panic!("partial line must not authorize login");
        };
        assert!(error.contains("timed out"), "{error}");
    }

    #[tokio::test]
    async fn callback_ignores_unrelated_and_incomplete_requests() {
        use tokio::net::TcpStream;
        for request in [
            "GET /favicon.ico HTTP/1.1\r\n".to_string(),
            "GET /auth/callback?state=wrong&code=bad HTTP/1.1\r\n".to_string(),
            "G".repeat(9_000),
            "GET /unfinished".to_string(),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let port = listener.local_addr().unwrap().port();
            let listeners = CallbackListeners {
                ipv4: listener,
                ipv6: None,
                port,
            };
            let callback = tokio::spawn(wait_for_callback(listeners, "expected".into()));
            let mut unrelated = TcpStream::connect(address).await.unwrap();
            unrelated.write_all(request.as_bytes()).await.unwrap();
            let mut valid = TcpStream::connect(address).await.unwrap();
            let _ = valid
                .write_all(b"GET /auth/callback?state=expected&code=good HTTP/1.1\r\n\r\n")
                .await;
            let result = tokio::time::timeout(Duration::from_secs(4), callback)
                .await
                .expect("unrelated connection must not block a valid callback")
                .unwrap();
            assert_eq!(result.map(|callback| callback.code).unwrap(), "good");
        }
    }

    #[test]
    fn resolved_oauth_api_key_and_catalog_token_come_from_one_session() {
        let session = OpenaiCodexOauthSession {
            api_key: "derived-key-a".into(),
            access_token: "bearer-a".into(),
            account_id: Some("account-a".into()),
            ..Default::default()
        };
        let auth = auth_from_openai_session(session).unwrap();
        assert_eq!(
            decode_oauth_api_key(&auth.api_key).as_deref(),
            Some("derived-key-a")
        );
        let token = auth.chatgpt_token.unwrap();
        assert_eq!(token.access_token, "bearer-a");
        assert_eq!(token.account_id.as_deref(), Some("account-a"));
    }

    #[test]
    fn resolved_bearer_encoding_preserves_its_catalog_identity() {
        let auth = auth_from_openai_session(OpenaiCodexOauthSession {
            access_token: "bearer-only".into(),
            account_id: Some("bearer-account".into()),
            ..Default::default()
        })
        .unwrap();
        let encoded = decode_chatgpt_bearer_token(&auth.api_key).unwrap();
        let token = auth.chatgpt_token.unwrap();
        assert_eq!(encoded.access_token, token.access_token);
        assert_eq!(encoded.account_id, token.account_id);
    }

    #[test]
    fn device_code_response_accepts_string_interval_and_usercode_alias() {
        let payload = serde_json::json!({
            "device_auth_id": "device-auth",
            "usercode": "ABCD-1234",
            "interval": "7"
        });

        let parsed: DeviceCodeResponse =
            serde_json::from_value(payload).expect("device code response should parse");

        assert_eq!(parsed.device_auth_id, "device-auth");
        assert_eq!(parsed.user_code, "ABCD-1234");
        assert_eq!(parsed.interval, 7);
    }

    #[test]
    fn device_code_challenge_uses_camel_case_wire_shape() {
        let challenge = OpenaiCodexOauthDeviceCodeChallenge {
            verification_url: "https://auth.openai.com/codex/device".to_string(),
            user_code: "ABCD-1234".to_string(),
            device_auth_id: "device-auth".to_string(),
            interval_secs: 5,
        };

        let wire = serde_json::to_value(challenge).expect("challenge should serialize");

        assert_eq!(
            wire["verificationUrl"],
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(wire["userCode"], "ABCD-1234");
        assert_eq!(wire["deviceAuthId"], "device-auth");
        assert_eq!(wire["intervalSecs"], 5);
    }

    #[test]
    fn device_challenge_binding_uses_server_device_id_and_user_code() {
        let challenge = OpenaiCodexOauthDeviceCodeChallenge {
            verification_url: "https://example.invalid".to_string(),
            user_code: "USER-CODE".to_string(),
            device_auth_id: "device-auth".to_string(),
            interval_secs: 5,
        };

        assert_eq!(
            device_challenge_binding(&challenge),
            "device-auth\0USER-CODE"
        );
    }

    #[test]
    fn callback_page_uses_the_native_ui_font_stack() {
        let html = callback_html("<Title>", "Message & details", false);

        assert!(html.contains(
            "font-family:-apple-system,BlinkMacSystemFont,'SF Pro Text','Helvetica Neue'"
        ));
        assert!(!html.contains("PingFang SC"));
        assert!(!html.contains("Microsoft YaHei UI"));
        assert!(!html.contains("@font-face"));
        assert!(!html.contains("data:font/woff2;base64,"));
        assert!(html.contains("letter-spacing:-.008em"));
        assert!(html.contains("letter-spacing:-.02em"));
        assert!(html.contains("&lt;Title&gt;"));
        assert!(html.contains("Message &amp; details"));
    }

    #[test]
    fn oauth_metadata_rejects_malformed_json_and_non_file_atomic_targets() {
        let root = std::env::temp_dir().join(format!(
            "speak-flow-codex-oauth-{}-{}",
            std::process::id(),
            now_ms()
        ));
        std::fs::create_dir_all(&root).expect("temporary metadata directory");
        let metadata_path = root.join("session.json");
        assert!(read_session_meta_at(&metadata_path)
            .expect("missing metadata is distinguishable")
            .is_none());
        crate::utils::paths::atomic_write(&metadata_path, b"{malformed")
            .expect("malformed metadata");

        assert!(read_session_meta_at(&metadata_path).is_err());
        crate::utils::paths::atomic_write(&metadata_path, b"{}").expect("empty metadata");
        assert!(read_session_meta_at(&metadata_path).is_err());

        let target_directory = root.join("target-directory");
        std::fs::create_dir_all(&target_directory).expect("atomic target directory");
        let metadata = PersistedOpenaiCodexOauthSession::default();
        assert!(write_metadata_at(&target_directory, &metadata).is_err());

        std::fs::remove_dir_all(root).expect("temporary metadata cleanup");
    }

    #[test]
    fn magpie_sync_skips_switched_off_and_unusable_accounts() {
        let json = serde_json::json!([
            {
                "agent": "codex",
                "on": false,
                "auth": { "tokens": { "access_token": "off-access", "refresh_token": "off-refresh" } }
            },
            {
                "agent": "claude",
                "on": true,
                "auth": { "tokens": { "access_token": "other-access", "refresh_token": "other-refresh" } }
            },
            {
                "agent": "codex",
                "on": true,
                "auth": { "tokens": { "access_token": "  " } }
            },
            {
                "agent": "codex",
                "on": true,
                "user": "on@example.com",
                "auth": {
                    "tokens": {
                        "access_token": "on-access",
                        "refresh_token": "on-refresh",
                        "account_id": "acc-1"
                    }
                }
            }
        ]);

        let session = session_from_magpie_logins(&json).expect("an enabled account is used");
        assert_eq!(session.access_token, "on-access");
        assert_eq!(session.refresh_token, "on-refresh");
        assert_eq!(session.account_id.as_deref(), Some("acc-1"));
        assert_eq!(session.email.as_deref(), Some("on@example.com"));
    }

    #[test]
    fn codex_auth_file_requires_both_tokens() {
        let complete = serde_json::json!({
            "tokens": { "access_token": "acc", "refresh_token": "ref" }
        });
        assert!(session_from_codex_auth(&complete).is_some());

        let missing_refresh = serde_json::json!({
            "tokens": { "access_token": "acc" }
        });
        assert!(session_from_codex_auth(&missing_refresh).is_none());
        assert!(session_from_codex_auth(&serde_json::json!({})).is_none());
    }
}
