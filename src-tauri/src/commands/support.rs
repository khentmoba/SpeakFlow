//! Read-only support actions.
//!
//! `get_diagnostics` reads app-owned files only, bounds how much it reads, and
//! redacts credential-shaped lines. `open_logs_folder` resolves the app log
//! directory itself; neither command accepts a path from the frontend.

use crate::utils::AppError;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tauri::Manager;

/// Enough history to cover several dictation sessions without copying a whole log.
const LOG_TAIL_BYTES: u64 = 64 * 1024;
const MAX_DIAGNOSTIC_LINES: usize = 400;

/// Credential-shaped markers, matched case-insensitively. A line that contains
/// one keeps its prefix and loses the rest, so the report stays useful.
const SENSITIVE_MARKERS: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "authorization",
    "bearer",
    "password",
    "secret",
    "token",
    "cookie",
];

fn redact_line(line: &str) -> String {
    // `to_ascii_lowercase` preserves byte offsets, so the slice below stays on
    // a character boundary even when the line contains non-ASCII text.
    let lower = line.to_ascii_lowercase();
    let cut = SENSITIVE_MARKERS
        .iter()
        .filter_map(|marker| lower.find(marker))
        .min();
    match cut {
        Some(index) => {
            let prefix = line[..index].trim_end();
            if prefix.is_empty() {
                "[redacted]".to_string()
            } else {
                format!("{prefix} [redacted]")
            }
        }
        None => line.to_string(),
    }
}

/// The last `max_bytes` of a file, decoded lossily and redacted line by line.
fn redacted_tail(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let text = String::from_utf8_lossy(&buffer);
    let mut lines: Vec<String> = text.lines().map(redact_line).collect();
    if lines.len() > MAX_DIAGNOSTIC_LINES {
        lines = lines.split_off(lines.len() - MAX_DIAGNOSTIC_LINES);
    }
    Ok(lines.join("\n"))
}

fn log_dir(app: &tauri::AppHandle) -> Result<PathBuf, AppError> {
    app.path()
        .app_log_dir()
        .map_err(|error| AppError::Other(format!("Cannot resolve the log folder: {error}")))
}

fn diagnostics_report(app: &tauri::AppHandle) -> Result<String, AppError> {
    let logs = log_dir(app)?;
    let mut report = String::new();
    report.push_str(&format!("SpeakFlow {}\n", env!("CARGO_PKG_VERSION")));
    report.push_str(&format!(
        "os: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    report.push_str(&format!(
        "engine: {}\n",
        crate::utils::paths::read_engine_config()
    ));
    report.push_str(&format!(
        "models: {}\n",
        crate::utils::paths::get_effective_models_dir().display()
    ));
    report.push_str(&format!(
        "data: {}\n",
        crate::utils::paths::get_data_dir().display()
    ));
    report.push_str(&format!("logs: {}\n", logs.display()));

    let app_log = logs.join("app.log");
    match redacted_tail(&app_log, LOG_TAIL_BYTES) {
        Ok(tail) if !tail.trim().is_empty() => {
            report.push_str("\n--- app.log (tail, redacted) ---\n");
            report.push_str(&tail);
            report.push('\n');
        }
        Ok(_) => report.push_str("\n--- app.log is empty ---\n"),
        Err(error) => report.push_str(&format!("\n--- app.log unavailable: {error} ---\n")),
    }
    Ok(report)
}

/// A bounded, redacted report the user can paste into an issue.
#[tauri::command]
pub fn get_diagnostics(app: tauri::AppHandle) -> Result<String, AppError> {
    diagnostics_report(&app)
}

/// Open the log folder in the platform file manager.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) -> Result<String, AppError> {
    let folder = log_dir(&app)?;
    if !folder.exists() {
        return Err(AppError::Other(format!(
            "The log folder does not exist yet: {}",
            folder.display()
        )));
    }
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        command.arg(&folder);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(&folder);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(&folder);
        command
    };
    command.spawn().map_err(|error| {
        AppError::Other(format!(
            "Could not open the log folder {}: {error}",
            folder.display()
        ))
    })?;
    Ok(folder.display().to_string())
}

/// Free bytes on the volume that holds the requested path, or the model folder
/// when no path is given. Used before a large model download.
#[tauri::command]
pub fn get_free_disk_space(path: Option<String>) -> Result<u64, AppError> {
    let target = disk_space_target(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = target
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut available: u64 = 0;
        let mut total: u64 = 0;
        let mut free: u64 = 0;
        // SAFETY: `wide` is NUL-terminated and the three pointers are valid
        // writable locals for the duration of the call.
        let ok =
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
        if ok == 0 {
            return Err(AppError::Other(format!(
                "Could not read free disk space for {}",
                target.display()
            )));
        }
        Ok(available)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target;
        Err(AppError::Other(
            "Free disk space is only reported on Windows".to_string(),
        ))
    }
}

fn disk_space_target(path: Option<String>) -> PathBuf {
    let target = path
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(crate::utils::paths::get_effective_models_dir);
    // The model folder often does not exist before the first download, and
    // GetDiskFreeSpaceExW rejects a missing path. The volume is the same for
    // the nearest existing ancestor.
    target
        .ancestors()
        .find(|ancestor| ancestor.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or(target)
}

#[cfg(test)]
mod tests {
    use super::{disk_space_target, redact_line, redacted_tail, MAX_DIAGNOSTIC_LINES};
    use std::io::Write;

    #[test]
    fn redacts_credential_shaped_lines_and_keeps_the_prefix() {
        assert_eq!(redact_line("set api_key=sk-live-1234"), "set [redacted]");
        assert_eq!(redact_line("Authorization: Bearer abc.def"), "[redacted]");
        assert_eq!(
            redact_line("refresh token stored for account"),
            "refresh [redacted]"
        );
    }

    #[test]
    fn keeps_ordinary_lines_intact() {
        let line = "启动 FunASR，设备: cuda:0";
        assert_eq!(redact_line(line), line);
        assert_eq!(
            redact_line("model download complete"),
            "model download complete"
        );
    }

    #[test]
    fn disk_space_target_resolves_a_missing_directory_to_its_parent() {
        let dir = std::env::temp_dir().join(format!(
            "speak-flow-disk-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("models").join("hub");
        assert_eq!(disk_space_target(Some(missing.display().to_string())), dir);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn disk_space_target_prefers_a_non_empty_path() {
        let dir = std::env::temp_dir().join(format!(
            "speak-flow-disk-existing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(disk_space_target(Some(dir.display().to_string())), dir);

        // The default model folder may not exist on a fresh machine; both
        // spellings resolve the same nearest existing ancestor.
        let blank = disk_space_target(Some("   ".to_string()));
        let none = disk_space_target(None);
        assert_eq!(blank, none);
        assert!(blank.is_dir());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tail_reads_only_the_end_of_the_file() {
        let dir = std::env::temp_dir().join(format!(
            "speak-flow-support-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "old line").unwrap();
        for index in 0..50 {
            writeln!(file, "line {index}").unwrap();
        }
        drop(file);

        let tail = redacted_tail(&path, 64).unwrap();
        assert!(tail.contains("line 49"));
        assert!(!tail.contains("old line"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tail_is_bounded_by_line_count() {
        let dir = std::env::temp_dir().join(format!(
            "speak-flow-support-lines-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");
        let mut file = std::fs::File::create(&path).unwrap();
        for index in 0..(MAX_DIAGNOSTIC_LINES + 50) {
            writeln!(file, "line {index}").unwrap();
        }
        drop(file);

        let tail = redacted_tail(&path, 1024 * 1024).unwrap();
        assert_eq!(tail.lines().count(), MAX_DIAGNOSTIC_LINES);
        assert!(tail.contains(&format!("line {}", MAX_DIAGNOSTIC_LINES + 49)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
