use super::*;

/// 获取 HuggingFace 缓存根目录
///
/// 优先使用用户自定义模型目录，其次按 HuggingFace 标准规则。
pub(super) fn get_hf_cache_root() -> PathBuf {
    paths::get_effective_models_dir()
}

/// 检查 HuggingFace 模型是否已缓存且包含实际模型权重文件
///
/// 仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshots 存在但无权重文件），
/// 导致后续加载卡死。这里额外验证 snapshots 中存在 >1MB 的模型权重文件（.pt/.bin/.safetensors/.onnx）。
#[cfg(test)]
pub(super) fn is_hf_repo_ready(repo_id: &str, required_file: Option<&str>) -> bool {
    hf_repo_model_state(repo_id, required_file) == ModelState::Ready
}

pub(super) fn hf_repo_model_state(repo_id: &str, required_file: Option<&str>) -> ModelState {
    let cache_root = get_hf_cache_root();
    let dir_name = format!("models--{}", repo_id.replace('/', "--"));
    let repo_dir = cache_root.join(&dir_name);

    log::info!(
        "模型检查: repo={}, cache_root={}, repo_dir={}, exists={}",
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
                "无法读取 snapshots 目录: {} — {}",
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
        log::info!("检查 snapshot: {}", snapshot_path.display());
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
            log::info!("模型就绪: {} (在 {})", repo_id, snapshot_path.display());
            return ModelState::Ready;
        }
    }

    log::warn!("模型未就绪: {} — 未找到完整下载清单或文件校验失败", repo_id);
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
            log::warn!("缺少模型完整性清单 {}: {}", manifest_path.display(), err);
            return false;
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&data) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "模型完整性清单解析失败 {}: {}",
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
            "模型 snapshot 缺少完整性清单，按旧版权重文件检查兼容放行: {}",
            snapshot_path.display()
        );
    }
    ready
}

/// 检查当前 ASR 引擎的模型文件是否已下载。
pub(super) fn inspect_model_files_for_engine(engine: &str) -> ModelCheckResult {
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

    let cache_root = get_hf_cache_root();
    let cache_path = cache_root.to_string_lossy().to_string();

    // 旧版或损坏的本地引擎值统一迁移到默认的 Qwen3-ASR 0.6B。
    let model = local_model_file(engine);
    let normalized_engine = model.engine;
    let (repo_id, filename, description) = (model.repo_id, model.filename, model.description);
    let mut missing_models = Vec::new();
    let mut corrupt_models = Vec::new();
    let model_state = report_model_repo_file_state(
        repo_id,
        Some(filename),
        description,
        &mut missing_models,
        &mut corrupt_models,
    );
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
