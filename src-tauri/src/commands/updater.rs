use serde::{Deserialize, Serialize};

use crate::utils::AppError;

static UPDATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

fn ensure_update_idle(state: &crate::state::AppState) -> Result<(), AppError> {
    use crate::state::RecordingPhase;
    if state.recording.recording.lock().is_some()
        || state.recording.snapshot().is_some_and(|snapshot| {
            matches!(
                snapshot.phase,
                RecordingPhase::Starting | RecordingPhase::Recording | RecordingPhase::Processing
            )
        })
    {
        return Err(AppError::Other(
            "Finish recording and transcription before updating.".into(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn install_app_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    expected_version: String,
    on_progress: tauri::ipc::Channel<UpdateProgress>,
) -> Result<(), AppError> {
    use tauri_plugin_updater::UpdaterExt;
    crate::services::local_only::ensure("Install an app update").map_err(AppError::Other)?;
    let _operation = UPDATE_LOCK
        .try_lock()
        .map_err(|_| AppError::Other("An update is already in progress.".into()))?;
    ensure_update_idle(&state)?;
    let exit_handle = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || crate::stop_funasr_on_exit(&exit_handle))
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|error| AppError::Download(error.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|error| AppError::Download(error.to_string()))?
        .ok_or_else(|| AppError::Other("No newer signed update is available.".into()))?;
    if update.version != expected_version {
        return Err(AppError::Other(
            "The available update changed. Check for updates again.".into(),
        ));
    }
    let mut downloaded = 0_u64;
    let mut last_percent = None;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let percent = total
                    .filter(|total| *total > 0)
                    .map(|total| downloaded * 100 / total);
                if percent != last_percent || last_percent.is_none() {
                    let _ = on_progress.send(UpdateProgress {
                        phase: "downloading",
                        downloaded,
                        total,
                    });
                    last_percent = percent;
                }
            },
            || {},
        )
        .await
        .map_err(|error| AppError::Download(error.to_string()))?;
    // Download verifies the signature before the installer can run.
    ensure_update_idle(&state)?;
    let _ = on_progress.send(UpdateProgress {
        phase: "installing",
        downloaded: bytes.len() as u64,
        total: Some(bytes.len() as u64),
    });
    tauri::async_runtime::spawn_blocking(move || update.install(bytes))
        .await
        .map_err(|error| AppError::Other(error.to_string()))?
        .map_err(|error| AppError::Other(error.to_string()))?;
    // The Windows installer relaunches the app; other desktops restart here.
    #[cfg(not(target_os = "windows"))]
    app.restart();
    Ok(())
}

const GITHUB_RELEASE_API: &str = "https://api.github.com/repos/khentmoba/SpeakFlow/releases/latest";
const GITHUB_RELEASES_URL: &str = "https://github.com/khentmoba/SpeakFlow/releases";
const UPDATER_USER_AGENT: &str = concat!("speak-flow/", env!("CARGO_PKG_VERSION"));
const UPDATER_REQUEST_TIMEOUT_SECS: u64 = 15;
/// Allowlist of hosts that `open_app_release_page` may open. The command receives the URL
/// from the frontend; without an allowlist it would expose "open any external URL" to IPC.
const ALLOWED_RELEASE_HOSTS: &[&str] = &["github.com", "www.github.com"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub release_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
}

#[tauri::command]
pub async fn check_app_update() -> Result<AppUpdateInfo, AppError> {
    // Local-only mode must not reach GitHub. Reporting "no update" keeps the
    // periodic check quiet instead of raising an error the user did not cause.
    if crate::services::local_only::is_enabled() {
        log::debug!("Update check skipped: Local-only mode is on");
        return Ok(app_update_info(env!("CARGO_PKG_VERSION"), None));
    }
    Ok(app_update_info(
        env!("CARGO_PKG_VERSION"),
        fetch_latest_release().await?,
    ))
}

fn app_update_info(current_version: &str, release: Option<GitHubRelease>) -> AppUpdateInfo {
    let Some(release) = release else {
        return AppUpdateInfo {
            available: false,
            current_version: current_version.to_string(),
            latest_version: None,
            notes: None,
            published_at: None,
            release_url: Some(GITHUB_RELEASES_URL.to_string()),
        };
    };
    let latest_version = normalize_version(&release.tag_name);
    let available = is_version_newer(&latest_version, current_version);

    AppUpdateInfo {
        available,
        current_version: current_version.to_string(),
        latest_version: Some(latest_version),
        notes: if available {
            release.body.filter(|body| !body.trim().is_empty())
        } else {
            None
        },
        published_at: release.published_at,
        release_url: Some(release.html_url),
    }
}

#[tauri::command]
pub async fn open_app_release_page(url: Option<String>) -> Result<String, AppError> {
    let target = url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| validate_release_url(&value).map(|_| value))
        .transpose()?
        .unwrap_or_else(|| GITHUB_RELEASES_URL.to_string());
    open_external_url(&target)?;
    Ok("Opened GitHub Releases page".to_string())
}

/// Only https + GitHub hosts pass. The frontend should only pass the `release_url`
/// returned by `check_app_update`, but since the command is exposed over IPC, the backend pins that assumption.
fn validate_release_url(value: &str) -> Result<(), AppError> {
    let parsed = reqwest::Url::parse(value)
        .map_err(|err| AppError::Other(format!("Invalid release URL: {}", err)))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Other(format!(
            "Release URL only supports https, got scheme: {}",
            parsed.scheme()
        )));
    }
    let host = parsed.host_str().unwrap_or("");
    if !ALLOWED_RELEASE_HOSTS.contains(&host) {
        return Err(AppError::Other(format!(
            "Release URL host is not in the allowlist: {}",
            host
        )));
    }
    Ok(())
}

async fn fetch_latest_release() -> Result<Option<GitHubRelease>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(UPDATER_REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|err| AppError::Download(format!("Failed to build HTTP client: {}", err)))?;
    let response = client
        .get(GITHUB_RELEASE_API)
        .header(reqwest::header::USER_AGENT, UPDATER_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|err| AppError::Download(format!("Failed to request GitHub release: {}", err)))?;

    // GitHub returns 404 when this repository has no published releases.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "GitHub release check failed: HTTP {}",
            response.status()
        )));
    }

    response
        .json::<GitHubRelease>()
        .await
        .map(Some)
        .map_err(|err| {
            AppError::Download(format!("Failed to parse GitHub release response: {}", err))
        })
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn parse_version(version: &str) -> Vec<u64> {
    normalize_version(version)
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

fn is_version_newer(latest: &str, current: &str) -> bool {
    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);
    let max_len = latest_parts.len().max(current_parts.len());

    for index in 0..max_len {
        let latest_part = latest_parts.get(index).copied().unwrap_or(0);
        let current_part = current_parts.get(index).copied().unwrap_or(0);
        if latest_part > current_part {
            return true;
        }
        if latest_part < current_part {
            return false;
        }
    }

    false
}

#[cfg(target_os = "windows")]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("rundll32")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("Failed to open download page: {}", err)))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("Failed to open download page: {}", err)))?;
    Ok(())
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn open_external_url(url: &str) -> Result<(), AppError> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map_err(|err| AppError::Other(format!("Failed to open download page: {}", err)))?;
    Ok(())
}

#[cfg(test)]
mod validation_tests {
    use super::{
        app_update_info, is_version_newer, validate_release_url, GitHubRelease, GITHUB_RELEASES_URL,
    };

    #[test]
    fn updates_reject_in_flight_recording_and_transcription() {
        use crate::state::{AppState, RecordingMode, RecordingPhase};
        let state = AppState::default();
        assert!(super::ensure_update_idle(&state).is_ok());
        for phase in [
            RecordingPhase::Starting,
            RecordingPhase::Recording,
            RecordingPhase::Processing,
        ] {
            state.recording.transition_snapshot_if_current(
                0,
                phase,
                RecordingMode::Dictation,
                None,
                None,
            );
            assert!(super::ensure_update_idle(&state).is_err());
        }
        state.recording.transition_snapshot_if_current(
            0,
            RecordingPhase::Idle,
            RecordingMode::Dictation,
            None,
            None,
        );
        assert!(super::ensure_update_idle(&state).is_ok());
    }

    #[tokio::test]
    async fn update_lock_rejects_a_duplicate_and_recovers_after_release() {
        let guard = super::UPDATE_LOCK.try_lock().unwrap();
        assert!(super::UPDATE_LOCK.try_lock().is_err());
        drop(guard);
        assert!(super::UPDATE_LOCK.try_lock().is_ok());
    }

    #[test]
    fn no_published_release_is_not_an_update_error() {
        let info = app_update_info("1.0.0", None);
        assert!(!info.available);
        assert_eq!(info.current_version, "1.0.0");
        assert!(info.latest_version.is_none());
        assert_eq!(info.release_url.as_deref(), Some(GITHUB_RELEASES_URL));
    }

    #[test]
    fn checks_this_projects_published_release_against_the_installed_version() {
        let info = app_update_info(
            "1.0.0",
            Some(GitHubRelease {
                tag_name: "v1.0.1".into(),
                body: Some("SpeakFlow update".into()),
                published_at: None,
                html_url: format!("{GITHUB_RELEASES_URL}/tag/v1.0.1"),
            }),
        );
        assert!(info.available);
        assert_eq!(info.latest_version.as_deref(), Some("1.0.1"));
        assert_eq!(
            info.release_url.as_deref(),
            Some("https://github.com/khentmoba/SpeakFlow/releases/tag/v1.0.1")
        );
    }

    #[test]
    fn release_urls_require_exact_github_host_and_https() {
        for url in [
            "https://github.com/a/b/releases",
            "https://www.github.com/a/b",
        ] {
            assert!(validate_release_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://github.com/a",
            "https://github.com.evil.example/a",
            "https://github.com@evil.example/a",
            "file:///tmp/a",
            "javascript:alert(1)",
            "not a url",
        ] {
            assert!(validate_release_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn numeric_version_order_is_lexicographic_with_zero_padding() {
        for (latest, current, expected) in [
            ("v1.6.3", "1.6.2", true),
            ("1.10", "1.9.9", true),
            ("1.6", "1.6.0", false),
            ("1.6.2", "1.6.2", false),
            ("1.5.99", "1.6.0", false),
            ("1.6.2-rc1", "1.6.1", true),
        ] {
            assert_eq!(is_version_newer(latest, current), expected);
        }
    }
}
