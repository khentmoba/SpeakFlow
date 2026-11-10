//! Tauri commands for Meetings: dual-track capture, call detection,
//! listing, Markdown/SRT/VTT/JSON export, re-summarization, and configuration.

use tauri::State;

use crate::services::meeting_service::{
    self, CallDetectionResult, MeetingRecord, MeetingStatusPayload,
};
use crate::services::profile_service;
use crate::state::user_profile::MeetingsConfig;
use crate::state::AppState;
use crate::utils::AppError;

#[tauri::command]
pub async fn get_meeting_status() -> Result<MeetingStatusPayload, AppError> {
    Ok(meeting_service::current_status())
}

#[tauri::command]
pub async fn detect_meeting_call(
    state: State<'_, AppState>,
) -> Result<CallDetectionResult, AppError> {
    let config = state.with_profile(|p| p.meetings.clone());
    tokio::task::spawn_blocking(move || meeting_service::detect_active_call(&config))
        .await
        .map_err(|e| AppError::Other(format!("Call detection failed: {e}")))
}

#[tauri::command]
pub async fn start_meeting_recording(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    app_name: Option<String>,
) -> Result<MeetingStatusPayload, AppError> {
    meeting_service::start_meeting(&app_handle, state.inner(), title, app_name)
        .map_err(AppError::Audio)
}

#[tauri::command]
pub async fn stop_meeting_recording(
    app_handle: tauri::AppHandle,
) -> Result<MeetingRecord, AppError> {
    meeting_service::stop_meeting(&app_handle)
        .await
        .map_err(AppError::Audio)
}

#[tauri::command]
pub async fn list_meetings() -> Result<Vec<MeetingRecord>, AppError> {
    tokio::task::spawn_blocking(meeting_service::load_meetings)
        .await
        .map_err(|e| AppError::Other(format!("Failed to list meetings: {e}")))
}

#[tauri::command]
pub async fn get_meeting(id: String) -> Result<Option<MeetingRecord>, AppError> {
    tokio::task::spawn_blocking(move || meeting_service::get_meeting_by_id(&id))
        .await
        .map_err(|e| AppError::Other(format!("Failed to read meeting: {e}")))
}

#[tauri::command]
pub async fn delete_meeting(id: String) -> Result<bool, AppError> {
    tokio::task::spawn_blocking(move || meeting_service::delete_meeting_by_id(&id))
        .await
        .map_err(|e| AppError::Other(format!("Failed to delete meeting: {e}")))?
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn export_meeting(
    window: tauri::WebviewWindow,
    id: String,
    format: String,
    save_to_disk: Option<bool>,
) -> Result<String, AppError> {
    let record = tokio::task::spawn_blocking(move || meeting_service::get_meeting_by_id(&id))
        .await
        .map_err(|e| AppError::Other(format!("Failed to read meeting: {e}")))?
        .ok_or_else(|| AppError::Other("Meeting not found".to_string()))?;

    let content =
        meeting_service::export_meeting_format(&record, &format).map_err(AppError::Other)?;

    if save_to_disk.unwrap_or(false) {
        let ext = match format.trim().to_ascii_lowercase().as_str() {
            "srt" => "srt",
            "vtt" => "vtt",
            "json" => "json",
            _ => "md",
        };
        let default_name = format!("{}.{ext}", record.id);
        let picked = rfd::AsyncFileDialog::new()
            .set_title("Export Meeting Transcript")
            .set_file_name(&default_name)
            .add_filter(ext.to_uppercase(), &[ext])
            .set_parent(&window)
            .save_file()
            .await;
        if let Some(handle) = picked {
            std::fs::write(handle.path(), content.as_bytes())
                .map_err(|e| AppError::Other(format!("Failed to write export file: {e}")))?;
            return Ok(handle.path().to_string_lossy().to_string());
        }
        return Ok(String::new());
    }

    Ok(content)
}

#[tauri::command]
pub async fn reprocess_meeting(
    app_handle: tauri::AppHandle,
    id: String,
) -> Result<MeetingRecord, AppError> {
    meeting_service::resummarize_meeting(&app_handle, &id)
        .await
        .map_err(AppError::Other)
}

#[tauri::command]
pub async fn set_meetings_config(
    state: State<'_, AppState>,
    meetings: MeetingsConfig,
) -> Result<(), AppError> {
    let mut sanitized = meetings;
    sanitized.keep_audio_days = sanitized.keep_audio_days.min(3650);
    sanitized.transcript_font_size = sanitized.transcript_font_size.clamp(10, 24);
    let expire_flag = state.with_profile(|p| p.labs.expire_meeting_audio);
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.meetings = sanitized.clone();
    });
    meeting_service::cleanup_expired_meeting_audio(&sanitized, expire_flag);
    Ok(())
}
