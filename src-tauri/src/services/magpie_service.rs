//! Detection for Magpie, the local model gateway.
//!
//! Magpie serves every subscription it holds behind one OpenAI-compatible
//! endpoint, so SpeakFlow can use it as a provider without knowing which
//! vendor answers. The provider is only offered when Magpie is on this machine.

use std::path::Path;
use std::time::Duration;

pub const MAGPIE_PROVIDER: &str = "magpie";
/// Magpie's local gateway. `magpie serve`, the desktop app and `magpie web`
/// all answer here; the port is fixed by Magpie itself.
pub const MAGPIE_GATEWAY_BASE_URL: &str = "http://127.0.0.1:3425/v1";
/// Magpie accepts any bearer locally. A literal keeps the key out of the
/// keyring, where it would look like a credential the user has to manage.
pub const MAGPIE_GATEWAY_KEY: &str = "magpie";

const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagpieStatus {
    /// Offer the provider at all: installed, or already answering.
    pub available: bool,
    /// The gateway answers right now, so requests will go through.
    pub running: bool,
    pub gateway_url: String,
    /// Which signal found it, for the settings hint.
    pub source: Option<String>,
}

impl Default for MagpieStatus {
    fn default() -> Self {
        Self {
            available: false,
            running: false,
            gateway_url: MAGPIE_GATEWAY_BASE_URL.to_string(),
            source: None,
        }
    }
}

pub fn is_magpie_provider(provider: &str) -> bool {
    provider == MAGPIE_PROVIDER
}

/// Where Magpie installs itself, in the order worth checking.
fn install_markers() -> Vec<std::path::PathBuf> {
    let mut markers = Vec::new();
    if let Some(home) = dirs::home_dir() {
        // The config directory exists from the first run, even when the
        // binary lives somewhere the app's PATH does not include.
        markers.push(home.join(".config").join("magpie"));
        markers.push(home.join(".local").join("bin").join("magpie"));
    }
    if let Some(local) = dirs::data_local_dir() {
        markers.push(local.join("Programs").join("magpie").join("magpie.exe"));
    }
    for path in [
        "/usr/local/bin/magpie",
        "/opt/homebrew/bin/magpie",
        "/usr/bin/magpie",
    ] {
        markers.push(std::path::PathBuf::from(path));
    }
    markers
}

fn installed_at() -> Option<String> {
    install_markers()
        .into_iter()
        .find(|path| Path::new(path).exists())
        .map(|path| path.display().to_string())
}

/// Probe the gateway with a short timeout. `GET /v1/models` is the cheapest
/// request that proves the whole path works.
async fn gateway_answers(client: &reqwest::Client) -> bool {
    match client
        .get(format!("{MAGPIE_GATEWAY_BASE_URL}/models"))
        .timeout(PROBE_TIMEOUT)
        .header("Authorization", format!("Bearer {MAGPIE_GATEWAY_KEY}"))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

pub async fn detect(client: &reqwest::Client) -> MagpieStatus {
    let installed = installed_at();
    let running = gateway_answers(client).await;
    let source = if running {
        Some("gateway".to_string())
    } else {
        installed
    };
    MagpieStatus {
        available: running || source.is_some(),
        running,
        gateway_url: MAGPIE_GATEWAY_BASE_URL.to_string(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_url_is_the_openai_compatible_root() {
        assert_eq!(MAGPIE_GATEWAY_BASE_URL, "http://127.0.0.1:3425/v1");
        assert!(MAGPIE_GATEWAY_BASE_URL.ends_with("/v1"));
    }

    #[test]
    fn only_the_magpie_provider_matches() {
        assert!(is_magpie_provider("magpie"));
        assert!(!is_magpie_provider("openai"));
        assert!(!is_magpie_provider(""));
    }

    #[test]
    fn default_status_hides_the_provider() {
        let status = MagpieStatus::default();
        assert!(!status.available);
        assert!(!status.running);
        assert_eq!(status.gateway_url, MAGPIE_GATEWAY_BASE_URL);
    }
}
