use crate::services::claude_oauth_service;
use crate::state::AppState;
use crate::utils::AppError;

#[tauri::command]
pub async fn start_claude_oauth() -> Result<claude_oauth_service::ClaudeOauthStart, AppError> {
    claude_oauth_service::start_login().map_err(AppError::Other)
}

#[tauri::command]
pub async fn complete_claude_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    code: String,
) -> Result<claude_oauth_service::ClaudeOauthStatus, AppError> {
    claude_oauth_service::complete_login(&app_handle, state.inner(), &code)
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn logout_claude_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    claude_oauth_service::logout(&app_handle, state.inner())
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn get_claude_oauth_status(
    state: tauri::State<'_, AppState>,
) -> Result<claude_oauth_service::ClaudeOauthStatus, AppError> {
    Ok(claude_oauth_service::status(state.inner()))
}

#[tauri::command]
pub async fn sync_claude_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<claude_oauth_service::ClaudeOauthStatus, AppError> {
    claude_oauth_service::sync_from_system(&app_handle, state.inner()).map_err(AppError::Other)
}
