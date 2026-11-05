//! Anthropic Claude subscription sign-in (Claude Code OAuth, authorization-code
//! with PKCE). Anthropic returns the code on its own success page instead of a
//! loopback redirect, so the flow is: open the browser, the user copies the
//! code the page shows, and the app exchanges it here.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri_plugin_keyring::KeyringExt;

use super::oauth_callback::{delete_keyring_password, generate_pkce_pair, generate_state, now_ms};
use crate::services::llm_provider::KEYRING_SERVICE;
use crate::state::user_profile::ClaudeAuthMode;
use crate::state::AppState;
use crate::utils::paths;

const TOKEN_HTTP_TIMEOUT: Duration = Duration::from_secs(20);

pub const CLAUDE_PROVIDER: &str = "claude";
/// Claude Code's public OAuth client. Not a secret: it ships inside the CLI.
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE_URL: &str = "https://claude.com/cai/oauth/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Anthropic renders the code on this page; it is not a loopback listener.
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const SCOPE: &str = "user:file_upload user:inference user:mcp_servers user:plugins user:profile user:sessions:claude_code";
const SESSION_KEYRING_USER: &str = "claude-oauth";
const SESSION_REFRESH_TOKEN_KEYRING_USER: &str = "claude-oauth-refresh-token";
const REFRESH_SKEW_SECS: u64 = 60;
/// A browser round trip must finish inside this window.
const PENDING_LOGIN_TTL: Duration = Duration::from_secs(15 * 60);
pub const CLAUDE_OAUTH_PREFIX: &str = "claude-oauth:";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaudeOauthSession {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_ms: Option<u64>,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOauthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub plan_type: Option<String>,
    pub expires_at_ms: Option<u64>,
}

/// What the UI needs to render the sign-in step: a URL to open and the state
/// the pasted code must carry.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeOauthStart {
    pub authorize_url: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

struct PendingLogin {
    verifier: String,
    state: String,
    started_at: Instant,
}

static PENDING_LOGIN: Mutex<Option<PendingLogin>> = Mutex::new(None);

struct RefreshFailure {
    message: String,
    invalidate_session: bool,
}

fn form_encode(pairs: &[(&str, &str)]) -> Result<String, String> {
    Ok(reqwest::Url::parse_with_params("http://localhost", pairs)
        .map_err(|err| format!("Failed to construct OAuth form: {err}"))?
        .query()
        .unwrap_or_default()
        .to_string())
}

fn session_meta_path() -> std::path::PathBuf {
    paths::get_data_dir().join("claude_oauth_session.json")
}

fn logout_tombstone_path() -> std::path::PathBuf {
    paths::get_data_dir().join("claude_oauth_session.json.logout")
}

fn write_logout_tombstone() -> Result<(), String> {
    paths::atomic_write(&logout_tombstone_path(), b"logged_out")
        .map_err(|_| "Failed to write Claude OAuth logout marker".to_string())
}

fn remove_logout_tombstone() -> Result<(), String> {
    match std::fs::remove_file(logout_tombstone_path()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Failed to clear Claude OAuth logout marker".to_string()),
    }
}

fn write_session_meta(session: &ClaudeOauthSession) -> Result<(), String> {
    let payload = serde_json::to_vec(session)
        .map_err(|_| "Failed to serialize Claude OAuth session".to_string())?;
    paths::atomic_write(&session_meta_path(), &payload)
        .map_err(|_| "Failed to save Claude OAuth session metadata".to_string())?;
    remove_logout_tombstone()
}

fn load_session_from_storage(app_handle: &tauri::AppHandle) -> Option<ClaudeOauthSession> {
    if logout_tombstone_path().exists() {
        return None;
    }

    let meta = std::fs::read(session_meta_path())
        .ok()
        .and_then(|raw| serde_json::from_slice::<ClaudeOauthSession>(&raw).ok())?;

    let refresh_token = app_handle
        .keyring()
        .get_password(KEYRING_SERVICE, SESSION_REFRESH_TOKEN_KEYRING_USER)
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| (!meta.refresh_token.trim().is_empty()).then(|| meta.refresh_token.clone()))?;

    Some(ClaudeOauthSession {
        refresh_token,
        ..meta
    })
}

fn save_session_to_storage(
    app_handle: &tauri::AppHandle,
    session: &ClaudeOauthSession,
) -> Result<(), String> {
    write_logout_tombstone()?;
    app_handle
        .keyring()
        .set_password(
            KEYRING_SERVICE,
            SESSION_REFRESH_TOKEN_KEYRING_USER,
            &session.refresh_token,
        )
        .map_err(|_| "Failed to save Claude OAuth refresh token".to_string())?;
    delete_keyring_password(
        app_handle,
        SESSION_KEYRING_USER,
        "delete Claude OAuth legacy session",
    )?;
    // Valid metadata commits last; an earlier failure leaves the intent marker.
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
        "delete Claude OAuth legacy session",
    ) {
        errors.push(error);
    }
    if let Err(error) = delete_keyring_password(
        app_handle,
        SESSION_REFRESH_TOKEN_KEYRING_USER,
        "delete Claude OAuth refresh token",
    ) {
        errors.push(error);
    }
    if errors.is_empty() {
        match std::fs::remove_file(session_meta_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => errors.push("Failed to delete Claude OAuth metadata".to_string()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn make_status(session: Option<&ClaudeOauthSession>) -> ClaudeOauthStatus {
    match session {
        Some(session) => ClaudeOauthStatus {
            logged_in: !session.access_token.trim().is_empty()
                || !session.refresh_token.trim().is_empty(),
            email: session.email.clone(),
            organization_name: session.organization_name.clone(),
            plan_type: session.plan_type.clone(),
            expires_at_ms: session.expires_at_ms,
        },
        None => ClaudeOauthStatus::default(),
    }
}

/// Claude access tokens carry the `sk-ant-oat` prefix; plain API keys do not.
pub fn is_claude_oauth_token(api_key: &str) -> bool {
    api_key.trim().starts_with("sk-ant-oat")
}

pub fn decode_claude_oauth_access_token(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if let Some(token) = trimmed.strip_prefix(CLAUDE_OAUTH_PREFIX) {
        let clean = token.trim();
        return (!clean.is_empty()).then(|| clean.to_string());
    }
    is_claude_oauth_token(trimmed).then(|| trimmed.to_string())
}

pub fn encode_claude_oauth_access_token(access_token: &str) -> Option<String> {
    let token = access_token.trim();
    (!token.is_empty()).then(|| format!("{CLAUDE_OAUTH_PREFIX}{token}"))
}

fn build_authorize_url(code_challenge: &str, state: &str) -> Result<String, String> {
    let query = form_encode(&[
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", SCOPE),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("state", state),
    ])?;
    Ok(format!("{AUTHORIZE_URL}?{query}"))
}

/// Anthropic shows `code#state`; accept a bare code too, and reject a state
/// that does not belong to the sign-in we started.
pub(super) fn parse_pasted_code(raw: &str, expected_state: &str) -> Result<String, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("Paste the code shown on the Claude sign-in page.".to_string());
    }
    let (code, state) = match text.split_once('#') {
        Some((code, state)) => (code.trim(), Some(state.trim())),
        None => (text, None),
    };
    if code.is_empty() {
        return Err("Paste the code shown on the Claude sign-in page.".to_string());
    }
    if let Some(state) = state {
        if !state.is_empty() && state != expected_state {
            return Err(
                "That code is from a different sign-in. Start the Claude sign-in again."
                    .to_string(),
            );
        }
    }
    Ok(code.to_string())
}

async fn exchange_code_for_tokens(
    client: &reqwest::Client,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let form = form_encode(&[
        ("client_id", CLIENT_ID),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", code_verifier),
    ])?;

    let response = client
        .post(TOKEN_URL)
        .timeout(TOKEN_HTTP_TIMEOUT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|err| format!("Failed to exchange the Claude sign-in code: {err}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Claude sign-in was rejected ({status}): {body}"));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| format!("Failed to read the Claude token response: {err}"))
}

async fn refresh_tokens(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<TokenResponse, RefreshFailure> {
    let form = form_encode(&[
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ])
    .map_err(|message| RefreshFailure {
        message,
        invalidate_session: false,
    })?;

    let response = client
        .post(TOKEN_URL)
        .timeout(TOKEN_HTTP_TIMEOUT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|err| RefreshFailure {
            message: format!("Failed to refresh the Claude sign-in: {err}"),
            invalidate_session: false,
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(RefreshFailure {
            message: format!("Claude token refresh failed ({status}): {body}"),
            invalidate_session: matches!(status.as_u16(), 400 | 401 | 403),
        });
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|err| RefreshFailure {
            message: format!("Failed to read the refreshed Claude token: {err}"),
            invalidate_session: false,
        })
}

fn session_needs_refresh(session: &ClaudeOauthSession) -> bool {
    match session.expires_at_ms {
        Some(expires_at_ms) => expires_at_ms <= now_ms().saturating_add(REFRESH_SKEW_SECS * 1000),
        // A session adopted from another tool carries no expiry; refresh once so
        // the next call has a real deadline instead of an untested token.
        None => true,
    }
}

pub fn sync_runtime_session(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Option<ClaudeOauthSession> {
    let login = state.claude_oauth_state().begin_login();
    let operation = login.token();
    let loaded = load_session_from_storage(app_handle);
    let result = match loaded {
        Some(session) => state
            .claude_oauth_state()
            .commit(operation, session, |_| Ok(())),
        None => Ok(false),
    };
    drop(login);
    if let Err(error) = result {
        log::warn!("Failed to restore Claude OAuth session: {error}");
    }
    state.read_claude_oauth_session()
}

pub async fn refresh_session_if_needed(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<Option<ClaudeOauthSession>, String> {
    let _refresh_guard = state.claude_oauth_state().lock_refresh().await;
    let Some((operation, session)) = state.claude_oauth_state().snapshot_for_refresh() else {
        return Ok(None);
    };

    if !session_needs_refresh(&session) && !session.access_token.trim().is_empty() {
        return Ok(Some(session));
    }

    if session.refresh_token.trim().is_empty() {
        return Err("The Claude sign-in expired. Connect Claude again in Settings.".to_string());
    }

    let token_response = match refresh_tokens(&state.http_client, &session.refresh_token).await {
        Ok(tokens) => tokens,
        Err(RefreshFailure {
            message,
            invalidate_session,
        }) => {
            if invalidate_session {
                let _ = state
                    .claude_oauth_state()
                    .invalidate(Some(operation), || clear_session_from_storage(app_handle));
            }
            return Err(message);
        }
    };

    let refreshed = ClaudeOauthSession {
        access_token: token_response.access_token,
        refresh_token: token_response
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(session.refresh_token),
        expires_at_ms: token_response
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000)),
        email: session.email,
        organization_name: session.organization_name,
        plan_type: session.plan_type,
    };

    let committed = state
        .claude_oauth_state()
        .commit(operation, refreshed.clone(), |session| {
            save_session_to_storage(app_handle, session)
        })?;

    if !committed {
        return Err("The Claude sign-in changed while refreshing. Please retry.".to_string());
    }

    Ok(Some(refreshed))
}

pub async fn resolve_oauth_origin_api_key(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let session = refresh_session_if_needed(app_handle, state)
        .await?
        .ok_or_else(|| "Connect Claude in Settings to use your subscription.".to_string())?;
    encode_claude_oauth_access_token(&session.access_token)
        .ok_or_else(|| "The Claude sign-in is missing an access token.".to_string())
}

pub fn status(state: &AppState) -> ClaudeOauthStatus {
    make_status(state.read_claude_oauth_session().as_ref())
}

/// Step 1: mint PKCE material, remember it, and hand the UI a URL to open.
pub fn start_login() -> Result<ClaudeOauthStart, String> {
    let (verifier, challenge) = generate_pkce_pair();
    let state_token = generate_state();
    let authorize_url = build_authorize_url(&challenge, &state_token)?;
    *PENDING_LOGIN.lock() = Some(PendingLogin {
        verifier,
        state: state_token.clone(),
        started_at: Instant::now(),
    });
    Ok(ClaudeOauthStart {
        authorize_url,
        state: state_token,
    })
}

/// Step 2: exchange the code the browser page displayed.
pub async fn complete_login(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    pasted_code: &str,
) -> Result<ClaudeOauthStatus, String> {
    let pending = PENDING_LOGIN
        .lock()
        .take()
        .ok_or_else(|| "Start the Claude sign-in again.".to_string())?;
    if pending.started_at.elapsed() > PENDING_LOGIN_TTL {
        return Err("The Claude sign-in timed out. Start it again.".to_string());
    }

    let code = parse_pasted_code(pasted_code, &pending.state)?;
    let tokens = exchange_code_for_tokens(&state.http_client, &code, &pending.verifier).await?;

    let login = state.claude_oauth_state().begin_login_after_refresh().await;
    let operation = login.token();
    let session = ClaudeOauthSession {
        access_token: tokens.access_token,
        refresh_token: tokens
            .refresh_token
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Claude did not return a refresh token. Sign in again.".to_string())?,
        expires_at_ms: tokens
            .expires_in
            .map(|expires_in| now_ms().saturating_add(expires_in * 1000)),
        email: None,
        organization_name: None,
        plan_type: None,
    };

    let status = make_status(Some(&session));
    let committed = state
        .claude_oauth_state()
        .commit(operation, session, |session| {
            save_session_to_storage(app_handle, session)
        })?;
    if !committed {
        return Err("The Claude sign-in was superseded. Please try again.".to_string());
    }
    Ok(status)
}

pub async fn logout(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    *PENDING_LOGIN.lock() = None;
    let _refresh_guard = state.claude_oauth_state().lock_refresh().await;
    state
        .claude_oauth_state()
        .invalidate(None, || clear_session_from_storage(app_handle))?;
    Ok(())
}

/// Adopt a sign-in that Claude Code or Magpie already holds on this machine.
/// Only runs when the user asks for it.
pub fn sync_from_system(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<ClaudeOauthStatus, String> {
    let session = read_system_session()?
        .ok_or("No Claude sign-in found in Claude Code or Magpie on this machine.")?;

    let login = state.claude_oauth_state().begin_login();
    let operation = login.token();
    let status = make_status(Some(&session));
    let committed = state
        .claude_oauth_state()
        .commit(operation, session, |session| {
            save_session_to_storage(app_handle, session)
        })?;
    if !committed {
        return Err("The Claude sign-in changed while syncing. Please retry.".to_string());
    }
    Ok(status)
}

fn read_system_session() -> Result<Option<ClaudeOauthSession>, String> {
    let home = dirs::home_dir().ok_or("Cannot locate the user home directory")?;

    // Claude Code's own credential file, then Magpie's account list.
    let candidates = [
        home.join(".claude").join(".credentials.json"),
        home.join(".config").join("magpie").join("logins.json"),
    ];
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if let Some(session) = session_from_json(&json) {
            return Ok(Some(session));
        }
    }
    Ok(None)
}

pub(super) fn session_from_json(json: &serde_json::Value) -> Option<ClaudeOauthSession> {
    // Claude Code: { "claudeAiOauth": { accessToken, refreshToken, expiresAt, subscriptionType } }
    if let Some(oauth) = json.get("claudeAiOauth") {
        if let Some(session) = session_from_oauth_entry(oauth, None) {
            return Some(session);
        }
    }
    // Magpie: [ { agent: "claude", auth: { claudeAiOauth: {...} }, profile: {...} } ]
    for entry in json.as_array().into_iter().flatten() {
        if entry.get("agent").and_then(|value| value.as_str()) != Some("claude") {
            continue;
        }
        // Prefer an account that is switched on.
        if entry.get("on").and_then(|value| value.as_bool()) == Some(false) {
            continue;
        }
        let oauth = entry
            .get("auth")
            .and_then(|auth| auth.get("claudeAiOauth"))?;
        let profile = entry.get("profile");
        if let Some(session) = session_from_oauth_entry(oauth, profile) {
            return Some(session);
        }
    }
    None
}

fn session_from_oauth_entry(
    oauth: &serde_json::Value,
    profile: Option<&serde_json::Value>,
) -> Option<ClaudeOauthSession> {
    let access_token = oauth.get("accessToken")?.as_str()?.trim().to_string();
    let refresh_token = oauth.get("refreshToken")?.as_str()?.trim().to_string();
    if access_token.is_empty() || refresh_token.is_empty() {
        return None;
    }
    Some(ClaudeOauthSession {
        access_token,
        refresh_token,
        expires_at_ms: oauth.get("expiresAt").and_then(|value| value.as_u64()),
        email: profile
            .and_then(|profile| profile.get("emailAddress"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        organization_name: profile
            .and_then(|profile| profile.get("organizationName"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        plan_type: oauth
            .get("subscriptionType")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

/// A stored choice wins; otherwise use the subscription when one is connected.
pub fn effective_claude_auth_mode(
    stored: Option<ClaudeAuthMode>,
    logged_in: bool,
) -> ClaudeAuthMode {
    match stored {
        Some(ClaudeAuthMode::ApiKey) => ClaudeAuthMode::ApiKey,
        Some(ClaudeAuthMode::Oauth) => ClaudeAuthMode::Oauth,
        None if logged_in => ClaudeAuthMode::Oauth,
        None => ClaudeAuthMode::ApiKey,
    }
}
