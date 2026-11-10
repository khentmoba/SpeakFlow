use crate::utils::AppError;
use std::time::Instant;

use tauri::Emitter;

use crate::services::{
    ai_polish_service, alibaba_asr_service, funasr_service, glm_asr_service, history_service,
};
use crate::state::AppState;
use crate::utils::{foreground, paths};

fn emit_history_updated(app_handle: &tauri::AppHandle, id: Option<i64>) {
    let _ = app_handle.emit("history-updated", serde_json::json!({ "id": id }));
}

#[tauri::command]
pub async fn list_transcription_history(
    filter: history_service::HistoryQuery,
) -> Result<history_service::HistoryPage, AppError> {
    history_service::list(filter).await.map_err(AppError::Other)
}

#[tauri::command]
pub async fn get_transcription_history_stats() -> Result<history_service::HistoryStats, AppError> {
    history_service::stats().await.map_err(AppError::Other)
}

#[tauri::command]
pub async fn search_transcripts(
    query: String,
    kind: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<crate::services::search_service::SearchDocument>, AppError> {
    crate::services::search_service::search_all(
        &query,
        kind.as_deref().unwrap_or("all"),
        limit.unwrap_or(40),
    )
    .await
    .map_err(AppError::Other)
}

#[tauri::command]
pub async fn delete_transcription_history(
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<bool, AppError> {
    let removed = history_service::delete(id).await?;
    if removed {
        emit_history_updated(&app_handle, Some(id));
    }
    Ok(removed)
}

fn export_markdown(records: &[history_service::HistoryRecord]) -> String {
    let mut output = String::from("# SpeakFlow Transcription History\n\n");
    for record in records {
        let timestamp = chrono::DateTime::from_timestamp_millis(record.created_at)
            .map(|value| {
                value
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| record.created_at.to_string());
        output.push_str(&format!("## {} · {}\n\n", timestamp, record.mode));
        if let Some(process) = record.app_process.as_deref() {
            output.push_str(&format!("- App: {}\n", process));
        }
        output.push_str(&format!("- Status: {}\n", record.status));
        output.push_str(&format!("- Engine: {}\n", record.engine));
        if let (Some(provider), Some(model)) = (record.provider.as_deref(), record.model.as_deref())
        {
            output.push_str(&format!("- Processing model: {} / {}\n", provider, model));
        }
        if let Some(total_ms) = record.total_ms {
            output.push_str(&format!("- Total time: {} ms\n", total_ms));
        }
        output.push('\n');
        let source_text = record
            .source_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(source_text) = source_text {
            if !record.original_text.trim().is_empty() {
                output.push_str("**Original ASR**\n\n");
                output.push_str(&record.original_text);
                output.push_str("\n\n");
            }
            output.push_str("**Edited source**\n\n");
            output.push_str(source_text);
            output.push_str("\n\n**Edited result**\n\n");
        } else if !record.original_text.trim().is_empty() && record.original_text != record.text {
            output.push_str("**Original ASR**\n\n");
            output.push_str(&record.original_text);
            output.push_str("\n\n**Final text**\n\n");
        }
        if record.text.trim().is_empty() {
            output.push_str(record.error.as_deref().unwrap_or("No text result"));
        } else {
            output.push_str(&record.text);
        }
        output.push_str("\n\n---\n\n");
    }
    output
}

#[tauri::command]
pub async fn export_transcription_history(
    window: tauri::WebviewWindow,
    format: String,
) -> Result<Option<String>, AppError> {
    let records = history_service::all_records().await?;
    let normalized_format = format.trim().to_ascii_lowercase();
    let (extension, filter_label, data) = match normalized_format.as_str() {
        "markdown" | "md" => ("md", "Markdown", export_markdown(&records).into_bytes()),
        "json" => (
            "json",
            "JSON",
            serde_json::to_vec_pretty(&records)
                .map_err(|error| format!("Failed to serialize history: {error}"))?,
        ),
        "csv" => (
            "csv",
            "CSV",
            crate::services::history_export::csv(&records).into_bytes(),
        ),
        "srt" => (
            "srt",
            "SubRip subtitles",
            crate::services::history_export::srt(&records).into_bytes(),
        ),
        "vtt" => (
            "vtt",
            "WebVTT subtitles",
            crate::services::history_export::vtt(&records).into_bytes(),
        ),
        _ => {
            return Err(AppError::Other(
                "History export format must be JSON, Markdown, CSV, SRT or VTT".into(),
            ))
        }
    };
    let file_name = format!("speak-flow-history.{extension}");
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter(filter_label, &[extension])
        .set_file_name(file_name);
    if let Some(directory) = dirs::download_dir() {
        dialog = dialog.set_directory(directory);
    }
    let selected = dialog.save_file().await;
    let Some(path) = selected else {
        return Ok(None);
    };
    tokio::fs::write(path.path(), data)
        .await
        .map_err(|error| format!("Failed to write history export file: {error}"))?;
    Ok(Some(paths::strip_win_prefix(path.path())))
}

async fn transcribe_saved_audio(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    audio_file: &str,
) -> Result<funasr_service::TranscriptionResult, String> {
    let audio = history_service::read_audio(audio_file).await?;
    let engine = paths::read_engine_config();
    let result = match engine.as_str() {
        "alibaba-asr" => alibaba_asr_service::transcribe(state, audio).await,
        "glm-asr" => glm_asr_service::transcribe(state, audio).await,
        _ => funasr_service::transcribe(state, audio, app_handle).await,
    }
    .map_err(|error| format!("Transcription retry failed: {error}"))?;
    if result.success {
        Ok(result)
    } else {
        Err(result
            .error
            .unwrap_or_else(|| "Transcription retry failed".into()))
    }
}

fn ensure_reprocessable_workflow(workflow: &str) -> Result<(), String> {
    if workflow == "dictation" {
        Ok(())
    } else {
        Err("Assistant and editing history cannot be reprocessed as ordinary dictation".into())
    }
}

async fn reprocess_stored_history(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    stored: history_service::StoredHistoryRecord,
    kind: &str,
) -> Result<history_service::HistoryRecord, String> {
    ensure_reprocessable_workflow(&stored.record.workflow)?;
    let started = Instant::now();
    let mut asr_ms = None;
    let mut language = stored.record.language.clone();
    let original_text = if kind == "asr" {
        let audio_file = stored.audio_file.as_deref().ok_or_else(|| {
            "This record has no saved audio and cannot be transcribed again".to_string()
        })?;
        let asr_started = Instant::now();
        let result = transcribe_saved_audio(app_handle, state, audio_file).await?;
        asr_ms = Some(asr_started.elapsed().as_millis().min(u64::MAX as u128) as u64);
        language = result.language;
        result.text.trim().to_string()
    } else if !stored.record.original_text.trim().is_empty() {
        stored.record.original_text.trim().to_string()
    } else {
        stored.record.text.trim().to_string()
    };
    if original_text.is_empty() {
        return Err("This record has no text to reprocess".into());
    }

    let process_name = stored.record.app_process.as_deref().unwrap_or_default();
    let window_title = stored
        .record
        .app_window_title
        .as_deref()
        .unwrap_or_default();
    let resolved =
        state.with_profile(|profile| profile.resolve_app_profile(process_name, window_title));
    let app_context = foreground::prompt_context_from_parts(process_name, window_title);
    let polish_started = Instant::now();
    let polish_outcome = ai_polish_service::polish_text_with_overrides_detailed(
        state,
        &original_text,
        app_handle,
        0,
        ai_polish_service::PolishOverrides {
            ai_polish_enabled: if kind == "polish" {
                Some(true)
            } else {
                resolved.ai_polish_enabled
            },
            translation_target: resolved.translation_target.clone(),
            custom_prompt: resolved.custom_prompt.clone(),
            tone_mode_override: resolved.tone_mode.clone(),
            app_vocabulary: resolved.vocabulary.clone(),
            screen_context_enabled: Some(false),
            screen_context_explicit: false,
            screen_context_foreground: None,
            app_context,
            emit_status: false,
            learn_from_result: false,
            allow_jev_gate: false,
            require_execution: kind == "polish",
        },
    )
    .await
    .map_err(|error| format!("Re-polishing failed: {error}"))?;
    let polish_ms = polish_outcome
        .executed
        .then_some(polish_started.elapsed().as_millis().min(u64::MAX as u128) as u64);
    let total_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);

    let retention_days = state.with_profile(|profile| profile.history_settings.retention_days);
    let new_id = history_service::insert(
        history_service::HistoryDraft {
            session_id: stored.record.session_id,
            mode: stored.record.mode.clone(),
            workflow: "dictation".into(),
            status: "success".into(),
            text: polish_outcome.text,
            original_text,
            source_text: None,
            duration_sec: stored.record.duration_sec,
            language,
            engine: if kind == "asr" {
                paths::read_engine_config()
            } else {
                stored.record.engine.clone()
            },
            provider: polish_outcome.provider,
            model: polish_outcome.model,
            app_process: stored.record.app_process.clone(),
            app_window_title: stored.record.app_window_title.clone(),
            app_rule_name: resolved.rule_name,
            audio_file: stored.audio_file,
            asr_ms,
            polish_ms,
            total_ms,
            error: None,
            reprocessed_from_id: Some(stored.record.id),
        },
        retention_days,
    )
    .await?;
    let record = history_service::get(new_id)
        .await?
        .map(|stored| stored.record)
        .ok_or_else(|| {
            "Reprocessing completed, but the new history record could not be read".to_string()
        })?;
    Ok(record)
}

#[tauri::command]
pub async fn reprocess_transcription_history(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    kind: String,
) -> Result<history_service::HistoryRecord, AppError> {
    let kind = kind.trim().to_ascii_lowercase();
    if kind != "polish" && kind != "asr" {
        return Err(AppError::Other(
            "Reprocessing mode must be polish or asr".into(),
        ));
    }

    let stored = history_service::get_for_reprocess(id)
        .await?
        .ok_or_else(|| "History record to reprocess was not found".to_string())?;
    let leased_audio = stored.audio_file.clone();
    let result = reprocess_stored_history(&app_handle, state.inner(), stored, &kind).await;

    if let Some(audio_file) = leased_audio {
        if let Err(error) = history_service::release_audio_lease(audio_file).await {
            // 租约只会延迟文件回收；下次启动会清除崩溃/异常遗留租约。
            log::warn!("释放历史重处理音频租约失败: {error}");
        }
    }

    if let Ok(record) = result.as_ref() {
        emit_history_updated(&app_handle, Some(record.id));
    }
    result.map_err(AppError::Other)
}

pub async fn persist_history_insert(
    app_handle: &tauri::AppHandle,
    draft: history_service::HistoryDraft,
    retention_days: u32,
) -> Result<i64, String> {
    let app_handle = app_handle.clone();
    let audio_file = draft.audio_file.clone();
    match history_service::insert(draft, retention_days).await {
        Ok(id) => {
            emit_history_updated(&app_handle, Some(id));
            Ok(id)
        }
        Err(error) => {
            if let Some(audio_file) = audio_file {
                if let Err(cleanup_error) =
                    history_service::cleanup_audio_if_unreferenced(audio_file).await
                {
                    log::warn!("回收未入库历史音频失败，将在下次启动重试: {cleanup_error}");
                }
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_reprocessable_workflow, export_markdown};
    use crate::services::history_service::HistoryRecord;

    #[test]
    fn only_plain_dictation_history_can_be_reprocessed() {
        assert!(ensure_reprocessable_workflow("dictation").is_ok());
        assert!(ensure_reprocessable_workflow("assistant").is_err());
        assert!(ensure_reprocessable_workflow("edit").is_err());
    }

    #[test]
    fn edit_export_keeps_instruction_source_and_actual_model_separate() {
        let output = export_markdown(&[HistoryRecord {
            id: 1,
            session_id: 2,
            created_at: 1_700_000_000_000,
            updated_at: 1_700_000_000_000,
            mode: "dictation".into(),
            workflow: "edit".into(),
            status: "success".into(),
            text: "这个方案目前还不够理想。".into(),
            original_text: "把它写得更礼貌".into(),
            source_text: Some("这个方案不行。".into()),
            duration_sec: Some(1.2),
            language: Some("zh".into()),
            engine: "sensevoice".into(),
            provider: Some("openai".into()),
            model: Some("gpt-test".into()),
            app_process: Some("Code.exe".into()),
            app_window_title: None,
            app_rule_name: None,
            audio_available: false,
            asr_ms: Some(100),
            polish_ms: Some(200),
            total_ms: Some(300),
            error: None,
            reprocessed_from_id: None,
        }]);

        assert!(output.contains("- Processing model: openai / gpt-test"));
        assert!(output.contains("**Original ASR**\n\n把它写得更礼貌"));
        assert!(output.contains("**Edited source**\n\n这个方案不行。"));
        assert!(output.contains("**Edited result**\n\n这个方案目前还不够理想。"));
    }
}
