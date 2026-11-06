use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must live below the repository root")
        .to_path_buf()
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).expect("contract source must be readable")
}

#[test]
fn sensevoice_is_not_a_selectable_or_runnable_engine() {
    let root = repo_root();
    let settings = read(root.join("src/pages/SettingsPage.tsx"));
    let engine_options = read(root.join("src/lib/asrEngineOptions.ts"));
    let engine_entry = read(root.join("src-tauri/resources/engine.py"));
    let rust_paths = read(root.join("src-tauri/src/utils/paths.rs"));
    let catalog = read(root.join("src-tauri/resources/asr_catalog.json"));

    assert!(!settings.contains("key: \"sensevoice\""));
    assert!(!engine_options.contains("key: \"sensevoice\""));
    assert!(!engine_entry.contains("\"sensevoice\""));
    assert!(!rust_paths.contains("\"sensevoice\""));
    // The retired FunASR runtime stays out. The catalog offers a SenseVoice
    // Small GGUF through transcribe.cpp, which is a different engine; it must
    // not resurrect the old "sensevoice" key.
    assert!(!catalog.contains("\"key\":\"sensevoice\""));
    // Engine lists are generated, so the UI can only offer catalog models.
    // Default selection is exercised by the paths module's configuration tests.
    assert!(settings.contains("from \"@/lib/asrEngineOptions\""));
    assert!(engine_options.contains("CATALOG_MODELS.map((model)"));
    assert!(engine_entry.contains("engine_choices = sorted(LOCAL_ASR_MODELS)"));
    assert!(catalog.contains("\"key\":\"qwen3-asr-0.6b\""));
}

#[test]
fn sensevoice_runtime_is_not_packaged() {
    let root = repo_root();
    let build_script = read(root.join("scripts/build_engine.py"));
    let tauri_config = read(root.join("src-tauri/tauri.conf.json"));

    assert!(!root.join("src-tauri/resources/funasr_server.py").exists());
    assert!(!build_script.to_ascii_lowercase().contains("funasr"));
    assert!(!tauri_config.contains("funasr_server.py"));
}
