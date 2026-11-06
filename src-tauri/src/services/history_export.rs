//! History export formats.
//!
//! JSON and Markdown keep every field for a backup. The subtitle and table
//! formats are for handing a transcript to another tool, so they carry the
//! fields those tools read and nothing else.
//!
//! Subtitle timing is relative to the first exported record: the first cue
//! starts at zero and each later cue starts at its distance from that record.
//! A record without a measured duration gets two seconds, the shortest span that
//! stays readable in a player.

use crate::services::history_service::HistoryRecord;

/// A record with no measured duration still needs a cue length.
const DEFAULT_CUE_SECONDS: f64 = 2.0;

fn escape_csv(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

fn timestamp(millis: i64, separator: char) -> String {
    let total = millis.max(0);
    let hours = total / 3_600_000;
    let minutes = (total % 3_600_000) / 60_000;
    let seconds = (total % 60_000) / 1000;
    let millis = total % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{millis:03}")
}

/// One row per record, with the fields a spreadsheet can filter on.
pub fn csv(records: &[HistoryRecord]) -> String {
    let mut out = String::from(
        "id,createdAt,mode,workflow,status,app,durationSec,language,engine,provider,model,text,originalText\n",
    );
    for record in records {
        let created = chrono::DateTime::from_timestamp_millis(record.created_at)
            .map(|value| value.to_rfc3339())
            .unwrap_or_default();
        let row = [
            record.id.to_string(),
            created,
            record.mode.clone(),
            record.workflow.clone(),
            record.status.clone(),
            record.app_process.clone().unwrap_or_default(),
            record
                .duration_sec
                .map(|value| format!("{value:.2}"))
                .unwrap_or_default(),
            record.language.clone().unwrap_or_default(),
            record.engine.clone(),
            record.provider.clone().unwrap_or_default(),
            record.model.clone().unwrap_or_default(),
            record.text.clone(),
            record.original_text.clone(),
        ];
        out.push_str(&row.map(|field| escape_csv(&field)).join(","));
        out.push('\n');
    }
    out
}

/// Cue spans in milliseconds, relative to the first record.
fn cue_spans(records: &[HistoryRecord]) -> Vec<(i64, i64)> {
    let Some(first) = records.first() else {
        return Vec::new();
    };
    records
        .iter()
        .map(|record| {
            let start = (record.created_at - first.created_at).max(0);
            let seconds = record
                .duration_sec
                .filter(|value| *value > 0.0)
                .unwrap_or(DEFAULT_CUE_SECONDS);
            let length = (seconds * 1000.0).round().max(500.0) as i64;
            (start, start + length)
        })
        .collect()
}

/// SubRip subtitles.
pub fn srt(records: &[HistoryRecord]) -> String {
    let mut out = String::new();
    for (index, (record, (start, end))) in records.iter().zip(cue_spans(records)).enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            timestamp(start, ','),
            timestamp(end, ','),
            record.text.trim()
        ));
    }
    out
}

/// WebVTT subtitles.
pub fn vtt(records: &[HistoryRecord]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (record, (start, end)) in records.iter().zip(cue_spans(records)) {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            timestamp(start, '.'),
            timestamp(end, '.'),
            record.text.trim()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, created_at: i64, text: &str, duration: Option<f64>) -> HistoryRecord {
        HistoryRecord {
            id,
            session_id: id as u64,
            created_at,
            updated_at: created_at,
            mode: "dictation".into(),
            workflow: "dictation".into(),
            status: "success".into(),
            text: text.into(),
            original_text: text.into(),
            source_text: None,
            duration_sec: duration,
            language: Some("en".into()),
            engine: "qwen3-asr".into(),
            provider: None,
            model: None,
            app_process: Some("Code.exe".into()),
            app_window_title: None,
            app_rule_name: None,
            audio_available: false,
            asr_ms: None,
            polish_ms: None,
            total_ms: None,
            error: None,
            reprocessed_from_id: None,
        }
    }

    #[test]
    fn csv_quotes_fields_that_contain_separators() {
        let records = vec![record(1, 0, "He said \"hi\", then left", Some(1.5))];
        let out = csv(&records);
        assert!(out.contains("\"He said \"\"hi\"\", then left\""));
        assert!(out.starts_with("id,createdAt,mode"));
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn srt_cues_start_at_zero_and_follow_the_record_spacing() {
        let records = vec![
            record(1, 1_700_000_000_000, "first", Some(2.0)),
            record(2, 1_700_000_005_000, "second", Some(1.0)),
        ];
        let out = srt(&records);
        assert!(out.starts_with("1\n00:00:00,000 --> 00:00:02,000\nfirst\n"));
        assert!(out.contains("2\n00:00:05,000 --> 00:00:06,000\nsecond\n"));
    }

    #[test]
    fn a_record_without_a_duration_gets_the_default_cue() {
        let records = vec![record(1, 0, "only", None)];
        assert!(srt(&records).contains("00:00:00,000 --> 00:00:02,000"));
    }

    #[test]
    fn vtt_has_a_header_and_dot_separated_timestamps() {
        let records = vec![record(1, 0, "hello", Some(1.0))];
        let out = vtt(&records);
        assert!(out.starts_with("WEBVTT\n\n"));
        assert!(out.contains("00:00:00.000 --> 00:00:01.000\nhello"));
    }

    #[test]
    fn empty_history_produces_empty_files() {
        assert_eq!(csv(&[]).lines().count(), 1);
        assert_eq!(srt(&[]), "");
        assert_eq!(vtt(&[]), "WEBVTT\n\n");
    }
}
