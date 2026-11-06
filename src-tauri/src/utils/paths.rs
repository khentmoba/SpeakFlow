use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};

use tauri::Manager;

const APP_IDENTIFIER: &str = "com.speak-flow.app";

pub fn get_data_dir() -> &'static PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let app_dir = std::env::var_os("SPEAK_FLOW_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".speak-flow"));
                base.join(APP_IDENTIFIER)
            });
        if let Err(err) = std::fs::create_dir_all(&app_dir) {
            log::warn!("创建应用数据目录失败 {}: {}", app_dir.display(), err);
        }
        app_dir
    })
}

fn get_resource_script_path(app: &tauri::AppHandle, filename: &str) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let script_path = resource_dir.join("resources").join(filename);
        if script_path.exists() {
            return script_path;
        }
    }

    PathBuf::from("resources").join(filename)
}

pub fn get_qwen3_asr_server_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "qwen3_asr_server.py")
}

pub fn get_r2t2_asr_server_path(app: &tauri::AppHandle) -> PathBuf {
    // Development DLLs are built beside this source script. Do not depend on
    // the launch working directory or a copied script without its DLL folder.
    #[cfg(debug_assertions)]
    {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/r2t2_asr_server.py");
        if source.is_file() {
            return source;
        }
    }
    get_resource_script_path(app, "r2t2_asr_server.py")
}

pub fn get_download_script_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "download_models.py")
}

pub fn strip_win_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

pub fn get_engine_config_path() -> PathBuf {
    get_data_dir().join("engine.json")
}

/// Engine used when engine.json is missing or names an unknown engine.
pub const DEFAULT_ENGINE: &str = "qwen3-asr-1.7b";

/// The one local engine that is not a catalog model.
pub const R2T2_ENGINE: &str = "confucius4-r2t2";

/// Every local engine: the catalog models plus R2T2. The catalog is
/// `src-tauri/resources/asr_catalog.json`, shared with Python and TypeScript.
pub fn local_asr_engines() -> impl Iterator<Item = &'static str> {
    super::asr_catalog::catalog_engines().chain(std::iter::once(R2T2_ENGINE))
}

pub fn is_local_engine(engine: &str) -> bool {
    engine == R2T2_ENGINE || super::asr_catalog::is_catalog_engine(engine)
}

pub const ONLINE_ASR_ENGINES: &[&str] = &["glm-asr", "alibaba-asr"];

pub fn read_engine_config() -> String {
    configured_engine(read_engine_json().get("engine").and_then(|v| v.as_str())).to_string()
}

fn configured_engine(engine: Option<&str>) -> &str {
    match engine {
        Some(value) if is_supported_engine(value) => value,
        _ => DEFAULT_ENGINE,
    }
}

pub fn is_supported_engine(engine: &str) -> bool {
    is_local_engine(engine) || ONLINE_ASR_ENGINES.contains(&engine)
}

/// Local engines served by `qwen3_asr_server.py` (transcribe.cpp GGUF models).
pub fn is_transcribe_cpp_engine(engine: &str) -> bool {
    super::asr_catalog::is_catalog_engine(engine)
}

pub fn is_online_engine(engine: &str) -> bool {
    ONLINE_ASR_ENGINES.contains(&engine)
}

/// Engines that condition recognition on Smart Vocabulary. The others only get
/// local spelling correction after recognition. Catalog models carry this in
/// `asr_catalog.json`; keep the rest in sync with `vocabulary` in
/// `src/lib/asrEngineCapabilities.ts`.
pub fn engine_receives_vocabulary(engine: &str) -> bool {
    super::asr_catalog::receives_vocabulary(engine)
        || matches!(engine, "confucius4-r2t2" | "glm-asr" | "alibaba-asr")
}

const GLM_ENDPOINT_INTERNATIONAL: &str = "https://api.z.ai";
const GLM_ENDPOINT_DOMESTIC: &str = "https://open.bigmodel.cn";

const ALIBABA_ENDPOINT_INTERNATIONAL: &str = "https://dashscope-intl.aliyuncs.com";
const ALIBABA_ENDPOINT_DOMESTIC: &str = "https://dashscope.aliyuncs.com";

pub const ALIBABA_DEFAULT_MODEL: &str = "qwen-audio-3.1-asr-flash";

/// 运行时抓取失败时使用的 ASR / Omni 兜底列表，更新于 2026-10-18。
/// 抓取成功后以账号实际可见模型为准，不受此列表限制。
///
/// 走哪条 HTTP 路径由 `alibaba_model_uses_omni_chat` 运行时决定：
/// - Qwen Audio / Qwen3 ASR → `/api/v1/services/aigc/multimodal-generation/generation`
/// - `*omni*` → `/compatible-mode/v1/chat/completions`
pub const ALIBABA_FALLBACK_MODEL_IDS: &[&str] = &[
    "qwen-audio-3.1-asr-flash",
    "qwen3-asr-flash",
    "qwen3-omni-flash",
    "qwen3-omni-plus",
    "qwen3.5-omni-flash",
    "qwen3.5-omni-plus",
    "qwen-omni-turbo",
];

/// 模型 ID 是否可能胜任 ASR（作为语音转文字使用）。
///
/// 用于过滤 DashScope `/v1/models` 返回的完整模型清单：包含 asr/omni/audio 关键词
/// 的模型入围，同时排除明确不做转写的 realtime / tts / vl / coder 等家族。
pub fn is_asr_capable_model_id(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let looks_asr =
        id.contains("asr") || id.contains("omni") || id.contains("audio") || id.contains("stt");
    if !looks_asr {
        return false;
    }
    // 精确匹配：`-vl-` / `-vl` 尾部（避免误伤 "novel"/"evaluation" 等普通词）
    const BLOCK_SUBSTR: &[&str] = &[
        "realtime",
        "tts",
        "embedding",
        "embed",
        "rerank",
        "caption",
        "coder",
        "math",
        "thinking",
        "image",
        "video-gen",
    ];
    if BLOCK_SUBSTR.iter().any(|b| id.contains(b)) {
        return false;
    }
    // vision-language 模型（qwen2.5-vl-*, qwen3-vl-*）走段边界匹配
    if id.contains("-vl-") || id.ends_with("-vl") {
        return false;
    }
    true
}

/// Omni 家族模型走 OpenAI-compat chat.completions 路径；
/// 其它走 ASR 专用的 multimodal-generation 路径。
pub fn alibaba_model_uses_omni_chat(model: &str) -> bool {
    model.contains("omni")
}

fn engine_json_object_or_empty(value: serde_json::Value) -> serde_json::Value {
    if value.is_object() {
        value
    } else {
        serde_json::json!({})
    }
}

fn read_engine_json() -> serde_json::Value {
    let path = get_engine_config_path();
    match std::fs::read_to_string(path) {
        Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
            Ok(value) => {
                if !value.is_object() {
                    log::warn!("engine.json 是有效 JSON 但不是对象，已按空配置处理");
                }
                engine_json_object_or_empty(value)
            }
            Err(err) => {
                log::warn!("engine.json 解析失败，已按空配置处理: {}", err);
                serde_json::json!({})
            }
        },
        Err(_) => serde_json::json!({}),
    }
}

#[cfg(target_os = "windows")]
fn replace_file_atomic(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let mut from_wide: Vec<u16> = from.as_os_str().encode_wide().collect();
    let mut to_wide: Vec<u16> = to.as_os_str().encode_wide().collect();
    from_wide.push(0);
    to_wide.push(0);
    // SAFETY: Both paths are NUL-terminated UTF-16 buffers that outlive the call, and the flags are plain integers.
    let ok = unsafe {
        MoveFileExW(
            from_wide.as_ptr(),
            to_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomic(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(from, to)
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), std::io::Error> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("atomic");
    let tmp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let write_result = (|| -> Result<(), std::io::Error> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(data)?;
        file.sync_all()?;
        replace_file_atomic(&tmp_path, path)
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_result
}

fn write_engine_json(obj: &serde_json::Value) -> Result<(), std::io::Error> {
    let config_path = get_engine_config_path();
    let serialized = serde_json::to_string_pretty(obj).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to serialize configuration: {}", e),
        )
    })?;
    atomic_write(&config_path, serialized.as_bytes())
}

fn read_region_field(field: &str) -> String {
    match read_engine_json().get(field).and_then(|v| v.as_str()) {
        Some("domestic") => "domestic".to_string(),
        _ => "international".to_string(),
    }
}

/// 返回 GLM-ASR 区域标识：`"international"` 或 `"domestic"`
pub fn read_glm_region() -> String {
    read_region_field("glm_endpoint")
}

/// 返回 Alibaba ASR 区域标识：`"international"` 或 `"domestic"`
pub fn read_alibaba_region() -> String {
    read_region_field("alibaba_region")
}

/// 返回 Alibaba ASR 当前选择的模型 ID。
///
/// 这里不再对值做白名单校验——DashScope 上架速度快于硬编码列表的更新频率，
/// 运行时抓取回来的新模型应该能直接用。非法字符由 write_alibaba_model 入口过滤。
pub fn read_alibaba_model() -> String {
    read_engine_json()
        .get("alibaba_model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| ALIBABA_DEFAULT_MODEL.to_string())
}

/// 返回当前活跃在线引擎的区域标识。
pub fn read_online_asr_region() -> String {
    match read_engine_config().as_str() {
        "alibaba-asr" => read_alibaba_region(),
        _ => read_glm_region(),
    }
}

/// 返回当前活跃在线引擎的端点域名（不含路径）。
pub fn read_online_asr_endpoint() -> String {
    match read_engine_config().as_str() {
        "alibaba-asr" => read_alibaba_endpoint(),
        _ => read_glm_endpoint(),
    }
}

/// 返回 GLM-ASR 端点域名（不含路径）。
pub fn read_glm_endpoint() -> String {
    match read_glm_region().as_str() {
        "domestic" => GLM_ENDPOINT_DOMESTIC.to_string(),
        _ => GLM_ENDPOINT_INTERNATIONAL.to_string(),
    }
}

/// 返回 Alibaba DashScope 端点域名（不含路径）。
pub fn read_alibaba_endpoint() -> String {
    match read_alibaba_region().as_str() {
        "domestic" => ALIBABA_ENDPOINT_DOMESTIC.to_string(),
        _ => ALIBABA_ENDPOINT_INTERNATIONAL.to_string(),
    }
}

/// Serializes read-modify-write updates so concurrent writers keep each other's fields.
fn update_engine_json(update: impl FnOnce(&mut serde_json::Value)) -> Result<(), std::io::Error> {
    static MUTATION: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _mutation = MUTATION.lock();
    let mut obj = read_engine_json();
    update(&mut obj);
    write_engine_json(&obj)
}

fn update_engine_json_field(key: &str, value: &str) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj[key] = serde_json::Value::String(value.to_string()))
}

/// 写入 GLM-ASR 区域。`region`: `"international"` 或 `"domestic"`
pub fn write_glm_region(region: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("glm_endpoint", region)
}

/// 写入 Alibaba ASR 区域。`region`: `"international"` 或 `"domestic"`
pub fn write_alibaba_region(region: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("alibaba_region", region)
}

/// 写入 Alibaba ASR 当前选择的模型 ID。
pub fn write_alibaba_model(model: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("alibaba_model", model)
}

/// 根据当前引擎写入对应区域配置。
pub fn write_online_asr_endpoint(region: &str) -> Result<(), std::io::Error> {
    match read_engine_config().as_str() {
        "alibaba-asr" => write_alibaba_region(region),
        _ => write_glm_region(region),
    }
}

/// 查找已解压的 engine.exe
///
/// 仅检查数据目录（从引擎归档解压后的位置）。
pub fn get_engine_exe_path(_app: &tauri::AppHandle) -> Option<PathBuf> {
    let data_engine = get_data_dir().join("engine").join("engine.exe");
    if data_engine.exists() {
        return Some(data_engine);
    }
    None
}

/// 查找资源目录中的 engine.exe（开发时直接放置 python-dist 的情况）。
pub fn get_resource_engine_exe_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let resource_engine = resource_dir
            .join("resources")
            .join("python-dist")
            .join("engine")
            .join("engine.exe");
        if resource_engine.exists() {
            return Some(resource_engine);
        }
    }
    None
}

/// 查找打包的引擎归档（NSIS 安装后存在于资源目录）
///
/// 跳过空文件（build.rs 为 dev 模式创建的占位文件）。
pub fn get_engine_archive_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let resources_dir = resource_dir.join("resources");
        for filename in ["engine.tar.xz", "engine.zip"] {
            let archive = resources_dir.join(filename);
            if archive.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                return Some(archive);
            }
        }
    }
    None
}

/// 获取 engine 解压目标目录
pub fn get_engine_dir() -> PathBuf {
    get_data_dir().join("engine")
}

pub fn write_engine_config(engine: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("engine", engine)
}

/// 用户未配置时关闭。0 表示不自动卸载 GPU 模型。
pub const MAX_GPU_IDLE_SECONDS: u64 = 7 * 24 * 60 * 60;

pub fn parse_gpu_idle_seconds(value: Option<&serde_json::Value>) -> u64 {
    let Some(value) = value else {
        return 0;
    };
    let seconds = match value {
        serde_json::Value::Number(number) => number.as_u64().unwrap_or(0),
        serde_json::Value::String(text) => text.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    };
    seconds.min(MAX_GPU_IDLE_SECONDS)
}

/// Speech-processing preferences. Both default on; a missing or malformed value
/// keeps the default rather than silently disabling the feature.
pub fn read_silence_filter() -> bool {
    read_bool_setting("silence_filter", true)
}

pub fn write_silence_filter(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["silence_filter"] = serde_json::Value::Bool(enabled))
}

pub fn read_remove_fillers() -> bool {
    read_bool_setting("remove_fillers", true)
}

pub fn write_remove_fillers(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["remove_fillers"] = serde_json::Value::Bool(enabled))
}

/// Recognition language saved for the local transcribe.cpp models.
///
/// `None` and `"auto"` both mean "let the model detect it". The saved value is
/// not tied to one model: the backend resolves it against the selected model's
/// language list at transcription time.
pub fn read_asr_language() -> Option<String> {
    read_engine_json()
        .get("asr_language")
        .and_then(|value| value.as_str())
        .and_then(normalize_language_setting)
}

fn normalize_language_setting(value: &str) -> Option<String> {
    let value = value.trim().to_lowercase();
    (!value.is_empty() && value != "auto").then_some(value)
}

pub fn write_asr_language(language: Option<&str>) -> Result<(), std::io::Error> {
    let normalized = language.and_then(normalize_language_setting);
    update_engine_json(|obj| match normalized {
        Some(value) => obj["asr_language"] = serde_json::Value::String(value),
        None => {
            if let Some(map) = obj.as_object_mut() {
                map.remove("asr_language");
            }
        }
    })
}

/// Ask a translation-capable model to write English from any spoken language.
/// Off by default; the backend also checks the selected model supports it.
pub fn read_translate_to_english() -> bool {
    read_bool_setting("translate_to_english", false)
}

pub fn write_translate_to_english(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["translate_to_english"] = serde_json::Value::Bool(enabled))
}

fn read_bool_setting(key: &str, default: bool) -> bool {
    read_engine_json()
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

/// Spoken shortcuts and the text they insert. Malformed entries are dropped
/// rather than failing the whole load.
pub fn read_snippets() -> Vec<crate::services::snippets::Snippet> {
    let config = read_engine_json();
    let Some(entries) = config.get("snippets").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let parsed = entries.iter().filter_map(|entry| {
        serde_json::from_value::<crate::services::snippets::Snippet>(entry.clone()).ok()
    });
    crate::services::snippets::normalize(parsed)
}

pub fn write_snippets(
    snippets: &[crate::services::snippets::Snippet],
) -> Result<(), std::io::Error> {
    let normalized = crate::services::snippets::normalize(snippets.iter().cloned());
    update_engine_json(|obj| {
        if normalized.is_empty() {
            if let Some(map) = obj.as_object_mut() {
                map.remove("snippets");
            }
        } else {
            obj["snippets"] = serde_json::json!(normalized);
        }
    })
}

/// The writing style applied by AI polish. Unknown values read as `Off`.
pub fn read_tone_mode() -> crate::services::tone_modes::ToneMode {
    crate::services::tone_modes::ToneMode::parse(
        read_engine_json()
            .get("tone_mode")
            .and_then(|value| value.as_str())
            .unwrap_or("off"),
    )
}

pub fn write_tone_mode(mode: crate::services::tone_modes::ToneMode) -> Result<(), std::io::Error> {
    update_engine_json_field("tone_mode", mode.as_str())
}

/// Collapse repeated words and phrases in the local transcript. On by default:
/// it only removes text that speech recognition duplicated.
pub fn read_tidy_transcript() -> bool {
    read_bool_setting("tidy_transcript", true)
}

pub fn write_tidy_transcript(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["tidy_transcript"] = serde_json::Value::Bool(enabled))
}

/// Trailing spoken commands such as "press enter". Off by default because the
/// phrases are only unambiguous at the end of a dictation.
pub fn read_voice_commands() -> bool {
    read_bool_setting("voice_commands", false)
}

pub fn write_voice_commands(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["voice_commands"] = serde_json::Value::Bool(enabled))
}

/// Extra input gain for quiet speech. Off by default.
pub fn read_whisper_mode() -> bool {
    read_bool_setting("whisper_mode", false)
}

pub fn write_whisper_mode(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["whisper_mode"] = serde_json::Value::Bool(enabled))
}

/// Refuse every outbound network request. Off by default.
pub fn read_local_only() -> bool {
    read_bool_setting("local_only", false)
}

pub fn write_local_only(enabled: bool) -> Result<(), std::io::Error> {
    update_engine_json(|obj| obj["local_only"] = serde_json::Value::Bool(enabled))
}

/// Extra read-only folders searched for catalog model files, in order.
///
/// Entries are trimmed, and empty, relative or duplicate paths are dropped.
/// Nothing is ever written to or deleted from these folders.
pub fn read_model_dirs() -> Vec<String> {
    let config = read_engine_json();
    let Some(entries) = config.get("model_dirs").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    normalize_model_dirs(entries.iter().filter_map(|entry| entry.as_str()))
}

pub fn write_model_dirs(dirs: &[String]) -> Result<(), std::io::Error> {
    let normalized = normalize_model_dirs(dirs.iter().map(String::as_str));
    update_engine_json(|obj| {
        if normalized.is_empty() {
            if let Some(map) = obj.as_object_mut() {
                map.remove("model_dirs");
            }
        } else {
            obj["model_dirs"] = serde_json::json!(normalized);
        }
    })
}

pub fn normalize_model_dirs<'a>(dirs: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    dirs.map(str::trim)
        .filter(|dir| !dir.is_empty())
        .filter(|dir| Path::new(dir).is_absolute())
        // The engine receives the list joined by ';', which Windows allows in a
        // folder name, so such a path could never round-trip.
        .filter(|dir| !dir.contains(';'))
        .filter(|dir| seen.insert(dir.to_ascii_lowercase()))
        .map(str::to_string)
        .collect()
}

pub fn read_gpu_idle_seconds() -> u64 {
    parse_gpu_idle_seconds(read_engine_json().get("gpu_idle_seconds"))
}

fn gpu_idle_timeout_from_config(config: &serde_json::Value) -> u64 {
    let remembered = parse_gpu_idle_seconds(config.get("gpu_idle_timeout_seconds"));
    if remembered > 0 {
        remembered
    } else {
        parse_gpu_idle_seconds(config.get("gpu_idle_seconds"))
    }
}

pub fn read_gpu_idle_timeout_seconds() -> u64 {
    gpu_idle_timeout_from_config(&read_engine_json())
}

pub fn write_gpu_idle_seconds(
    seconds: u64,
    timeout_seconds: Option<u64>,
) -> Result<(), std::io::Error> {
    let seconds = seconds.min(MAX_GPU_IDLE_SECONDS);
    update_engine_json(|obj| {
        // Keep the effective value (0 = off) separate from the user's last timeout.
        let remembered = timeout_seconds
            .filter(|value| *value > 0)
            .unwrap_or_else(|| {
                if seconds > 0 {
                    seconds
                } else {
                    gpu_idle_timeout_from_config(obj)
                }
            })
            .min(MAX_GPU_IDLE_SECONDS);
        if let Some(map) = obj.as_object_mut() {
            map.insert("gpu_idle_seconds".to_string(), serde_json::json!(seconds));
            map.insert(
                "gpu_idle_timeout_seconds".to_string(),
                serde_json::json!(remembered),
            );
        }
    })
}

/// 读取用户自定义模型目录（None 表示使用默认 HF 缓存）
pub fn read_models_dir() -> Option<String> {
    read_engine_json()
        .get("models_dir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 写入自定义模型目录（None 表示恢复默认）
pub fn write_models_dir(dir: Option<&str>) -> Result<(), std::io::Error> {
    update_engine_json(|obj| {
        if let Some(map) = obj.as_object_mut() {
            match dir.filter(|s| !s.is_empty()) {
                Some(d) => {
                    map.insert(
                        "models_dir".to_string(),
                        serde_json::Value::String(d.to_string()),
                    );
                }
                None => {
                    map.remove("models_dir");
                }
            }
        }
    })
}

/// 默认 HF 缓存根目录（不考虑自定义配置）
pub fn get_default_models_dir() -> PathBuf {
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        return PathBuf::from(hf_home).join("hub");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".cache").join("huggingface").join("hub");
    }
    PathBuf::from(".cache").join("huggingface").join("hub")
}

/// 获取生效的模型缓存目录：自定义路径 > HF_HOME > 默认
pub fn get_effective_models_dir() -> PathBuf {
    if let Some(custom) = read_models_dir() {
        return PathBuf::from(custom);
    }
    get_default_models_dir()
}

#[cfg(test)]
#[path = "paths_concurrency_tests.rs"]
mod concurrency_tests;

#[cfg(test)]
mod tests {
    use super::engine_json_object_or_empty;

    #[test]
    fn alibaba_default_and_fallback_prefer_native_model_and_retain_legacy() {
        assert_eq!(super::ALIBABA_DEFAULT_MODEL, "qwen-audio-3.1-asr-flash");
        assert_eq!(
            super::ALIBABA_FALLBACK_MODEL_IDS.first().copied(),
            Some("qwen-audio-3.1-asr-flash")
        );
        assert!(super::ALIBABA_FALLBACK_MODEL_IDS.contains(&"qwen3-asr-flash"));
    }

    #[test]
    fn engine_selection_keeps_every_supported_engine() {
        for engine in super::local_asr_engines().chain(super::ONLINE_ASR_ENGINES.iter().copied()) {
            assert_eq!(super::configured_engine(Some(engine)), engine);
        }
        assert_eq!(super::configured_engine(None), "qwen3-asr-1.7b");
        assert_eq!(super::configured_engine(Some("invalid")), "qwen3-asr-1.7b");
    }

    #[test]
    fn model_dirs_drop_empty_relative_and_duplicate_paths() {
        let normalized = super::normalize_model_dirs(
            [
                r"  C:\models  ",
                r"C:\models",
                r"relative\dir",
                "",
                "   ",
                r"D:\hex\models",
                r"E:\odd;name",
            ]
            .into_iter(),
        );
        assert_eq!(normalized, vec![r"C:\models", r"D:\hex\models"]);
    }

    #[test]
    fn speech_processing_defaults_to_on_without_a_saved_value() {
        assert!(super::read_bool_setting("silence_filter", true));
        assert!(super::read_bool_setting("remove_fillers", true));
        assert!(!super::read_bool_setting("missing_key", false));
    }

    #[test]
    fn language_setting_trims_and_treats_auto_as_unset() {
        assert_eq!(super::normalize_language_setting(" FR "), Some("fr".into()));
        assert_eq!(super::normalize_language_setting("auto"), None);
        assert_eq!(super::normalize_language_setting("  "), None);
    }

    #[test]
    fn gpu_idle_seconds_default_off_and_reject_garbage() {
        assert_eq!(super::parse_gpu_idle_seconds(None), 0);
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!(0))),
            0
        );
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!(180))),
            180
        );
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!(-5))),
            0
        );
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!("nope"))),
            0
        );
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!(true))),
            0
        );
        assert_eq!(
            super::parse_gpu_idle_seconds(Some(&serde_json::json!(
                super::MAX_GPU_IDLE_SECONDS + 10
            ))),
            super::MAX_GPU_IDLE_SECONDS
        );
    }

    #[test]
    fn gpu_idle_timeout_survives_disable_and_restart() {
        // A fresh process isolates DATA_DIR's OnceLock from every other test.
        if std::env::var_os("SPEAK_FLOW_GPU_IDLE_TEST_CHILD").is_none() {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "speak-flow-gpu-idle-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "utils::paths::tests::gpu_idle_timeout_survives_disable_and_restart",
                    "--nocapture",
                ])
                .env("SPEAK_FLOW_DATA_DIR", &dir)
                .env("SPEAK_FLOW_GPU_IDLE_TEST_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let restarted = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "utils::paths::tests::gpu_idle_timeout_survives_disable_and_restart",
                    "--nocapture",
                ])
                .env("SPEAK_FLOW_DATA_DIR", &dir)
                .env("SPEAK_FLOW_GPU_IDLE_TEST_CHILD", "restart")
                .output()
                .unwrap();
            assert!(
                restarted.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&restarted.stdout),
                String::from_utf8_lossy(&restarted.stderr)
            );
            std::fs::remove_file(dir.join("engine.json")).unwrap();
            std::fs::remove_dir(dir).unwrap();
            return;
        }
        if std::env::var("SPEAK_FLOW_GPU_IDLE_TEST_CHILD").as_deref() == Ok("restart") {
            assert_eq!(super::read_gpu_idle_seconds(), 0);
            assert_eq!(super::read_gpu_idle_timeout_seconds(), 75);
            super::write_gpu_idle_seconds(super::read_gpu_idle_timeout_seconds(), None).unwrap();
            assert_eq!(super::read_gpu_idle_seconds(), 75);
            return;
        }
        assert_eq!(super::read_gpu_idle_seconds(), 0);
        super::atomic_write(
            &super::get_engine_config_path(),
            br#"{"engine":"confucius4-r2t2","gpu_idle_seconds":45}"#,
        )
        .unwrap();
        assert_eq!(super::read_gpu_idle_timeout_seconds(), 45);
        super::write_gpu_idle_seconds(60, None).unwrap();
        assert_eq!(super::read_gpu_idle_seconds(), 60);
        super::write_gpu_idle_seconds(0, None).unwrap();
        assert_eq!(super::read_gpu_idle_seconds(), 0);
        let config: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(super::get_engine_config_path()).unwrap(),
        )
        .unwrap();
        assert_eq!(config["gpu_idle_timeout_seconds"], 60);
        assert_eq!(config["engine"], "confucius4-r2t2");
        super::write_gpu_idle_seconds(0, Some(75)).unwrap();
        assert_eq!(super::read_gpu_idle_seconds(), 0);
        assert_eq!(super::read_gpu_idle_timeout_seconds(), 75);
    }

    #[test]
    fn engine_json_string_normalizes_to_empty_object() {
        let value = serde_json::json!("legacy-string");
        let normalized = engine_json_object_or_empty(value);

        assert!(normalized.is_object());
        assert!(normalized.as_object().unwrap().is_empty());
    }

    #[test]
    fn engine_json_array_normalizes_to_empty_object() {
        let value = serde_json::json!(["legacy-array"]);
        let normalized = engine_json_object_or_empty(value);

        assert!(normalized.is_object());
        assert!(normalized.as_object().unwrap().is_empty());
    }
}
