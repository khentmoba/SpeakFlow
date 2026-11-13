use super::*;

/// Get the HuggingFace cache root directory
///
/// Prefers the user's custom model directory, then follows the standard HuggingFace rules.
pub(super) fn get_hf_cache_root() -> PathBuf {
    paths::get_effective_models_dir()
}

/// Check whether a HuggingFace model is cached and contains actual model weight files
///
/// Checking only the directory structure is not enough: cancelling a download midway leaves an empty shell directory (refs/snapshots exist but no weight files),
/// which later hangs loading. This additionally verifies that snapshots contains a model weight file larger than 1MB (.pt/.bin/.safetensors/.onnx).
#[cfg(test)]
pub(super) fn is_hf_repo_ready(repo_id: &str, required_file: Option<&str>) -> bool {
    hf_repo_model_state(repo_id, required_file) == ModelState::Ready
}

pub(super) fn hf_repo_model_state(repo_id: &str, required_file: Option<&str>) -> ModelState {
    let cache_root = get_hf_cache_root();
    let dir_name = format!("models--{}", repo_id.replace('/', "--"));
    let repo_dir = cache_root.join(&dir_name);

    log::info!(
        "Model check: repo={}, cache_root={}, repo_dir={}, exists={}",
        repo_id,
        cache_root.display(),
        repo_dir.display(),
        repo_dir.is_dir()
    );

    match std::fs::metadata(&repo_dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ModelState::Missing,
        _ => return ModelState::Corrupt,
    }

    let snapshots_dir = repo_dir.join("snapshots");
    let entries = match std::fs::read_dir(&snapshots_dir) {
        Ok(e) => e,
        Err(err) => {
            log::warn!(
                "Cannot read the snapshots directory: {} — {}",
                snapshots_dir.display(),
                err
            );
            return if snapshots_dir.exists() || err.kind() != std::io::ErrorKind::NotFound {
                ModelState::Corrupt
            } else {
                ModelState::Missing
            };
        }
    };

    let mut snapshots = match entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(snapshots) => snapshots,
        Err(_) => return ModelState::Corrupt,
    };
    snapshots.sort();
    if let Ok(revision) = std::fs::read_to_string(repo_dir.join("refs/main")) {
        if let Some(index) = snapshots.iter().position(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some(revision.trim())
        }) {
            let selected = snapshots.remove(index);
            snapshots.insert(0, selected);
        }
    }
    let mut model_state = ModelState::Missing;
    for snapshot_path in snapshots {
        model_state = ModelState::Corrupt;
        if !snapshot_path.is_dir() {
            continue;
        }
        if required_file.is_some_and(|filename| {
            !std::fs::metadata(snapshot_path.join(filename))
                .is_ok_and(|meta| meta.is_file() && meta.len() >= 1_000_000)
        }) {
            continue;
        }
        log::info!("Checking snapshot: {}", snapshot_path.display());
        let manifest_present = !matches!(
            std::fs::symlink_metadata(snapshot_path.join(HF_COMPLETE_MANIFEST_NAME)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        );
        let ready = if manifest_present {
            snapshot_matches_completion_manifest_for_file(&snapshot_path, required_file)
        } else {
            snapshot_matches_legacy_weight_check(&snapshot_path)
        };
        if ready {
            log::info!("Model ready: {} (in {})", repo_id, snapshot_path.display());
            return ModelState::Ready;
        }
    }

    log::warn!(
        "Model not ready: {} — no complete download manifest found or file verification failed",
        repo_id
    );
    model_state
}

#[cfg(test)]
pub(super) fn snapshot_matches_completion_manifest(snapshot_path: &std::path::Path) -> bool {
    snapshot_matches_completion_manifest_for_file(snapshot_path, None)
}

pub(super) fn snapshot_matches_completion_manifest_for_file(
    snapshot_path: &std::path::Path,
    required_file: Option<&str>,
) -> bool {
    const MIN_SIZE: u64 = 1_000_000;
    let manifest_path = snapshot_path.join(HF_COMPLETE_MANIFEST_NAME);
    let data = match std::fs::read_to_string(&manifest_path) {
        Ok(data) => data,
        Err(err) => {
            log::warn!(
                "Missing model integrity manifest {}: {}",
                manifest_path.display(),
                err
            );
            return false;
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&data) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "Failed to parse model integrity manifest {}: {}",
                manifest_path.display(),
                err
            );
            return false;
        }
    };
    let Some(files) = manifest.get("files").and_then(|value| value.as_array()) else {
        return false;
    };
    let mut has_weight = false;
    let mut has_required_file = required_file.is_none();
    for item in files {
        let Some(rel_path) = item.get("path").and_then(|value| value.as_str()) else {
            return false;
        };
        let Some(expected_size) = item.get("size").and_then(|value| value.as_u64()) else {
            return false;
        };
        let normalized = rel_path.replace('\\', "/");
        if normalized.is_empty()
            || !std::path::Path::new(&normalized)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return false;
        }
        let path = snapshot_path.join(normalized);
        let actual_size = match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => meta.len(),
            _ => return false,
        };
        if actual_size != expected_size {
            return false;
        }
        match item.get("sha256") {
            None | Some(serde_json::Value::Null) => {}
            Some(serde_json::Value::String(expected_hash)) => {
                if expected_hash.len() != 64
                    || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return false;
                }
                let Ok(mut file) = std::fs::File::open(&path) else {
                    return false;
                };
                let mut hash = Sha256::new();
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => hash.update(&buffer[..count]),
                        Err(_) => return false,
                    }
                }
                if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected_hash) {
                    return false;
                }
            }
            _ => return false,
        }
        if required_file == Some(rel_path) {
            has_required_file = true;
        }
        if [".pt", ".bin", ".safetensors", ".onnx", ".gguf"]
            .iter()
            .any(|ext| rel_path.ends_with(ext))
            && actual_size >= MIN_SIZE
        {
            has_weight = true;
        }
    }
    has_weight && has_required_file
}

pub(super) fn snapshot_matches_legacy_weight_check(snapshot_path: &std::path::Path) -> bool {
    const MIN_SIZE: u64 = 1_000_000;
    const WEIGHT_EXTS: &[&str] = &[".pt", ".bin", ".safetensors", ".onnx", ".gguf"];

    fn visit(dir: &std::path::Path, exts: &[&str], min_size: u64) -> Option<bool> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut has_weight = false;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".incomplete") {
                return Some(false);
            }
            if path.is_dir() {
                match visit(&path, exts, min_size) {
                    Some(true) => has_weight = true,
                    Some(false) | None => return Some(false),
                }
            } else if exts.iter().any(|ext| name.ends_with(ext)) {
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.len() >= min_size => has_weight = true,
                    Ok(_) => {}
                    Err(_) => return Some(false),
                }
            }
        }
        Some(has_weight)
    }

    let ready = visit(snapshot_path, WEIGHT_EXTS, MIN_SIZE).unwrap_or(false);
    if ready {
        log::warn!(
            "Model snapshot has no integrity manifest; allowing it through via the legacy weight-file check for compatibility: {}",
            snapshot_path.display()
        );
    }
    ready
}

/// A catalog model file found in one of the user's read-only model folders.
///
/// The file must sit directly in the folder and match the catalog size exactly,
/// the same rule the cache uses, so a half-copied or different quantization is
/// not mistaken for the pinned model.
pub(super) fn find_shared_model_file(filename: &str, size: u64) -> Option<PathBuf> {
    paths::read_model_dirs().into_iter().find_map(|dir| {
        let candidate = PathBuf::from(dir).join(filename);
        std::fs::metadata(&candidate)
            .ok()
            .filter(|meta| meta.is_file() && meta.len() == size)
            .map(|_| candidate)
    })
}

/// True when any snapshot of `repo_dir` holds `filename` at exactly `size` bytes.
///
/// A size match is enough for the model browser's "Installed" label. It does not
/// hash the file; the selected engine still gets the full readiness check.
pub(super) fn repo_dir_has_file(repo_dir: &std::path::Path, filename: &str, size: u64) -> bool {
    let Ok(snapshots) = std::fs::read_dir(repo_dir.join("snapshots")) else {
        return false;
    };
    snapshots.filter_map(Result::ok).any(|snapshot| {
        std::fs::metadata(snapshot.path().join(filename))
            .is_ok_and(|meta| meta.is_file() && meta.len() == size)
    })
}

/// Catalog engines whose model file is present in the models directory or in
/// one of the user's read-only model folders.
pub fn installed_catalog_engines() -> Vec<String> {
    let cache_root = get_hf_cache_root();
    crate::utils::asr_catalog::models()
        .iter()
        .filter(|model| {
            let repo_dir = cache_root.join(format!("models--{}", model.repo_id.replace('/', "--")));
            repo_dir_has_file(&repo_dir, &model.filename, model.size)
                || find_shared_model_file(&model.filename, model.size).is_some()
        })
        .map(|model| model.key.clone())
        .collect()
}

/// Check whether the model files for the current ASR engine have been downloaded.
pub(crate) fn inspect_model_files_for_engine(engine: &str) -> ModelCheckResult {
    if paths::is_online_engine(engine) {
        return ModelCheckResult {
            all_present: true,
            asr_model: true,
            vad_model: true,
            punc_model: true,
            engine: engine.to_string(),
            cache_path: String::new(),
            missing_models: Vec::new(),
            model_state: ModelState::Ready,
            corrupt_models: Vec::new(),
        };
    }

    if engine.starts_with("custom:") {
        return match crate::services::custom_models::resolve_custom_model(engine) {
            Some(entry) if std::path::Path::new(&entry.path).is_file() => ModelCheckResult {
                all_present: true,
                asr_model: true,
                vad_model: true,
                punc_model: true,
                engine: engine.to_string(),
                cache_path: entry.path,
                missing_models: Vec::new(),
                model_state: ModelState::Ready,
                corrupt_models: Vec::new(),
            },
            Some(entry) => ModelCheckResult {
                all_present: false,
                asr_model: false,
                vad_model: true,
                punc_model: true,
                engine: engine.to_string(),
                cache_path: entry.path.clone(),
                missing_models: vec![format!("{} ({})", entry.name, entry.path)],
                model_state: ModelState::Missing,
                corrupt_models: Vec::new(),
            },
            None => ModelCheckResult {
                all_present: false,
                asr_model: false,
                vad_model: true,
                punc_model: true,
                engine: engine.to_string(),
                cache_path: String::new(),
                missing_models: vec![format!("Custom model {}", engine)],
                model_state: ModelState::Missing,
                corrupt_models: Vec::new(),
            },
        };
    }

    let cache_root = get_hf_cache_root();
    let cache_path = cache_root.to_string_lossy().to_string();

    // Legacy or corrupted local engine values are uniformly migrated to the default Qwen3-ASR 0.6B.
    let model = local_model_file(engine);
    let normalized_engine = model.engine;
    let (repo_id, filename, description) =
        (model.repo_id, model.filename, model.description.as_str());
    let mut missing_models = Vec::new();
    let mut corrupt_models = Vec::new();
    let mut model_state = report_model_repo_file_state(
        repo_id,
        Some(filename),
        description,
        &mut missing_models,
        &mut corrupt_models,
    );
    // R2T2 resolves its own pinned snapshot and never reads the model folders,
    // so a shared copy must not make it look ready.
    if model_state != ModelState::Ready && normalized_engine != paths::R2T2_ENGINE {
        if let Some(shared) = find_shared_model_file(filename, model.size) {
            log::info!("Model comes from the shared folder: {}", shared.display());
            model_state = ModelState::Ready;
            missing_models.clear();
            corrupt_models.clear();
        }
    }
    let asr_present = model_state == ModelState::Ready;

    ModelCheckResult {
        all_present: asr_present,
        asr_model: asr_present,
        vad_model: true,
        punc_model: true,
        engine: normalized_engine.to_string(),
        cache_path,
        missing_models,
        model_state,
        corrupt_models,
    }
}

pub async fn check_model_files() -> Result<ModelCheckResult, AppError> {
    Ok(inspect_model_files_for_engine(&paths::read_engine_config()))
}
