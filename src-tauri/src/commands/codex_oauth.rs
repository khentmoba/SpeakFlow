use crate::services::codex_oauth_service;
use crate::state::AppState;
use crate::utils::AppError;

#[tauri::command]
pub async fn login_openai_codex_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    client_id: Option<String>,
    new_account: Option<bool>,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, AppError> {
    codex_oauth_service::login(
        &app_handle,
        state.inner(),
        client_id.as_deref(),
        new_account.unwrap_or(false),
    )
    .await
    .map_err(AppError::Other)
}

#[tauri::command]
pub async fn remove_chatgpt_account(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    client_id: String,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, AppError> {
    codex_oauth_service::remove_account(&app_handle, state.inner(), &client_id)
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn start_openai_codex_oauth_device_code(
    state: tauri::State<'_, AppState>,
) -> Result<codex_oauth_service::OpenaiCodexOauthDeviceCodeChallenge, AppError> {
    codex_oauth_service::start_device_code_login(state.inner())
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn complete_openai_codex_oauth_device_code(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    challenge: codex_oauth_service::OpenaiCodexOauthDeviceCodeChallenge,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, AppError> {
    codex_oauth_service::complete_device_code_login(&app_handle, state.inner(), challenge)
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn logout_openai_codex_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    codex_oauth_service::logout(&app_handle, state.inner())
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn get_openai_codex_oauth_status(
    state: tauri::State<'_, AppState>,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, AppError> {
    Ok(codex_oauth_service::status(state.inner()))
}

#[tauri::command]
pub async fn sync_openai_codex_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<codex_oauth_service::OpenaiCodexOauthStatus, AppError> {
    codex_oauth_service::sync_from_system(&app_handle, state.inner()).map_err(AppError::Other)
}
