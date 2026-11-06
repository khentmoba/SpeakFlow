//! Whisper is offered again, but only as the labelled long-tail family.
//!
//! It is the only source of the 61 languages the rest of the catalog does not
//! cover, and its own benchmark grades most of those languages D or F. Keep it
//! out of the default picks and keep the retired SenseVoice runtime out.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri should have a repository parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

#[test]
fn whisper_is_offered_only_as_the_long_tail_family() {
    let catalog = read("src-tauri/resources/asr_catalog.json");
    assert_eq!(
        catalog.match_indices("\"long_tail\":true").count(),
        8,
        "only the multilingual Whisper variants are long-tail"
    );
    assert_eq!(catalog.match_indices("\"key\":\"whisper").count(), 12);
    for line in catalog
        .lines()
        .filter(|line| line.contains("\"key\":\"whisper"))
    {
        assert!(line.contains("\"recommended\":false"), "{line}");
        if line.contains("\"languages\":[\"en\"]") {
            assert!(line.contains("\"long_tail\":false"), "{line}");
        } else {
            assert!(line.contains("\"long_tail\":true"), "{line}");
        }
    }
    // SenseVoice stays retired; its own contract covers the runtime.
    assert!(!catalog.to_ascii_lowercase().contains("sensevoice"));
}

#[test]
fn bundled_qwen_runtime_uses_firered_vad_without_whisper_dependencies() {
    let project = read("pyproject.toml");
    let build = read("scripts/build_engine.py");
    let qwen_server = read("src-tauri/resources/qwen3_asr_server.py");

    assert!(project.contains("kaldi-native-fbank"));
    assert!(project.contains("onnxruntime"));
    assert!(!project.contains("faster-whisper"));
    assert!(!project.contains("librosa"));
    assert!(build.contains("fireredvad_vad.onnx"));
    assert!(!build.contains("whisper_server.py"));
    assert!(qwen_server.contains("FireRedVad"));
    assert!(!qwen_server.contains("faster_whisper"));
}

#[test]
fn whisper_uses_the_shared_transcribe_cpp_server() {
    let engine = read("src-tauri/resources/engine.py");
    let server = read("src-tauri/resources/qwen3_asr_server.py");
    let engine_options = read("src/lib/asrEngineOptions.ts");
    let tauri_config = read("src-tauri/tauri.conf.json");

    // Catalog models are dispatched by the shared server; no dedicated runtime.
    assert!(engine.contains("TRANSCRIBE_CPP_ASR_MODELS"));
    assert!(server.contains("TRANSCRIBE_CPP_ASR_MODELS"));
    assert!(!engine_options.contains("key: \"whisper\""));
    assert!(!tauri_config.contains("resources/whisper_server.py"));
    assert!(!repo_root()
        .join("src-tauri/resources/whisper_server.py")
        .exists());
}
