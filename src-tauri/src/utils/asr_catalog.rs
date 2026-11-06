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
    /// Conditions recognition on Smart Vocabulary. Other models only get
    /// spelling correction after recognition.
    pub vocabulary: bool,
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
}
