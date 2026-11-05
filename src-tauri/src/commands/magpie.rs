use crate::services::magpie_service;
use crate::state::AppState;
use crate::utils::AppError;

/// Whether Magpie is installed or answering on this machine. The settings page
/// uses it to decide if the provider is offered at all.
#[tauri::command]
pub async fn detect_magpie(
    state: tauri::State<'_, AppState>,
) -> Result<magpie_service::MagpieStatus, AppError> {
    Ok(magpie_service::detect(&state.http_client).await)
}
