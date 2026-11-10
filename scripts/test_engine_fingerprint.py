"""The engine fingerprint ignores the app version and tracks packaged inputs."""

import subprocess
import tempfile
import unittest
from pathlib import Path

import engine_fingerprint


class FingerprintTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        (self.root / "src-tauri/resources").mkdir(parents=True)
        self.write_versions("1.0.0")
        self.write("src-tauri/resources/engine.py", "print('engine')\n")
        self.write("src-tauri/resources/test_engine.py", "pass\n")
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)

    def tearDown(self):
        self.directory.cleanup()

    def write(self, path, text):
        (self.root / path).write_text(text, encoding="utf-8")

    def write_versions(self, version):
        self.write("pyproject.toml", f'[project]\nname = "speak-flow"\nversion = "{version}"\n')
        self.write("uv.lock", f'[[package]]\nname = "speak-flow"\nversion = "{version}"\n')

    def test_version_bump_keeps_the_fingerprint(self):
        before = engine_fingerprint.fingerprint(self.root)
        self.write_versions("2.0.0")
        self.assertEqual(engine_fingerprint.fingerprint(self.root), before)

    def test_engine_source_change_changes_the_fingerprint(self):
        before = engine_fingerprint.fingerprint(self.root)
        self.write("src-tauri/resources/engine.py", "print('changed')\n")
        self.assertNotEqual(engine_fingerprint.fingerprint(self.root), before)

    def test_dependency_change_changes_the_fingerprint(self):
        before = engine_fingerprint.fingerprint(self.root)
        self.write("uv.lock", '[[package]]\nname = "speak-flow"\nversion = "1.0.0"\n\n[[package]]\nname = "numpy"\n')
        self.assertNotEqual(engine_fingerprint.fingerprint(self.root), before)

    def test_tests_are_not_engine_inputs(self):
        before = engine_fingerprint.fingerprint(self.root)
        self.write("src-tauri/resources/test_engine.py", "assert True\n")
        self.assertEqual(engine_fingerprint.fingerprint(self.root), before)


if __name__ == "__main__":
    unittest.main()
