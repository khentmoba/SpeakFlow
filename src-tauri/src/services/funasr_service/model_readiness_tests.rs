use sha2::{Digest, Sha256};

fn run_in_isolated_cache(test_name: &str) -> bool {
    const CHILD_ENV: &str = "LIGHT_WHISPER_MODEL_READINESS_TEST_CHILD";
    if std::env::var(CHILD_ENV).as_deref() == Ok(test_name) {
        let cache = crate::utils::paths::get_data_dir().join("models");
        crate::utils::paths::write_models_dir(Some(cache.to_str().unwrap())).unwrap();
        return false;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "light-whisper-model-readiness-{}-{nonce}",
        std::process::id()
    ));
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env("SPEAK_FLOW_DATA_DIR", &directory)
        .env(CHILD_ENV, test_name)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&directory);
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    true
}

fn snapshot(repo_id: &str) -> std::path::PathBuf {
    let path = super::get_hf_cache_root()
        .join(format!("models--{}", repo_id.replace('/', "--")))
        .join("snapshots/test-revision");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("model.gguf"), vec![0_u8; 1_000_001]).unwrap();
    path
}

#[test]
fn installed_model_listing_needs_the_exact_file_name_and_size() {
    let repo_dir = std::env::temp_dir().join(format!(
        "speak-flow-installed-listing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let snapshot = repo_dir.join("snapshots/revision");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join("model-Q8_0.gguf"), vec![0_u8; 2_000]).unwrap();

    assert!(super::repo_dir_has_file(
        &repo_dir,
        "model-Q8_0.gguf",
        2_000
    ));
    assert!(!super::repo_dir_has_file(
        &repo_dir,
        "model-Q8_0.gguf",
        2_001
    ));
    assert!(!super::repo_dir_has_file(
        &repo_dir,
        "model-Q4_0.gguf",
        2_000
    ));
    assert!(!super::repo_dir_has_file(
        &repo_dir.join("missing"),
        "model-Q8_0.gguf",
        2_000
    ));
    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn installed_listing_reports_only_models_present_at_the_catalog_size() {
    if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::installed_listing_reports_only_models_present_at_the_catalog_size",
        ) {
            return;
        }
    let model = super::local_model_file("whisper-tiny");
    let snapshot = super::get_hf_cache_root()
        .join(format!("models--{}", model.repo_id.replace('/', "--")))
        .join("snapshots/test-revision");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join(model.filename), vec![0_u8; 2_000]).unwrap();

    // The catalog size is the real one, so a 2 KB stand-in is not "installed".
    assert!(!super::installed_catalog_engines().contains(&model.engine.to_string()));

    let expected = crate::utils::asr_catalog::find("whisper-tiny")
        .unwrap()
        .size;
    std::fs::write(snapshot.join(model.filename), vec![0_u8; expected as usize]).unwrap();
    let installed = super::installed_catalog_engines();
    assert!(
        installed.contains(&"whisper-tiny".to_string()),
        "{installed:?}"
    );
    assert!(!installed.contains(&"whisper-small".to_string()));
}

#[test]
fn model_repo_invalid_manifest_cannot_fall_back_to_large_legacy_weight() {
    if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::model_repo_invalid_manifest_cannot_fall_back_to_large_legacy_weight",
        ) {
            return;
        }
    let digest = format!("{:x}", Sha256::digest(vec![0_u8; 1_000_001]));
    let valid_weight = serde_json::json!({
        "path": "model.gguf", "size": 1_000_001, "sha256": digest,
    });
    let cases = [
            ("malformed", "{".to_string(), false),
            (
                "size-mismatch",
                serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_002}]}).to_string(),
                false,
            ),
            (
                "hash-mismatch",
                serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_001, "sha256": "0".repeat(64)}]}).to_string(),
                false,
            ),
            (
                "missing-listed-file",
                serde_json::json!({"files": [valid_weight, {"path": "config.json", "size": 2}]}).to_string(),
                false,
            ),
            (
                "unlisted-required-file",
                serde_json::json!({"files": [{"path": "other.gguf", "size": 1_000_001, "sha256": digest}]}).to_string(),
                true,
            ),
        ];
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for (case, manifest, manifest_valid) in cases {
        let repo_id = format!("test/{case}");
        let path = snapshot(&repo_id);
        if case == "unlisted-required-file" {
            std::fs::copy(path.join("model.gguf"), path.join("other.gguf")).unwrap();
        }
        std::fs::write(path.join(super::HF_COMPLETE_MANIFEST_NAME), manifest).unwrap();
        actual.push((
            case,
            super::snapshot_matches_completion_manifest(&path),
            super::is_hf_repo_ready(&repo_id, Some("model.gguf")),
        ));
        expected.push((case, manifest_valid, false));
    }
    assert_eq!(
        actual, expected,
        "a present manifest is authoritative, including SHA256 and the selected file"
    );
}

#[test]
fn model_repo_preserves_valid_manifest_and_no_manifest_legacy_cache() {
    if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::model_repo_preserves_valid_manifest_and_no_manifest_legacy_cache",
        ) {
            return;
        }
    let legacy = snapshot("test/legacy");
    assert!(!super::snapshot_matches_completion_manifest(&legacy));
    assert!(super::is_hf_repo_ready("test/legacy", Some("model.gguf")));

    let valid = snapshot("test/valid");
    let digest = format!(
        "{:x}",
        Sha256::digest(std::fs::read(valid.join("model.gguf")).unwrap())
    );
    std::fs::write(
        valid.join(super::HF_COMPLETE_MANIFEST_NAME),
        serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_001, "sha256": digest}]})
            .to_string(),
    )
    .unwrap();
    assert!(super::snapshot_matches_completion_manifest(&valid));
    assert!(super::is_hf_repo_ready("test/valid", Some("model.gguf")));
}

#[test]
fn selected_model_status_distinguishes_missing_ready_and_corrupt() {
    if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::selected_model_status_distinguishes_missing_ready_and_corrupt",
        ) {
            return;
        }
    let inspect =
        || serde_json::to_value(super::inspect_model_files_for_engine("qwen3-asr-0.6b")).unwrap();
    let missing = inspect();
    let model = super::local_model_file("qwen3-asr-0.6b");
    let selected = snapshot(model.repo_id);
    std::fs::rename(selected.join("model.gguf"), selected.join(model.filename)).unwrap();
    let ready = inspect();
    std::fs::write(selected.join(super::HF_COMPLETE_MANIFEST_NAME), "{").unwrap();
    let corrupt = inspect();

    assert_eq!(
        [
            missing["model_state"].as_str(),
            ready["model_state"].as_str(),
            corrupt["model_state"].as_str(),
        ],
        [Some("missing"), Some("ready"), Some("corrupt")],
        "serialized model state must preserve the reason recognition cannot start"
    );
    assert_eq!(missing["missing_models"].as_array().unwrap().len(), 1);
    assert!(missing["corrupt_models"].as_array().unwrap().is_empty());
    assert_eq!(ready["all_present"], true);
    assert!(ready["missing_models"].as_array().unwrap().is_empty());
    assert!(ready["corrupt_models"].as_array().unwrap().is_empty());
    assert_eq!(corrupt["all_present"], false);
    assert!(corrupt["missing_models"].as_array().unwrap().is_empty());
    let corrupt_models = corrupt["corrupt_models"].as_array().unwrap();
    assert_eq!(corrupt_models.len(), 1);
    assert!(corrupt_models[0]
        .as_str()
        .is_some_and(|name| !name.is_empty()));
}
