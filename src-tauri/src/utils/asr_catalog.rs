//! Local speech models SpeakFlow can download, read from the same
//! `resources/asr_catalog.json` that the Python engine and the frontend use.
//! Regenerate it with `scripts/sync_asr_catalog.py`; never edit it by hand.

use std::sync::OnceLock;

use serde::Deserialize;

const CATALOG_JSON: &str = include_str!("../../resources/asr_catalog.json");

#[derive(Debug, Deserialize)]
pub struct CatalogModel {
    pub key: String,
    pub name: String,
    pub repo_id: String,
    pub filename: String,
    pub size: u64,
    /// Offered as a default pick in the model browser. Read by the UI from the
    /// same JSON; kept here so the contract tests can assert Whisper is never
    /// recommended.
    #[allow(dead_code)]
    pub recommended: bool,
    /// Conditions recognition on Smart Vocabulary. Other models only get
    /// spelling correction after recognition.
    pub vocabulary: bool,
    /// The model can write English text from speech in another language.
    pub translation: bool,
    /// The model can detect the spoken language itself.
    pub language_detection: bool,
    /// Whisper: broad coverage, mostly experimental accuracy. Read by the UI
    /// from the same JSON; kept here for the long-tail contract test.
    #[allow(dead_code)]
    pub long_tail: bool,
    /// Language codes the model can produce.
    pub languages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    models: Vec<CatalogModel>,
}

fn catalog() -> &'static [CatalogModel] {
    static CATALOG: OnceLock<Vec<CatalogModel>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            serde_json::from_str::<Catalog>(CATALOG_JSON)
                .expect("resources/asr_catalog.json is valid")
                .models
        })
        .as_slice()
}

pub fn models() -> &'static [CatalogModel] {
    catalog()
}

pub fn find(engine: &str) -> Option<&'static CatalogModel> {
    catalog().iter().find(|model| model.key == engine)
}

pub fn is_catalog_engine(engine: &str) -> bool {
    find(engine).is_some()
}

pub fn catalog_engines() -> impl Iterator<Item = &'static str> {
    catalog().iter().map(|model| model.key.as_str())
}

pub fn receives_vocabulary(engine: &str) -> bool {
    find(engine).is_some_and(|model| model.vocabulary)
}

pub fn supports_translation(engine: &str) -> bool {
    find(engine).is_some_and(|model| model.translation)
}

/// The language the model can actually produce for a requested language.
///
/// A single-language model always uses that language. A requested language the
/// model lists wins over detection. Otherwise detection is used when the model
/// supports it, and English is the last resort before the model's first
/// language. `"auto"` means "let the model detect it".
pub fn resolve_language(engine: &str, requested: Option<&str>) -> String {
    let Some(model) = find(engine) else {
        return "auto".to_string();
    };
    if model.languages.len() == 1 {
        if let Some(only) = model.languages.first() {
            return only.clone();
        }
    }
    let requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "auto");
    if let Some(value) = requested {
        if model.languages.iter().any(|language| language == value) {
            return value.to_string();
        }
    }
    if model.language_detection {
        return "auto".to_string();
    }
    model
        .languages
        .iter()
        .find(|language| *language == "en")
        .or_else(|| model.languages.first())
        .cloned()
        .unwrap_or_else(|| "auto".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_keys_are_unique_and_shipped_engines_remain() {
        let mut keys: Vec<&str> = catalog_engines().collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), total, "duplicate engine keys");
        for engine in [
            "qwen3-asr-0.6b",
            "qwen3-asr-1.7b",
            "parakeet-unified-en-0.6b",
            "parakeet-tdt-0.6b-v2",
            "canary-qwen-2.5b",
            "cohere-transcribe-03-2026",
        ] {
            assert!(is_catalog_engine(engine), "{engine} left the catalog");
        }
        assert!(!is_catalog_engine("confucius4-r2t2"));
        assert!(!is_catalog_engine("glm-asr"));
    }

    #[test]
    fn every_model_names_an_exact_gguf_file() {
        for model in catalog() {
            assert!(model.filename.ends_with(".gguf"), "{}", model.key);
            assert!(model.repo_id.contains('/'), "{}", model.key);
            assert!(!model.name.is_empty(), "{}", model.key);
        }
    }

    #[test]
    fn only_qwen3_conditions_recognition_on_vocabulary() {
        let mut engines: Vec<&str> = catalog_engines()
            .filter(|engine| receives_vocabulary(engine))
            .collect();
        engines.sort_unstable();
        assert_eq!(engines, ["qwen3-asr-0.6b", "qwen3-asr-1.7b"]);
    }

    #[test]
    fn whisper_returns_as_the_labelled_long_tail_family() {
        let whisper: Vec<&CatalogModel> = catalog()
            .iter()
            .filter(|model| model.key.starts_with("whisper"))
            .collect();
        assert_eq!(whisper.len(), 12);
        // Only the multilingual variants add coverage; the four English-only
        // variants are Whisper models but not long-tail.
        let long_tail: Vec<&CatalogModel> = whisper
            .iter()
            .copied()
            .filter(|model| model.long_tail)
            .collect();
        assert_eq!(long_tail.len(), 8);
        assert!(long_tail.iter().all(|model| model.languages.len() > 1));
        assert!(whisper.iter().all(|model| !model.recommended));
        assert!(whisper.iter().any(|model| model.key == "whisper-large-v3"));
        assert!(!catalog()
            .iter()
            .any(|model| model.key.contains("sensevoice")));
    }

    #[test]
    fn resolve_language_prefers_the_model_it_can_actually_run() {
        // Single-language models ignore a different request.
        assert_eq!(
            resolve_language("parakeet-unified-en-0.6b", Some("fr")),
            "en"
        );
        // A listed language wins.
        assert_eq!(resolve_language("qwen3-asr-0.6b", Some("fr")), "fr");
        // An unlisted language falls back to detection when the model has it.
        assert_eq!(
            resolve_language("nemotron-3.5-asr-streaming-0.6b", Some("ga")),
            "auto"
        );
        // No detection and no English leaves the model's first language.
        assert_eq!(resolve_language("canary-180m-flash", Some("ja")), "en");
        // No request and no detection uses English.
        assert_eq!(resolve_language("canary-180m-flash", None), "en");
        // Unknown engines are left to the server.
        assert_eq!(resolve_language("not-a-model", Some("fr")), "auto");
    }
}
