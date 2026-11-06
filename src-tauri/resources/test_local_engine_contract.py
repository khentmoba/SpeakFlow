"""Keeps engine keys and model files in step across Rust, TypeScript and Python.

`src-tauri/resources/asr_catalog.json` is the single source of truth, generated
by `scripts/sync_asr_catalog.py`. These tests fail if a consumer drifts from it
or if the catalog itself is malformed.
"""

import json
import re
import sys
import unittest
from pathlib import Path

RESOURCES_DIR = Path(__file__).resolve().parent
REPO_ROOT = RESOURCES_DIR.parent.parent
sys.path.insert(0, str(RESOURCES_DIR))

from hf_cache_utils import ASR_CATALOG, LOCAL_ASR_MODELS, TRANSCRIBE_CPP_ASR_MODELS  # noqa: E402

CATALOG_JSON = RESOURCES_DIR / "asr_catalog.json"
BENCHMARKS_JSON = RESOURCES_DIR / "asr_benchmarks.json"
ASR_BENCHMARKS = json.loads(BENCHMARKS_JSON.read_text(encoding="utf-8"))
PATHS_RS = REPO_ROOT / "src-tauri" / "src" / "utils" / "paths.rs"
CATALOG_RS = REPO_ROOT / "src-tauri" / "src" / "utils" / "asr_catalog.rs"
FUNASR_RS = REPO_ROOT / "src-tauri" / "src" / "services" / "funasr_service.rs"
ENGINE_PY = RESOURCES_DIR / "engine.py"
TAURI_CONF = REPO_ROOT / "src-tauri" / "tauri.conf.json"
BUILD_ENGINE = REPO_ROOT / "scripts" / "build_engine.py"
ENGINE_OPTIONS_TS = REPO_ROOT / "src" / "lib" / "asrEngineOptions.ts"
ENGINE_CAPABILITIES_TS = REPO_ROOT / "src" / "lib" / "asrEngineCapabilities.ts"
ONLINE_ENGINES = {"glm-asr", "alibaba-asr"}
R2T2_ENGINE = "confucius4-r2t2"
REQUIRED_FIELDS = {
    "key",
    "name",
    "description",
    "repo_id",
    "filename",
    "revision",
    "size",
    "sha256",
    "license",
    "recommended",
    "languages",
    "vocabulary",
}


class AsrCatalogTests(unittest.TestCase):
    def test_catalog_is_well_formed(self):
        self.assertEqual(ASR_CATALOG["engine_version"], "0.4.1")
        self.assertTrue(ASR_CATALOG["source_commit"])
        keys = [model["key"] for model in ASR_CATALOG["models"]]
        self.assertEqual(len(keys), len(set(keys)), "duplicate engine keys")
        for model in ASR_CATALOG["models"]:
            missing = REQUIRED_FIELDS - set(model)
            self.assertFalse(missing, f"{model.get('key')} is missing {sorted(missing)}")
            self.assertTrue(model["filename"].endswith(".gguf"), model["key"])
            self.assertRegex(model["sha256"], r"^[0-9a-f]{64}$")
            self.assertGreater(model["size"], 1_000_000)
            self.assertNotEqual(model["license"], "other", f"{model['key']} needs a named license")
            self.assertTrue(model["languages"], model["key"])

    def test_benchmarks_cover_every_model_and_only_listed_languages(self):
        self.assertEqual(
            sorted(ASR_BENCHMARKS["models"]),
            sorted(model["key"] for model in ASR_CATALOG["models"]),
        )
        for model in ASR_CATALOG["models"]:
            benchmark = ASR_BENCHMARKS["models"][model["key"]]
            self.assertIn("speed", benchmark)
            self.assertTrue(benchmark["accuracy"], model["key"])
            # Whisper lists languages it supports but has no benchmark for, so only
            # the reverse direction holds: every score belongs to a listed language.
            for language in benchmark["accuracy"]:
                self.assertIn(language, model["languages"], f"{model['key']} {language}")
            for grade, error in benchmark["accuracy"].values():
                self.assertRegex(grade, r"^[A-F][+-]?$")
                self.assertGreaterEqual(error, 0)

    def test_shipped_engines_keep_their_identity(self):
        shipped = {
            "qwen3-asr-0.6b": ("handy-computer/Qwen3-ASR-0.6B-gguf", "Qwen3-ASR-0.6B-Q8_0.gguf"),
            "qwen3-asr-1.7b": ("handy-computer/Qwen3-ASR-1.7B-gguf", "Qwen3-ASR-1.7B-Q8_0.gguf"),
            "parakeet-unified-en-0.6b": (
                "handy-computer/parakeet-unified-en-0.6b-gguf",
                "parakeet-unified-en-0.6b-Q8_0.gguf",
            ),
            "parakeet-tdt-0.6b-v2": (
                "handy-computer/parakeet-tdt-0.6b-v2-gguf",
                "parakeet-tdt-0.6b-v2-Q8_0.gguf",
            ),
            "canary-qwen-2.5b": ("handy-computer/canary-qwen-2.5b-gguf", "canary-qwen-2.5b-Q8_0.gguf"),
            "cohere-transcribe-03-2026": (
                "handy-computer/cohere-transcribe-03-2026-gguf",
                "cohere-transcribe-03-2026-Q8_0.gguf",
            ),
        }
        for engine, expected in shipped.items():
            model = TRANSCRIBE_CPP_ASR_MODELS[engine]
            self.assertEqual((model["repo_id"], model["filename"]), expected)
        self.assertEqual(
            {engine for engine, model in TRANSCRIBE_CPP_ASR_MODELS.items() if model["vocabulary"]},
            {"qwen3-asr-0.6b", "qwen3-asr-1.7b"},
        )

    def test_python_registry_is_the_catalog(self):
        self.assertEqual(sorted(LOCAL_ASR_MODELS), sorted([*TRANSCRIBE_CPP_ASR_MODELS, R2T2_ENGINE]))
        for engine, model in TRANSCRIBE_CPP_ASR_MODELS.items():
            self.assertEqual(model, next(m for m in ASR_CATALOG["models"] if m["key"] == engine))

    def test_rust_reads_the_same_catalog_file(self):
        source = CATALOG_RS.read_text(encoding="utf-8")
        self.assertIn('include_str!("../../resources/asr_catalog.json")', source)
        paths = PATHS_RS.read_text(encoding="utf-8")
        self.assertNotIn("LOCAL_ASR_ENGINES", paths)
        self.assertIn("super::asr_catalog::catalog_engines()", paths)

    def test_rust_readiness_keeps_only_the_r2t2_pin(self):
        source = FUNASR_RS.read_text(encoding="utf-8")
        self.assertIn('("davidxifeng/Confucius4-R2T2-gguf", "r2t2-q8_0.gguf")', source)
        self.assertIn("crate::utils::asr_catalog::find(engine)", source)

    def test_catalog_ships_with_the_app_and_the_engine(self):
        resources = json.loads(TAURI_CONF.read_text(encoding="utf-8"))["bundle"]["resources"]
        self.assertEqual(resources["resources/asr_catalog.json"], "resources/asr_catalog.json")
        self.assertIn('"asr_catalog.json"', BUILD_ENGINE.read_text(encoding="utf-8"))
        self.assertTrue(CATALOG_JSON.is_file())
        self.assertTrue(BENCHMARKS_JSON.is_file())
        # Benchmarks stay out of the startup bundle: only the browser imports them.
        self.assertNotIn("asr_benchmarks", (
            REPO_ROOT / "src" / "lib" / "asrCatalog.ts"
        ).read_text(encoding="utf-8"))

    def test_bundled_engine_entry_derives_its_choices(self):
        source = ENGINE_PY.read_text(encoding="utf-8")
        self.assertIsNone(re.search(r"engine_choices = \[", source))
        self.assertIn("engine_choices = sorted(LOCAL_ASR_MODELS)", source)

    def test_display_names_cover_every_transcribe_cpp_engine(self):
        import qwen3_asr_server

        self.assertEqual(
            set(qwen3_asr_server.ENGINE_DISPLAY_NAMES), set(TRANSCRIBE_CPP_ASR_MODELS)
        )
        self.assertEqual(
            qwen3_asr_server.ENGINE_DISPLAY_NAMES["parakeet-unified-en-0.6b"],
            "Parakeet Unified EN 0.6B",
        )

    def test_frontend_engine_options_cover_every_engine(self):
        source = ENGINE_OPTIONS_TS.read_text(encoding="utf-8")
        self.assertIn("CATALOG_MODELS.map((model)", source)
        self.assertIn("...catalogOptions, ...NON_CATALOG_OPTIONS", source)
        block = source[source.index("const NON_CATALOG_OPTIONS"):source.index("/** Every selectable engine")]
        self.assertEqual(
            sorted(re.findall(r'key: "([^"]+)"', block)),
            sorted([R2T2_ENGINE, *ONLINE_ENGINES]),
        )

    def test_frontend_capabilities_match_the_catalog(self):
        source = ENGINE_CAPABILITIES_TS.read_text(encoding="utf-8")
        self.assertIn("...Object.fromEntries(", source)
        self.assertIn("CATALOG_MODELS.map((model)", source)
        self.assertIn("downloadSize: formatModelSize(model.size)", source)
        self.assertIn("vocabulary: model.vocabulary", source)
        block = source[source.index("NON_CATALOG_CAPABILITIES"):source.index("export const ASR_ENGINE_CAPABILITIES")]
        entries = re.findall(
            r'"([^"]+)": \{ execution: "(local|cloud)", final: true, interim: \w+, downloadSize: (?:null|"[^"]+"), vocabulary: (true|false) \}',
            block,
        )
        self.assertEqual(
            sorted(entries),
            [
                ("alibaba-asr", "cloud", "true"),
                ("confucius4-r2t2", "local", "true"),
                ("glm-asr", "cloud", "true"),
            ],
        )


if __name__ == "__main__":
    unittest.main()
