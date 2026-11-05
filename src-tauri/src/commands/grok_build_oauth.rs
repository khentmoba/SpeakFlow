use crate::services::grok_build_oauth_service;
use crate::state::AppState;
use crate::utils::AppError;

#[tauri::command]
pub async fn login_grok_build_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, AppError> {
    grok_build_oauth_service::login(&app_handle, state.inner())
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn start_grok_build_oauth_device_code(
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthDeviceCodeChallenge, AppError> {
    grok_build_oauth_service::start_device_code_login(state.inner())
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn complete_grok_build_oauth_device_code(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    challenge: grok_build_oauth_service::GrokBuildOauthDeviceCodeChallenge,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, AppError> {
    grok_build_oauth_service::complete_device_code_login(&app_handle, state.inner(), challenge)
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn logout_grok_build_oauth(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    grok_build_oauth_service::logout(&app_handle, state.inner()).map_err(AppError::Other)
}

#[tauri::command]
pub async fn get_grok_build_oauth_status(
    state: tauri::State<'_, AppState>,
) -> Result<grok_build_oauth_service::GrokBuildOauthStatus, AppError> {
    Ok(grok_build_oauth_service::status(state.inner()))
}
