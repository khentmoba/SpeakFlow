//! FunASR engine discovery and installation helpers.

use crate::state::AppState;
use crate::utils::{paths, AppError};
use sha2::{Digest, Sha256};
use std::{future::Future, path::PathBuf, time::Duration};
use tauri::Emitter;
use tokio::process::Command;

use super::{now_unix_ms, EngineProgressGate, ENGINE_ARCHIVE_FINGERPRINT};

/// Engine run mode
pub enum EngineRuntime {
    /// Production mode: run the bundled engine.exe directly
    Bundled { exe_path: String },
    /// Development mode: use the system Python interpreter + .py scripts
    Development { python_path: String },
}

fn to_normalized_path(path: &std::path::Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    paths::strip_win_prefix(&canonical)
}

const EMPTY_ARCHIVE_FINGERPRINT: &str =
    "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const PUBLISHED_ENGINE_ARCHIVE_FINGERPRINT: &str =
    "sha256:76fae1f69d274bab6f18d1054c92bc3e0caa13d01f4aa93e98424f801b155391";

fn effective_engine_archive_fingerprint(build_fingerprint: &str) -> &str {
    if build_fingerprint != EMPTY_ARCHIVE_FINGERPRINT {
        build_fingerprint
    } else {
        PUBLISHED_ENGINE_ARCHIVE_FINGERPRINT
    }
}

fn expected_engine_install_fingerprint(app_handle: &tauri::AppHandle) -> String {
    if ENGINE_ARCHIVE_FINGERPRINT != EMPTY_ARCHIVE_FINGERPRINT
        || paths::get_engine_archive_path(app_handle).is_some()
    {
        ENGINE_ARCHIVE_FINGERPRINT.to_string()
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

pub(super) fn engine_install_fingerprint_matches(installed: &str, expected: &str) -> bool {
    let installed = installed.trim();
    let effective_archive = effective_engine_archive_fingerprint(ENGINE_ARCHIVE_FINGERPRINT);
    installed == expected
        || installed == effective_archive
        || ((expected == ENGINE_ARCHIVE_FINGERPRINT || expected == effective_archive)
            && installed
                .rsplit_once('+')
                .is_some_and(|(_, archive_fingerprint)| {
                    archive_fingerprint == ENGINE_ARCHIVE_FINGERPRINT
                        || archive_fingerprint == effective_archive
                }))
}

/// Locate the engine runtime
///
/// In priority order:
/// 1. engine.exe already extracted to the data directory (reused directly when the version matches)
/// 2. An unextracted engine archive (engine.tar.xz / legacy engine.zip) → extract, then use
/// 3. engine.exe in the resource directory (python-dist placed directly during development)
/// 4. System Python (development mode)
pub async fn find_engine(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    expected_generation: u64,
) -> Result<EngineRuntime, AppError> {
    let progress_gate = EngineProgressGate {
        generation: state.engine.funasr_generation.clone(),
        status_commit: state.engine.funasr_status_commit.clone(),
        expected_generation,
    };
    let _install_guard = state.engine.engine_install_op.lock().await;
    if !progress_gate.is_current() {
        return Err(AppError::Asr("Engine lookup was cancelled".to_string()));
    }
    let expected_fingerprint = expected_engine_install_fingerprint(app_handle);

    // Strategy 1: already-extracted engine.exe (used when the version matches)
    if let Some(engine_path) = paths::get_engine_exe_path(app_handle) {
        let version_file = paths::get_engine_dir().join(".version");
        let installed_version = std::fs::read_to_string(&version_file).unwrap_or_default();

        if engine_install_fingerprint_matches(&installed_version, &expected_fingerprint) {
            let path_str = paths::strip_win_prefix(&engine_path);
            log::info!("Found engine: {} ({})", path_str, expected_fingerprint);
            return Ok(EngineRuntime::Bundled { exe_path: path_str });
        }

        log::info!(
            "Engine fingerprint mismatch (installed: {:?}, current: {}), re-extracting",
            installed_version.trim(),
            expected_fingerprint
        );
    }

    // Strategy 2: an engine archive exists and must be extracted (first launch or version upgrade)
    if let Some(archive_path) = paths::get_engine_archive_path(app_handle) {
        log::info!(
            "Found engine archive, preparing to extract: {}",
            archive_path.display()
        );
        match extract_engine_archive(&archive_path, app_handle, &progress_gate).await {
            Ok(engine_exe) => {
                let path_str = paths::strip_win_prefix(&engine_exe);
                log::info!("Engine extraction complete: {}", path_str);
                return Ok(EngineRuntime::Bundled { exe_path: path_str });
            }
            // Lightweight updates can leave an older bundled archive behind.
            Err(AppError::Download(err)) => {
                log::warn!(
                    "Ignoring unverifiable local engine archive {}: {}. Falling back to engine download.",
                    archive_path.display(),
                    err
                );
            }
            Err(err) => return Err(err),
        }
    }

    // Unpackaged executables are a development-only fallback.
    if cfg!(debug_assertions) {
        if let Some(engine_path) = paths::get_resource_engine_exe_path(app_handle) {
            let path_str = paths::strip_win_prefix(&engine_path);
            log::info!("Found engine in resources directory: {}", path_str);
            return Ok(EngineRuntime::Bundled { exe_path: path_str });
        }
    }

    // Strategy 4: development mode
    if let Ok(python_path) = find_python().await {
        return Ok(EngineRuntime::Development { python_path });
    }

    // Strategy 5: download the engine archive online (first run of the lightweight installer or an engine upgrade)
    log::info!("No local speech engine found; attempting online download...");
    let engine_exe = download_and_extract_engine(app_handle, &progress_gate).await?;
    let path_str = paths::strip_win_prefix(&engine_exe);
    log::info!("Engine download and extraction complete: {}", path_str);
    Ok(EngineRuntime::Bundled { exe_path: path_str })
}

/// Extract the engine archive into the data directory
async fn extract_engine_archive(
    archive_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<std::path::PathBuf, AppError> {
    progress_gate.commit_if_current(|| {
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "First startup: extracting engine files..."
            }),
        );
    });

    let engine_dir = paths::get_engine_dir();
    let archive = archive_path.to_path_buf();
    let handle = app_handle.clone();
    let progress_gate = progress_gate.clone();

    // Extraction is CPU-bound and IO-bound, so run it on a blocking thread
    tokio::task::spawn_blocking(move || {
        verify_engine_archive(
            &archive,
            effective_engine_archive_fingerprint(ENGINE_ARCHIVE_FINGERPRINT),
        )?;
        let parent = engine_dir.parent().ok_or_else(|| {
            AppError::Asr("Engine directory missing parent directory".to_string())
        })?;
        std::fs::create_dir_all(parent).map_err(|e| {
            AppError::Asr(format!("Failed to create engine parent directory: {}", e))
        })?;

        let stamp = now_unix_ms();
        let staging_dir = parent.join(format!("engine.staging.{}", stamp));
        let backup_dir = parent.join(format!("engine.backup.{}", stamp));

        if staging_dir.exists() {
            let _ = std::fs::remove_dir_all(&staging_dir);
        }
        std::fs::create_dir_all(&staging_dir)
            .map_err(|e| AppError::Asr(format!("Failed to create engine directory: {}", e)))?;

        let extract_result = (|| -> Result<usize, AppError> {
            let archive_name = archive
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();

            let total = if archive_name.ends_with(".tar.xz") {
                extract_tar_xz_archive(&archive, &staging_dir, &handle, &progress_gate)?
            } else {
                extract_zip_archive(&archive, &staging_dir, &handle, &progress_gate)?
            };

            if total == 0 {
                return Err(AppError::Asr("Engine archive is empty".to_string()));
            }
            let staged_exe = staging_dir.join("engine.exe");
            if !staged_exe.is_file() {
                return Err(AppError::Asr(
                    "Engine archive is missing engine.exe".to_string(),
                ));
            }

            // Write a version marker for later upgrade detection; write only to staging and swap everything in after verification succeeds.
            let fingerprint = effective_engine_archive_fingerprint(ENGINE_ARCHIVE_FINGERPRINT);
            paths::atomic_write(
                staging_dir.join(".version").as_path(),
                fingerprint.as_bytes(),
            )
            .map_err(|e| AppError::Asr(format!("Failed to write engine version tag: {}", e)))?;

            Ok(total)
        })();

        let total = match extract_result {
            Ok(total) => total,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return Err(err);
            }
        };

        replace_engine_dir(&engine_dir, &staging_dir, &backup_dir)?;
        let _ = std::fs::remove_dir_all(&backup_dir);

        log::info!("Engine extraction complete: {} entries", total);
        Ok(engine_dir.join("engine.exe"))
    })
    .await
    .map_err(|e| AppError::Asr(format!("Extraction task panicked: {}", e)))?
}

pub(super) fn replace_engine_dir(
    engine_dir: &std::path::Path,
    staging_dir: &std::path::Path,
    backup_dir: &std::path::Path,
) -> Result<(), AppError> {
    if backup_dir.exists() {
        let _ = std::fs::remove_dir_all(backup_dir);
    }

    let had_previous = engine_dir.exists();
    if had_previous {
        std::fs::rename(engine_dir, backup_dir)
            .map_err(|e| AppError::Asr(format!("Failed to backup old engine: {}", e)))?;
    }

    match std::fs::rename(staging_dir, engine_dir) {
        Ok(()) => Ok(()),
        Err(err) => {
            if had_previous {
                if let Err(restore_err) = std::fs::rename(backup_dir, engine_dir) {
                    return Err(AppError::Asr(format!(
                        "Failed to replace engine directory: {}; failed to restore old engine: {} (backup kept at {})",
                        err,
                        restore_err,
                        backup_dir.display()
                    )));
                }
            }
            Err(AppError::Asr(format!(
                "Failed to replace engine directory: {}",
                err
            )))
        }
    }
}

fn extract_zip_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("Failed to open engine archive: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Asr(format!("Failed to read engine archive: {}", e)))?;

    let total = zip.len();
    log::info!("Starting ZIP engine archive extraction: {} files", total);

    for i in 0..total {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Asr(format!("Failed to read archive entry: {}", e)))?;

        let entry_path = engine_dir.join(
            entry
                .enclosed_name()
                .ok_or_else(|| AppError::Asr("Archive contains unsafe path".to_string()))?,
        );

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .map_err(|e| AppError::Asr(format!("Failed to create directory: {}", e)))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::Asr(format!("Failed to create parent directory: {}", e))
                })?;
            }
            let mut outfile = std::fs::File::create(&entry_path)
                .map_err(|e| AppError::Asr(format!("Failed to create file: {}", e)))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| AppError::Asr(format!("Failed to write file: {}", e)))?;
        }

        report_extract_progress(handle, progress_gate, i + 1, Some(total), false);
    }

    Ok(total)
}

fn extract_tar_xz_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<usize, AppError> {
    log::info!("Starting TAR.XZ engine archive extraction");

    let extracted = unpack_tar_xz_entries(archive, engine_dir, |count| {
        report_extract_progress(handle, progress_gate, count, None, false);
    })?;

    if extracted > 0 && !extracted.is_multiple_of(200) {
        report_extract_progress(handle, progress_gate, extracted, None, true);
    }

    Ok(extracted)
}

fn unpack_tar_xz_entries(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    mut on_entry: impl FnMut(usize),
) -> Result<usize, AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("Failed to open engine archive: {}", e)))?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut extracted = 0usize;

    for entry_result in tar
        .entries()
        .map_err(|e| AppError::Asr(format!("Failed to read engine archive: {}", e)))?
    {
        let mut entry = entry_result
            .map_err(|e| AppError::Asr(format!("Failed to read archive entry: {}", e)))?;
        // Ok(false) means tar refused the entry (a `..` component) and wrote nothing.
        let unpacked = entry
            .unpack_in(engine_dir)
            .map_err(|e| AppError::Asr(format!("Failed to write file: {}", e)))?;
        if !unpacked {
            return Err(AppError::Asr("Archive contains unsafe path".to_string()));
        }
        extracted += 1;
        on_entry(extracted);
    }

    Ok(extracted)
}

fn report_extract_progress(
    handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
    current: usize,
    total: Option<usize>,
    force: bool,
) {
    let should_emit = force || current.is_multiple_of(200) || total.is_some_and(|t| current == t);

    if !should_emit {
        return;
    }
    progress_gate.commit_if_current(|| {
        if let Some(total) = total {
            if total == 0 {
                return;
            }
            let pct = current * 100 / total;
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("Extracting engine files... {}%", pct)
                }),
            );
        } else {
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "loading",
                    "message": format!("Extracting engine files... {} items processed", current)
                }),
            );
        }
    });
}

struct TempFileGuard<'a>(&'a std::path::Path);
impl<'a> Drop for TempFileGuard<'a> {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_file(self.0);
        }
    }
}

fn verify_engine_archive(archive: &std::path::Path, expected: &str) -> Result<(), AppError> {
    let mut file = std::fs::File::open(archive)
        .map_err(|e| AppError::Download(format!("Failed to open engine archive: {e}")))?;
    let mut digest = Sha256::new();
    let size = std::io::copy(&mut file, &mut digest)
        .map_err(|e| AppError::Download(format!("Failed to hash engine archive: {e}")))?;
    let actual = format!("sha256:{:x}", digest.finalize());
    if size == 0 || actual != expected {
        return Err(AppError::Download(
            "Engine archive checksum mismatch; refusing to install unverified code".into(),
        ));
    }
    Ok(())
}

async fn download_step<T>(
    progress_gate: &EngineProgressGate,
    operation: impl Future<Output = Result<T, reqwest::Error>>,
) -> Result<T, AppError> {
    let operation = tokio::time::timeout(Duration::from_secs(30), operation);
    tokio::pin!(operation);
    loop {
        if !progress_gate.is_current() {
            return Err(AppError::Asr("Engine download was cancelled".into()));
        }
        tokio::select! {
            result = &mut operation => {
                return result
                    .map_err(|_| AppError::Download("Engine download stalled for 30 seconds".into()))?
                    .map_err(|e| AppError::Download(format!("Engine download failed: {e}")));
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
}

fn candidate_engine_download_urls() -> Vec<String> {
    let mut urls = Vec::new();

    if let Ok(custom_url) = std::env::var("SPEAK_FLOW_ENGINE_URL") {
        let trimmed = custom_url.trim();
        if !trimmed.is_empty() {
            urls.push(trimmed.to_string());
        }
    }

    let default_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    let raw_tag = env!("CARGO_PKG_VERSION");

    urls.push(format!(
        "https://github.com/khentmoba/SpeakFlow/releases/download/{}/engine.tar.xz",
        raw_tag
    ));
    urls.push(format!(
        "https://github.com/khentmoba/SpeakFlow/releases/download/{}/engine.tar.xz",
        default_tag
    ));
    urls
}

async fn download_to_file(
    client: &reqwest::Client,
    url: &str,
    target_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<(), AppError> {
    let mut response = download_step(progress_gate, client.get(url).send()).await?;

    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    let total_size = response.content_length();
    let mut file = tokio::fs::File::create(target_path)
        .await
        .map_err(|e| AppError::Download(format!("Failed to create temporary file: {e}")))?;

    let mut downloaded = 0_u64;
    let mut last_percent = None;

    while let Some(chunk) = download_step(progress_gate, response.chunk()).await? {
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| AppError::Download(format!("Failed to write temporary file: {e}")))?;

        downloaded += chunk.len() as u64;
        let percent = total_size.map(|total| {
            if total > 0 {
                (downloaded * 100 / total).min(100)
            } else {
                0
            }
        });

        if percent != last_percent {
            last_percent = percent;
            let pct = percent.unwrap_or(0);
            let mb_downloaded = (downloaded as f64) / (1024.0 * 1024.0);
            let message = if let Some(total) = total_size {
                let mb_total = (total as f64) / (1024.0 * 1024.0);
                format!(
                    "First startup: downloading speech engine ({:.1}MB / {:.1}MB, {}%)...",
                    mb_downloaded, mb_total, pct
                )
            } else {
                format!(
                    "First startup: downloading speech engine ({:.1}MB)...",
                    mb_downloaded
                )
            };

            progress_gate.commit_if_current(|| {
                let _ = app_handle.emit(
                    "funasr-status",
                    serde_json::json!({
                        "status": "loading",
                        "message": &message,
                    }),
                );
                let _ = app_handle.emit(
                    "model-download-status",
                    serde_json::json!({
                        "status": "downloading",
                        "model": "engine",
                        "progress": pct,
                        "overall_progress": pct,
                        "message": &message,
                    }),
                );
            });
        }
    }

    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| AppError::Download(format!("Failed to flush temporary file: {e}")))?;

    if downloaded == 0 {
        let _ = tokio::fs::remove_file(target_path).await;
        return Err(AppError::Download(
            "Downloaded engine archive is empty".into(),
        ));
    }

    Ok(())
}

async fn download_and_extract_engine(
    app_handle: &tauri::AppHandle,
    progress_gate: &EngineProgressGate,
) -> Result<PathBuf, AppError> {
    progress_gate.commit_if_current(|| {
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "First startup: connecting to server to download speech engine components..."
            }),
        );
        let _ = app_handle.emit(
            "model-download-status",
            serde_json::json!({
                "status": "downloading",
                "model": "engine",
                "progress": 0,
                "overall_progress": 0,
                "message": "First startup: connecting to server to download speech engine components..."
            }),
        );
    });

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Download(format!("Failed to create HTTP client: {e}")))?;

    let urls = candidate_engine_download_urls();
    let temp_archive_path =
        paths::get_data_dir().join(format!("engine.download.{}.tar.xz", now_unix_ms()));
    let _cleanup_guard = TempFileGuard(&temp_archive_path);

    let mut last_err = None;
    let mut downloaded = false;

    for url in &urls {
        if !progress_gate.is_current() {
            return Err(AppError::Asr("Engine download was cancelled".to_string()));
        }

        log::info!("Attempting to download speech engine from: {}", url);
        match download_to_file(&client, url, &temp_archive_path, app_handle, progress_gate).await {
            Ok(()) => {
                downloaded = true;
                break;
            }
            Err(err) => {
                log::warn!("Failed to download speech engine from {}: {}", url, err);
                last_err = Some(err);
                if temp_archive_path.exists() {
                    let _ = std::fs::remove_file(&temp_archive_path);
                }
            }
        }
    }

    if !downloaded {
        return Err(last_err.unwrap_or_else(|| {
            AppError::Download(
                "Failed to download speech engine: all download sources unavailable".into(),
            )
        }));
    }

    // Download succeeded; run the extraction
    extract_engine_archive(&temp_archive_path, app_handle, progress_gate).await
}

/// Find a usable Python interpreter (development-mode fallback)
pub(super) async fn find_python() -> Result<String, AppError> {
    if !cfg!(debug_assertions) {
        return Err(AppError::Asr(
            "System Python is only supported in development builds".into(),
        ));
    }
    // ---- Strategy 1: check the project .venv virtual environment ----
    let mut venv_candidates = vec![PathBuf::from(".venv"), PathBuf::from("..").join(".venv")];
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            venv_candidates.push(exe_dir.join("..").join("..").join("..").join(".venv"));
            venv_candidates.push(
                exe_dir
                    .join("..")
                    .join("..")
                    .join("..")
                    .join("..")
                    .join(".venv"),
            );
        }
    }

    for venv_dir in &venv_candidates {
        let venv_python = venv_dir.join("Scripts").join("python.exe");

        if tokio::fs::try_exists(&venv_python).await.unwrap_or(false) {
            let path_str = to_normalized_path(&venv_python);
            log::info!("Found virtual environment Python: {}", path_str);
            return Ok(path_str);
        }
    }

    // ---- Strategy 2: search the system PATH ----
    // Try several possible Python command names
    let python_names = vec!["python.exe", "python3.exe", "python"];

    for name in &python_names {
        let check_cmd = Command::new("where").arg(name).output().await;

        if let Ok(output) = check_cmd {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !path.is_empty() {
                    let version_check = Command::new(&path).arg("--version").output().await;

                    if let Ok(ver_output) = version_check {
                        if ver_output.status.success() {
                            let version = String::from_utf8_lossy(&ver_output.stdout);
                            log::info!("Found system Python: {} ({})", path, version.trim());
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // All strategies failed
    Err(AppError::Asr(
        "No available Python interpreter found. Please install Python 3.8+ or create a .venv virtual environment in the project directory (uv recommended)."
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn progress_gate() -> EngineProgressGate {
        EngineProgressGate {
            generation: Arc::new(AtomicU64::new(1)),
            status_commit: Arc::new(parking_lot::Mutex::new(())),
            expected_generation: 1,
        }
    }

    #[test]
    fn tar_archive_entry_escaping_engine_dir_fails_extraction() {
        let root =
            std::env::temp_dir().join(format!("speak-flow-tar-escape-{}", rand::random::<u64>()));
        let engine_dir = root.join("engine");
        std::fs::create_dir_all(&engine_dir).expect("create engine dir");
        let archive = root.join("engine.tar.xz");

        let payload = b"escaped";
        let name = b"../escaped.txt";
        let mut header = tar::Header::new_gnu();
        header.as_old_mut().name[..name.len()].copy_from_slice(name);
        header.set_size(payload.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        let mut builder = tar::Builder::new(Vec::new());
        builder
            .append(&header, &payload[..])
            .expect("append escaping entry");
        let tar_bytes = builder.into_inner().expect("finish tar");
        let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 1);
        std::io::Write::write_all(&mut encoder, &tar_bytes).expect("compress tar");
        std::fs::write(&archive, encoder.finish().expect("finish xz")).expect("write archive");

        let result = unpack_tar_xz_entries(&archive, &engine_dir, |_| {});

        assert!(
            matches!(&result, Err(AppError::Asr(message)) if message.contains("unsafe path")),
            "escaping entry must fail extraction, got {result:?}"
        );
        assert!(!root.join("escaped.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_build_fingerprint_uses_published_archive() {
        assert_eq!(
            effective_engine_archive_fingerprint(EMPTY_ARCHIVE_FINGERPRINT),
            PUBLISHED_ENGINE_ARCHIVE_FINGERPRINT
        );
    }

    #[test]
    fn non_empty_build_fingerprint_keeps_its_own_archive() {
        let build_fingerprint =
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(
            effective_engine_archive_fingerprint(build_fingerprint),
            build_fingerprint
        );
    }

    #[test]
    fn downloaded_engine_is_reused_with_app_version_expectation() {
        let installed = effective_engine_archive_fingerprint(ENGINE_ARCHIVE_FINGERPRINT);
        let expected = env!("CARGO_PKG_VERSION");
        assert!(engine_install_fingerprint_matches(installed, expected));
        assert!(!engine_install_fingerprint_matches(
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            expected
        ));
    }

    #[test]
    fn archive_verification_rejects_modified_empty_and_unpinned_archives() {
        let path =
            std::env::temp_dir().join(format!("speak-flow-archive-{}", rand::random::<u64>()));
        let _cleanup = TempFileGuard(&path);
        let expected = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        std::fs::write(&path, b"abc").unwrap();
        verify_engine_archive(&path, expected).unwrap();
        assert!(verify_engine_archive(&path, "1.0.5").is_err());
        std::fs::write(&path, b"tampered executable").unwrap();
        assert!(verify_engine_archive(&path, expected).is_err());
        std::fs::write(&path, b"").unwrap();
        assert!(verify_engine_archive(
            &path,
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        )
        .is_err());
    }

    #[test]
    fn stale_local_archive_falls_back_without_bypassing_download_verification() {
        let source = include_str!("installation.rs");
        let discovery = source
            .split("pub async fn find_engine(")
            .nth(1)
            .unwrap()
            .split("async fn extract_engine_archive(")
            .next()
            .unwrap();
        let fallback = discovery.find("Err(AppError::Download(err)) =>").unwrap();
        let other_errors = discovery.find("Err(err) => return Err(err)").unwrap();
        let download = discovery
            .find("download_and_extract_engine(app_handle")
            .unwrap();
        assert!(fallback < other_errors && other_errors < download);

        let extraction = source
            .split("async fn extract_engine_archive(")
            .nth(1)
            .unwrap()
            .split("pub(super) fn replace_engine_dir(")
            .next()
            .unwrap();
        assert!(
            extraction.find("verify_engine_archive(").unwrap()
                < extraction.find("std::fs::create_dir_all(parent)").unwrap()
        );
        let download = source
            .split("async fn download_and_extract_engine(")
            .nth(1)
            .unwrap()
            .split("pub(super) async fn find_python()")
            .next()
            .unwrap();
        assert!(download.contains(
            "extract_engine_archive(&temp_archive_path, app_handle, progress_gate).await"
        ));
    }

    #[test]
    fn release_discovery_rejects_system_python_before_searching() {
        let source = include_str!("installation.rs");
        let discovery = source
            .split("pub(super) async fn find_python()")
            .nth(1)
            .unwrap();
        let guard = discovery.find("if !cfg!(debug_assertions)").unwrap();
        let rejection = discovery
            .find("System Python is only supported in development builds")
            .unwrap();
        let search = discovery.find("let mut venv_candidates").unwrap();
        assert!(guard < rejection && rejection < search);
    }

    async fn stalled_download(send_headers: bool, cancel: bool) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/engine.tar.xz", listener.local_addr().unwrap());
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            if send_headers {
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n")
                    .await
                    .unwrap();
            }
            ready_tx.send(()).unwrap();
            std::future::pending::<()>().await;
            drop(socket);
        });
        let gate = progress_gate();
        let worker_gate = gate.clone();
        let (headers_tx, headers_rx) = tokio::sync::oneshot::channel();
        let download = tokio::spawn(async move {
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let mut response = download_step(&worker_gate, client.get(url).send()).await?;
            let _ = headers_tx.send(());
            download_step(&worker_gate, response.chunk()).await
        });
        tokio::time::timeout(Duration::from_secs(5), ready_rx)
            .await
            .unwrap()
            .unwrap();
        if send_headers {
            tokio::time::timeout(Duration::from_secs(5), headers_rx)
                .await
                .unwrap()
                .unwrap();
        }
        if cancel {
            gate.generation.fetch_add(1, Ordering::SeqCst);
        }
        tokio::time::pause();
        let started = tokio::time::Instant::now();
        let error = download.await.unwrap().unwrap_err().to_string();
        if cancel {
            assert!(error.contains("cancelled"), "{error}");
            assert!(started.elapsed() <= Duration::from_millis(200));
        } else {
            assert!(error.contains("stalled"), "{error}");
            assert!(started.elapsed() <= Duration::from_secs(31));
        }
        server.abort();
        tokio::time::resume();
    }

    #[tokio::test]
    async fn engine_download_times_out_on_stalled_headers_and_body() {
        stalled_download(false, false).await;
        stalled_download(true, false).await;
    }

    #[tokio::test]
    async fn engine_download_cancels_without_waiting_for_headers_or_body() {
        stalled_download(false, true).await;
        stalled_download(true, true).await;
    }
}
