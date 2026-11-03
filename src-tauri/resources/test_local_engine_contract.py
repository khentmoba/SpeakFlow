"""Keeps engine keys and model files in step across Rust, TypeScript and Python."""

import re
import sys
import unittest
from pathlib import Path

RESOURCES_DIR = Path(__file__).resolve().parent
REPO_ROOT = RESOURCES_DIR.parent.parent
sys.path.insert(0, str(RESOURCES_DIR))

from hf_cache_utils import LOCAL_ASR_MODELS, TRANSCRIBE_CPP_ASR_MODELS  # noqa: E402

PATHS_RS = REPO_ROOT / "src-tauri" / "src" / "utils" / "paths.rs"
FUNASR_RS = REPO_ROOT / "src-tauri" / "src" / "services" / "funasr_service.rs"
ENGINE_PY = RESOURCES_DIR / "engine.py"
ENGINE_OPTIONS_TS = REPO_ROOT / "src" / "lib" / "asrEngineOptions.ts"
ENGINE_CAPABILITIES_TS = REPO_ROOT / "src" / "lib" / "asrEngineCapabilities.ts"
ONLINE_ENGINES = {"glm-asr", "alibaba-asr"}


def _quoted(block):
    return re.findall(r'"([^"]+)"', block)


def _rust_const_list(source, name):
    match = re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", source, re.S)
    if match is None:
        raise AssertionError(f"{name} not found in paths.rs")
    return _quoted(match.group(1))


class LocalEngineContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.paths_source = PATHS_RS.read_text(encoding="utf-8")
        cls.rust_local = _rust_const_list(cls.paths_source, "LOCAL_ASR_ENGINES")
        cls.rust_online = _rust_const_list(cls.paths_source, "ONLINE_ASR_ENGINES")

    def test_rust_local_engines_match_downloadable_models(self):
        self.assertEqual(sorted(self.rust_local), sorted(LOCAL_ASR_MODELS))

    def test_rust_online_engines_are_the_cloud_options(self):
        self.assertEqual(set(self.rust_online), ONLINE_ENGINES)

    def test_transcribe_cpp_family_matches_python_registry(self):
        match = re.search(
            r"pub fn is_transcribe_cpp_engine\(engine: &str\) -> bool \{\s*matches!\(\s*engine,\s*(.*?)\s*\)\s*\}",
            self.paths_source,
            re.S,
        )
        self.assertIsNotNone(match)
        self.assertEqual(set(_quoted(match.group(1))), set(TRANSCRIBE_CPP_ASR_MODELS))

    def test_rust_readiness_table_points_at_the_pinned_python_files(self):
        entries = re.findall(
            r'engine: "([^"]+)",\s*repo_id: "([^"]+)",\s*filename: "([^"]+)"',
            FUNASR_RS.read_text(encoding="utf-8"),
        )
        rust_files = {engine: (repo, filename) for engine, repo, filename in entries}
        python_files = {
            engine: (model["repo_id"], model["filename"])
            for engine, model in LOCAL_ASR_MODELS.items()
        }
        self.assertEqual(rust_files, python_files)

    def test_frontend_engine_options_cover_every_engine(self):
        keys = re.findall(r'key: "([^"]+)"', ENGINE_OPTIONS_TS.read_text(encoding="utf-8"))
        self.assertEqual(sorted(keys), sorted([*self.rust_local, *self.rust_online]))

    def test_frontend_capabilities_report_download_size_for_local_engines(self):
        source = ENGINE_CAPABILITIES_TS.read_text(encoding="utf-8")
        local = dict(re.findall(r'"([^"]+)": \{ execution: "local", final: true, interim: \w+, downloadSize: "([^"]+)"', source))
        cloud = set(re.findall(r'"([^"]+)": \{ execution: "cloud"', source))
        self.assertEqual(set(local), set(LOCAL_ASR_MODELS))
        self.assertEqual(cloud, ONLINE_ENGINES)
        for engine, size in local.items():
            self.assertRegex(size, r"^\d+(\.\d+)? (MB|GB)$", engine)

    def test_frontend_vocabulary_flags_match_rust(self):
        match = re.search(
            r"pub fn engine_receives_vocabulary\(engine: &str\) -> bool \{\s*matches!\(\s*engine,\s*(.*?)\s*\)\s*\}",
            self.paths_source,
            re.S,
        )
        self.assertIsNotNone(match)
        flags = dict(
            re.findall(
                r'"([^"]+)": \{ execution: .*?, vocabulary: (true|false) \}',
                ENGINE_CAPABILITIES_TS.read_text(encoding="utf-8"),
            )
        )
        self.assertEqual(set(flags), {*self.rust_local, *self.rust_online})
        self.assertEqual(
            {engine for engine, flag in flags.items() if flag == "true"},
            set(_quoted(match.group(1))),
        )

    def test_bundled_engine_entry_accepts_every_local_engine(self):
        match = re.search(r"engine_choices = \[(.*?)\]", ENGINE_PY.read_text(encoding="utf-8"), re.S)
        self.assertIsNotNone(match)
        self.assertEqual(set(_quoted(match.group(1))), set(LOCAL_ASR_MODELS))

    def test_display_names_cover_every_transcribe_cpp_engine(self):
        import qwen3_asr_server

        self.assertEqual(
            set(qwen3_asr_server.ENGINE_DISPLAY_NAMES), set(TRANSCRIBE_CPP_ASR_MODELS)
        )


if __name__ == "__main__":
    unittest.main()
