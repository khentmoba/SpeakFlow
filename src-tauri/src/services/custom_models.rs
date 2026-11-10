//! Custom GGUF inspection and zero-copy HuggingFace cache discovery.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::utils::paths;

const CUSTOM_MODELS_FILE: &str = "custom_models.json";
const GGUF_MAGIC: &[u8; 4] = b"GGUF";
const MAX_GGUF_STR_LEN: u64 = 16 * 1024 * 1024;
const MAX_KV_SCAN: u64 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GgufInspection {
    pub valid: bool,
    pub version: u32,
    pub tensor_count: u64,
    pub metadata_kv_count: u64,
    pub architecture: Option<String>,
    pub model_name: Option<String>,
    pub file_type: Option<u32>,
    pub file_size: u64,
    pub supported_by_transcribe_cpp: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CustomModelEntry {
    pub id: String,
    pub engine_key: String,
    pub name: String,
    pub path: String,
    pub source: String, // "local_gguf" | "hf_cache"
    pub repo_id: Option<String>,
    pub architecture: Option<String>,
    pub file_size: u64,
    pub added_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredHfModel {
    pub repo_id: String,
    pub filename: String,
    pub path: String,
    pub file_size: u64,
    pub architecture: Option<String>,
    pub model_name: Option<String>,
    pub already_registered: bool,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn is_supported_asr_arch(arch: Option<&str>, filename_hint: &str) -> bool {
    let haystack = format!(
        "{} {}",
        arch.unwrap_or_default().to_ascii_lowercase(),
        filename_hint.to_ascii_lowercase()
    );
    [
        "whisper",
        "qwen",
        "parakeet",
        "nemo",
        "nemotron",
        "canary",
        "moonshine",
        "voxtral",
        "granite",
        "cohere",
        "gigaam",
        "funasr",
        "medasr",
        "moss",
        "asr",
        "r2t2",
    ]
    .iter()
    .any(|kw| haystack.contains(kw))
}

fn read_u32_le<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64_le<R: Read>(reader: &mut R) -> Result<u64, String> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(buf))
}

fn read_gguf_string<R: Read>(reader: &mut R) -> Result<String, String> {
    let len = read_u64_le(reader)?;
    if len > MAX_GGUF_STR_LEN {
        return Err(format!("GGUF string length {} exceeds safe limit", len));
    }
    let mut bytes = vec![0u8; len as usize];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn gguf_scalar_size(vtype: u32) -> Option<u64> {
    match vtype {
        0 | 1 | 7 => Some(1), // uint8, int8, bool
        2 | 3 => Some(2),     // uint16, int16
        4..=6 => Some(4),     // uint32, int32, float32
        10..=12 => Some(8),
        _ => None,
    }
}

fn skip_gguf_value<R: Read + Seek>(reader: &mut R, vtype: u32) -> Result<(), String> {
    if let Some(sz) = gguf_scalar_size(vtype) {
        reader
            .seek(SeekFrom::Current(sz as i64))
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    match vtype {
        8 => {
            let len = read_u64_le(reader)?;
            if len > MAX_GGUF_STR_LEN {
                return Err("GGUF string too large".to_string());
            }
            reader
                .seek(SeekFrom::Current(len as i64))
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        9 => {
            // Array: elem_type (u32), count (u64)
            let elem_type = read_u32_le(reader)?;
            let count = read_u64_le(reader)?;
            if let Some(sz) = gguf_scalar_size(elem_type) {
                let bytes = count.checked_mul(sz).ok_or("GGUF array overflow")?;
                reader
                    .seek(SeekFrom::Current(bytes as i64))
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            for _ in 0..count.min(4096) {
                skip_gguf_value(reader, elem_type)?;
            }
            Ok(())
        }
        other => Err(format!("Unsupported GGUF metadata type {}", other)),
    }
}

/// Inspects a GGUF file header directly in pure Rust without loading tensors into RAM/VRAM.
pub fn inspect_gguf_file(path: &Path) -> GgufInspection {
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => {
            return GgufInspection {
                valid: false,
                version: 0,
                tensor_count: 0,
                metadata_kv_count: 0,
                architecture: None,
                model_name: None,
                file_type: None,
                file_size: 0,
                supported_by_transcribe_cpp: false,
                error: Some("Path is not a file".to_string()),
            };
        }
        Err(e) => {
            return GgufInspection {
                valid: false,
                version: 0,
                tensor_count: 0,
                metadata_kv_count: 0,
                architecture: None,
                model_name: None,
                file_type: None,
                file_size: 0,
                supported_by_transcribe_cpp: false,
                error: Some(e.to_string()),
            };
        }
    };
    let file_size = meta.len();
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return GgufInspection {
                valid: false,
                version: 0,
                tensor_count: 0,
                metadata_kv_count: 0,
                architecture: None,
                model_name: None,
                file_type: None,
                file_size,
                supported_by_transcribe_cpp: false,
                error: Some(e.to_string()),
            };
        }
    };
    let mut reader = BufReader::new(file);
    let mut magic = [0u8; 4];
    if reader.read_exact(&mut magic).is_err() || &magic != GGUF_MAGIC {
        return GgufInspection {
            valid: false,
            version: 0,
            tensor_count: 0,
            metadata_kv_count: 0,
            architecture: None,
            model_name: None,
            file_type: None,
            file_size,
            supported_by_transcribe_cpp: false,
            error: Some("Not a valid GGUF file (missing GGUF magic header)".to_string()),
        };
    }

    let version = match read_u32_le(&mut reader) {
        Ok(v) if (2..=3).contains(&v) => v,
        Ok(v) => {
            return GgufInspection {
                valid: false,
                version: v,
                tensor_count: 0,
                metadata_kv_count: 0,
                architecture: None,
                model_name: None,
                file_type: None,
                file_size,
                supported_by_transcribe_cpp: false,
                error: Some(format!("Unsupported GGUF version {}", v)),
            };
        }
        Err(e) => {
            return GgufInspection {
                valid: false,
                version: 0,
                tensor_count: 0,
                metadata_kv_count: 0,
                architecture: None,
                model_name: None,
                file_type: None,
                file_size,
                supported_by_transcribe_cpp: false,
                error: Some(e),
            };
        }
    };

    let tensor_count = read_u64_le(&mut reader).unwrap_or(0);
    let kv_count = read_u64_le(&mut reader).unwrap_or(0);

    let mut architecture = None;
    let mut model_name = None;
    let mut file_type = None;

    for _ in 0..kv_count.min(MAX_KV_SCAN) {
        let Ok(key) = read_gguf_string(&mut reader) else {
            break;
        };
        let Ok(vtype) = read_u32_le(&mut reader) else {
            break;
        };
        match (key.as_str(), vtype) {
            ("general.architecture", 8) => {
                if let Ok(val) = read_gguf_string(&mut reader) {
                    architecture = Some(val);
                } else {
                    break;
                }
            }
            ("general.name", 8) => {
                if let Ok(val) = read_gguf_string(&mut reader) {
                    model_name = Some(val);
                } else {
                    break;
                }
            }
            ("general.file_type", 4) => {
                if let Ok(val) = read_u32_le(&mut reader) {
                    file_type = Some(val);
                } else {
                    break;
                }
            }
            _ => {
                if skip_gguf_value(&mut reader, vtype).is_err() {
                    break;
                }
            }
        }
    }

    let filename_hint = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let supported = is_supported_asr_arch(architecture.as_deref(), filename_hint);

    GgufInspection {
        valid: true,
        version,
        tensor_count,
        metadata_kv_count: kv_count,
        architecture,
        model_name,
        file_type,
        file_size,
        supported_by_transcribe_cpp: supported,
        error: None,
    }
}

fn custom_models_path() -> PathBuf {
    paths::get_data_dir().join(CUSTOM_MODELS_FILE)
}

pub fn load_custom_models() -> Vec<CustomModelEntry> {
    let path = custom_models_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<CustomModelEntry>>(&data).unwrap_or_default()
}

pub fn save_custom_models(entries: &[CustomModelEntry]) -> Result<(), String> {
    let path = custom_models_path();
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Failed to serialize custom models: {e}"))?;
    paths::atomic_write(&path, json.as_bytes())
        .map_err(|e| format!("Failed to write custom models: {e}"))
}

pub fn resolve_custom_model(engine_key: &str) -> Option<CustomModelEntry> {
    if !engine_key.starts_with("custom:") {
        return None;
    }
    load_custom_models()
        .into_iter()
        .find(|entry| entry.engine_key == engine_key)
}

pub fn is_registered_custom_engine(engine_key: &str) -> bool {
    resolve_custom_model(engine_key).is_some()
}

fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == '-' || ch == '_' || ch == ' ') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "model".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

/// Registers a GGUF model (either local file or zero-copy HF cache path) without copying.
pub fn register_custom_model(
    path: &Path,
    name_override: Option<&str>,
    source: &str,
    repo_id: Option<&str>,
) -> Result<CustomModelEntry, String> {
    let inspection = inspect_gguf_file(path);
    if !inspection.valid {
        return Err(inspection
            .error
            .unwrap_or_else(|| "Invalid GGUF model file".to_string()));
    }
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let path_str = paths::strip_win_prefix(&canonical);

    let mut existing = load_custom_models();
    if let Some(found) = existing
        .iter()
        .find(|e| e.path.eq_ignore_ascii_case(&path_str))
    {
        return Ok(found.clone());
    }

    let base_name = name_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or(inspection.model_name.clone())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Custom GGUF")
                .to_string()
        });
    let base_slug = slugify(&base_name);
    let mut id = base_slug.clone();
    let mut counter = 2usize;
    while existing.iter().any(|e| e.id == id) {
        id = format!("{}-{}", base_slug, counter);
        counter += 1;
    }

    let entry = CustomModelEntry {
        engine_key: format!("custom:{}", id),
        id,
        name: base_name,
        path: path_str,
        source: if source == "hf_cache" {
            "hf_cache".to_string()
        } else {
            "local_gguf".to_string()
        },
        repo_id: repo_id.map(str::to_string),
        architecture: inspection.architecture,
        file_size: inspection.file_size,
        added_at: now_secs(),
    };
    existing.push(entry.clone());
    save_custom_models(&existing)?;
    Ok(entry)
}

pub fn remove_custom_model(id: &str) -> Result<bool, String> {
    let mut existing = load_custom_models();
    let before = existing.len();
    existing.retain(|entry| entry.id != id && entry.engine_key != id);
    if existing.len() == before {
        return Ok(false);
    }
    save_custom_models(&existing)?;
    Ok(true)
}

fn collect_gguf_under(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_gguf_under(&path, depth + 1, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
        {
            out.push(path);
        }
    }
}

/// Zero-copy discovery across the HuggingFace cache and user model folders.
pub fn scan_hf_cache_models() -> Vec<DiscoveredHfModel> {
    let mut roots = vec![
        paths::get_effective_models_dir(),
        paths::get_default_models_dir(),
    ];
    for extra in paths::read_model_dirs() {
        roots.push(PathBuf::from(extra));
    }

    let registered: HashSet<String> = load_custom_models()
        .into_iter()
        .map(|m| m.path.to_ascii_lowercase())
        .collect();
    let mut seen_paths = HashSet::new();
    let mut out = Vec::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let repo_dir = entry.path();
            if !repo_dir.is_dir() {
                continue;
            }
            let dir_name = repo_dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let repo_id = if let Some(rest) = dir_name.strip_prefix("models--") {
                rest.replace("--", "/")
            } else {
                continue;
            };
            let snapshots = repo_dir.join("snapshots");
            if !snapshots.is_dir() {
                continue;
            }
            let mut ggufs = Vec::new();
            collect_gguf_under(&snapshots, 0, &mut ggufs);
            for gguf in ggufs {
                let canon = std::fs::canonicalize(&gguf).unwrap_or_else(|_| gguf.clone());
                let path_str = paths::strip_win_prefix(&canon);
                if !seen_paths.insert(path_str.to_ascii_lowercase()) {
                    continue;
                }
                let inspection = inspect_gguf_file(&gguf);
                if !inspection.valid || inspection.file_size < 1_000_000 {
                    continue;
                }
                let filename = gguf
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("model.gguf")
                    .to_string();
                let already_registered = registered.contains(&path_str.to_ascii_lowercase());
                out.push(DiscoveredHfModel {
                    repo_id: repo_id.clone(),
                    filename,
                    path: path_str,
                    file_size: inspection.file_size,
                    architecture: inspection.architecture,
                    model_name: inspection.model_name,
                    already_registered,
                });
            }
        }
    }

    out.sort_by(|a, b| a.repo_id.cmp(&b.repo_id).then(a.filename.cmp(&b.filename)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_gguf(path: &Path, arch: &str, name: &str) {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes()); // version 3
        buf.extend_from_slice(&4u64.to_le_bytes()); // tensor count
        buf.extend_from_slice(&2u64.to_le_bytes()); // kv count

        let write_kv_str = |buf: &mut Vec<u8>, key: &str, val: &str| {
            buf.extend_from_slice(&(key.len() as u64).to_le_bytes());
            buf.extend_from_slice(key.as_bytes());
            buf.extend_from_slice(&8u32.to_le_bytes()); // string type
            buf.extend_from_slice(&(val.len() as u64).to_le_bytes());
            buf.extend_from_slice(val.as_bytes());
        };
        write_kv_str(&mut buf, "general.architecture", arch);
        write_kv_str(&mut buf, "general.name", name);
        std::fs::write(path, &buf).unwrap();
    }

    #[test]
    fn inspects_valid_gguf_v3_header() {
        let dir = std::env::temp_dir().join(format!("sf-gguf-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("custom-parakeet.gguf");
        write_test_gguf(&file, "parakeet", "Parakeet Custom Q8");

        let info = inspect_gguf_file(&file);
        assert!(info.valid);
        assert_eq!(info.version, 3);
        assert_eq!(info.tensor_count, 4);
        assert_eq!(info.architecture.as_deref(), Some("parakeet"));
        assert_eq!(info.model_name.as_deref(), Some("Parakeet Custom Q8"));
        assert!(info.supported_by_transcribe_cpp);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_magic_header() {
        let dir = std::env::temp_dir().join(format!("sf-gguf-bad-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("fake.gguf");
        std::fs::write(&file, b"NOTGGUF12345").unwrap();

        let info = inspect_gguf_file(&file);
        assert!(!info.valid);
        assert!(info.error.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
