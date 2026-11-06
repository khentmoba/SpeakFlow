use super::{
    engine_install_fingerprint_matches, read_json_response, read_json_response_matching,
    server_response_to_transcription_result, EngineProgressGate, ServerCommand, ServerResponse,
    StartingFlagGuard, ENGINE_ARCHIVE_FINGERPRINT,
};
use crate::state::user_profile::{CorrectionPattern, CorrectionSource, HotWord, HotWordSource};
use crate::state::AppState;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};

async fn read_response_from_chunks(chunks: &[&[u8]]) -> ServerResponse {
    let (mut writer, reader) = tokio::io::duplex(1024);
    for chunk in chunks {
        writer.write_all(chunk).await.unwrap();
    }
    writer.shutdown().await.unwrap();

    let mut reader = BufReader::new(reader);
    read_json_response(&mut reader, Duration::from_secs(1), "test")
        .await
        .unwrap()
}

#[test]
fn server_command_serializes_flat_optional_audio_fields() {
    let inline = serde_json::to_value(ServerCommand::Transcribe {
        options: Some(crate::state::user_profile::R2T2Config {
            context: "domain context".into(),
            language: Some("de".into()),
        }),
        audio_path: None,
        audio_base64: Some("AQI=".to_string()),
        audio_format: Some("pcm_s16le".to_string()),
        sample_rate: Some(16_000),
        hot_words: Some(vec!["speak-flow".to_string()]),
        language: Some("ca".to_string()),
        translate_to_english: Some(true),
        filter_silence: Some(false),
    })
    .unwrap();

    assert_eq!(inline["action"], serde_json::json!("transcribe"));
    assert_eq!(inline["options"]["context"], "domain context");
    assert_eq!(inline["options"]["language"], "de");
    assert_eq!(inline["audio_base64"], serde_json::json!("AQI="));
    assert_eq!(inline["audio_format"], serde_json::json!("pcm_s16le"));
    assert_eq!(inline["sample_rate"], serde_json::json!(16_000));
    assert_eq!(inline["hot_words"], serde_json::json!(["speak-flow"]));
    assert_eq!(inline["language"], serde_json::json!("ca"));
    assert_eq!(inline["translate_to_english"], serde_json::json!(true));
    assert_eq!(inline["filter_silence"], serde_json::json!(false));
    assert!(inline.get("audio_path").is_none());

    let idle = serde_json::to_value(ServerCommand::SetGpuIdle { seconds: 0 }).unwrap();
    assert_eq!(idle["action"], serde_json::json!("set_gpu_idle"));
    assert_eq!(idle["seconds"], serde_json::json!(0));
    let idle = serde_json::to_value(ServerCommand::SetGpuIdle { seconds: 180 }).unwrap();
    assert_eq!(idle["seconds"], serde_json::json!(180));

    let path = serde_json::to_value(ServerCommand::Transcribe {
        options: None,
        audio_path: Some("C:/tmp/clip.wav".to_string()),
        audio_base64: None,
        audio_format: None,
        sample_rate: None,
        hot_words: None,
        language: None,
        translate_to_english: None,
        filter_silence: None,
    })
    .unwrap();

    assert_eq!(path["action"], serde_json::json!("transcribe"));
    assert!(path.get("options").is_none());
    assert_eq!(path["audio_path"], serde_json::json!("C:/tmp/clip.wav"));
    assert!(path.get("audio_base64").is_none());
    assert!(path.get("audio_format").is_none());
    assert!(path.get("filter_silence").is_none());
    assert!(path.get("sample_rate").is_none());
    assert!(path.get("hot_words").is_none());
    assert!(path.get("language").is_none());
    assert!(path.get("translate_to_english").is_none());
}

#[test]
fn engine_install_fingerprint_reuses_legacy_versioned_archive_marker() {
    let legacy = format!("1.3.13+{}", ENGINE_ARCHIVE_FINGERPRINT);

    assert!(engine_install_fingerprint_matches(
        &legacy,
        ENGINE_ARCHIVE_FINGERPRINT
    ));
    assert!(engine_install_fingerprint_matches(
        ENGINE_ARCHIVE_FINGERPRINT,
        ENGINE_ARCHIVE_FINGERPRINT
    ));
}

#[test]
fn engine_install_fingerprint_rejects_a_different_archive() {
    assert!(!engine_install_fingerprint_matches(
        "1.3.13+0000000000000001-0000000000000002",
        ENGINE_ARCHIVE_FINGERPRINT
    ));
}

#[test]
fn local_engine_responses_use_manual_vocabulary_with_a_full_learned_list() {
    let state = AppState::new();
    state.update_profile_mut(|profile| {
        for index in 0..100 {
            profile.hot_words.push(HotWord {
                text: format!("Learned{index}"),
                weight: 5,
                source: HotWordSource::Learned,
                use_count: 100,
                last_used: 0,
            });
        }
        for term in ["C++", "C#", "José"] {
            profile.hot_words.push(HotWord {
                text: term.into(),
                weight: 3,
                source: HotWordSource::User,
                use_count: 0,
                last_used: 0,
            });
        }
        profile.hot_words.push(HotWord {
            text: "github".to_string(),
            weight: 3,
            source: HotWordSource::User,
            use_count: 10,
            last_used: 0,
        });
        profile.hot_words.push(HotWord {
            text: "openclaw".to_string(),
            weight: 3,
            source: HotWordSource::User,
            use_count: 10,
            last_used: 0,
        });
        profile.correction_patterns.push(CorrectionPattern {
            original: "open cloud".to_string(),
            corrected: "openclaw".to_string(),
            count: 33,
            last_seen: 0,
            source: CorrectionSource::User,
            manual: false,
        });
    });

    for engine in crate::utils::paths::local_asr_engines() {
        let response: ServerResponse = serde_json::from_value(serde_json::json!({
            "success": true,
            "text": "请用 get hub 打开 open cloud。 Ask josé to use c++ and c#.",
            "engine": engine
        }))
        .unwrap();
        let result = server_response_to_transcription_result(
            &state,
            response,
            crate::services::funasr_service::TranscribeOptions { final_pass: true },
        );
        assert_eq!(
            result.text,
            "请用 github 打开 openclaw。 Ask José to use C++ and C#."
        );
    }
}

#[test]
fn stale_starting_guard_does_not_clear_a_new_start_owner() {
    let engine = crate::state::EngineState::default();
    assert!(engine.try_begin_funasr_start(1));
    let stale_guard = StartingFlagGuard {
        engine: &engine,
        owner: 1,
        child: None,
    };

    // set_engine/迁移会取消旧启动并释放标志，随后新启动获得所有权。
    engine.block_funasr_starting();
    engine.unblock_funasr_starting();
    assert!(engine.try_begin_funasr_start(2));

    drop(stale_guard);

    assert!(
        engine.is_funasr_starting(),
        "旧启动 guard 不得清除新启动持有的标志"
    );
}

#[test]
fn stale_engine_extraction_progress_is_suppressed_after_stop() {
    let generation = Arc::new(AtomicU64::new(4));
    let gate = EngineProgressGate {
        generation: generation.clone(),
        status_commit: Arc::new(parking_lot::Mutex::new(())),
        expected_generation: 4,
    };
    assert!(gate.is_current());

    generation.fetch_add(1, Ordering::SeqCst);

    assert!(
        !gate.is_current(),
        "old extraction progress must not emit after an engine switch changes generation"
    );
}

#[tokio::test]
async fn read_json_response_accepts_valid_json_line() {
    let response = read_response_from_chunks(&[br#"{"success":true,"message":"ok"}\n"#]).await;

    assert_eq!(response.success, Some(true));
    assert_eq!(response.message.as_deref(), Some("ok"));
}

#[tokio::test]
async fn read_json_response_recovers_from_non_utf8_prefix() {
    let response =
        read_response_from_chunks(&[b"\xff\xfe", br#"{"success":true,"message":"ok"}\n"#]).await;

    assert_eq!(response.success, Some(true));
    assert_eq!(response.message.as_deref(), Some("ok"));
}

#[tokio::test]
async fn read_json_response_recovers_json_from_mixed_line() {
    let response =
        read_response_from_chunks(&[br#"noise >>> {"success":true,"message":"ok"}\n"#]).await;

    assert_eq!(response.success, Some(true));
    assert_eq!(response.message.as_deref(), Some("ok"));
}

#[tokio::test]
async fn read_json_response_skips_python_dict_noise_and_reads_next_json() {
    let response = read_response_from_chunks(&[
        b"{'load_data': '0.001'}\n",
        br#"{"success":true,"message":"ok"}\n"#,
    ])
    .await;

    assert_eq!(response.success, Some(true));
    assert_eq!(response.message.as_deref(), Some("ok"));
}

#[tokio::test]
async fn read_json_response_reports_stdout_eof_without_a_response() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    writer.shutdown().await.unwrap();
    let mut reader = BufReader::new(reader);

    let error =
        read_json_response::<ServerResponse, _>(&mut reader, Duration::from_secs(1), "test")
            .await
            .expect_err("closed stdout must fail instead of waiting for a response");

    assert_eq!(error.to_string(), "ASR error: test failed: stdout closed");
}

#[tokio::test]
async fn read_json_response_times_out_when_stdout_stays_open_without_data() {
    let (_writer, reader) = tokio::io::duplex(1024);
    let mut reader = BufReader::new(reader);

    let error = tokio::time::timeout(
        Duration::from_secs(1),
        read_json_response::<ServerResponse, _>(&mut reader, Duration::from_millis(20), "test"),
    )
    .await
    .expect("response timeout path must not hang the test")
    .expect_err("silent stdout must respect the response timeout");

    assert_eq!(error.to_string(), "ASR error: test timed out");
}

#[tokio::test]
async fn read_json_response_ignores_stale_response_before_matching_request() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    writer
        .write_all(
            br#"{"request_id":1,"success":true,"message":"stale"}
{"request_id":2,"success":true,"message":"fresh"}
"#,
        )
        .await
        .unwrap();
    writer.shutdown().await.unwrap();
    let mut reader = BufReader::new(reader);
    let response = read_json_response_matching(
        &mut reader,
        Duration::from_secs(1),
        "test",
        |response: &ServerResponse| response.request_id == Some(2),
    )
    .await
    .unwrap();

    assert_eq!(
            response.message.as_deref(),
            Some("fresh"),
            "ASR IPC must correlate responses by request_id and skip stale responses left behind by a cancelled request"
        );
}

#[tokio::test]
async fn read_json_response_accepts_legacy_response_without_request_id() {
    let (mut writer, reader) = tokio::io::duplex(1024);
    writer
        .write_all(
            br#"{"success":true,"message":"legacy"}
{"request_id":7,"success":true,"message":"matched"}
"#,
        )
        .await
        .unwrap();
    writer.shutdown().await.unwrap();
    let mut reader = BufReader::new(reader);
    let response = read_json_response_matching(
        &mut reader,
        Duration::from_secs(1),
        "test",
        |response: &ServerResponse| match response.request_id {
            Some(id) => id == 7,
            None => true,
        },
    )
    .await
    .unwrap();

    assert_eq!(response.message.as_deref(), Some("legacy"));
}

#[test]
fn engine_extraction_preserves_existing_engine_until_archive_succeeds() {
    let root = std::env::temp_dir().join(format!(
        "speak_flow_engine_replace_test_{}_{}",
        std::process::id(),
        super::now_unix_ms()
    ));
    let engine_dir = root.join("engine");
    let missing_staging_dir = root.join("missing-staging");
    let backup_dir = root.join("engine.backup");
    std::fs::create_dir_all(&engine_dir).unwrap();
    std::fs::write(engine_dir.join("engine.exe"), b"old-engine").unwrap();

    let result = super::replace_engine_dir(&engine_dir, &missing_staging_dir, &backup_dir);

    assert!(result.is_err());
    assert_eq!(
        std::fs::read(engine_dir.join("engine.exe")).unwrap(),
        b"old-engine"
    );
    let _ = std::fs::remove_dir_all(root);
}
