"""Exercise the real release CI gate with a local GitHub CLI mock."""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CANDIDATE = "a" * 40


def bash_executable() -> str:
    bash = (Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
            if os.name == "nt" else shutil.which("bash"))
    assert bash and Path(bash).is_file(), "Git Bash (Windows) or Bash is required"
    return str(bash)


def shell_function(name: str) -> str:
    source = (ROOT / "scripts/release.sh").read_text(encoding="utf-8")
    match = re.search(rf"^{name}\(\) \{{\n.*?^\}}", source, re.M | re.S)
    assert match is not None, f"{name}() is missing from scripts/release.sh"
    return match.group(0)


class ReleaseCiTests(unittest.TestCase):
    def run_gate(self, formal: str, head: str = CANDIDATE) -> subprocess.CompletedProcess:
        function = shell_function("wait_for_ci")
        bash = bash_executable()
        # Load only this read-only function. The release script's main/tag/upload
        # paths are never sourced or invoked; every gh call is handled locally.
        harness = r'''
set -euo pipefail
CI_WORKFLOW=ci.yml
CI_DISCOVERY_TIMEOUT_SECONDS=5
fail() { printf '%s\n' "$*" >&2; exit 1; }
gh() {
    case "$*" in
        'run list '*) printf '123\n' ;;
        'run watch '*) return 0 ;;
        *'--json headSha '*) printf '%s\n' "$TEST_RUN_HEAD" ;;
        *'--json conclusion '*) printf 'success\n' ;;
        *'--json jobs '*'Application formal contracts'*)
            if [[ "$TEST_FORMAL_RESULT" != missing ]]; then printf '%s\n' "$TEST_FORMAL_RESULT"; fi ;;
        *'--json jobs '*) printf 'success\n' ;;
        *) printf 'Unexpected gh invocation: %s\n' "$*" >&2; return 99 ;;
    esac
}
'''
        environment = dict(os.environ, TEST_FORMAL_RESULT=formal, TEST_RUN_HEAD=head)
        return subprocess.run([str(bash), "-c", harness + function + '\nwait_for_ci "$1"',
                               "release-ci-test", CANDIDATE], env=environment,
                              text=True, encoding="utf-8", capture_output=True, timeout=15)

    def test_rust_cache_cannot_restore_paths_from_a_renamed_repository(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        cache = workflow.split("      - name: Cache Rust dependencies and build outputs", 1)[1]
        cache = cache.split("      - name:", 1)[0]
        keys = [line.strip() for line in cache.splitlines()
                if line.strip().startswith(("key: rust-", "rust-"))]
        self.assertEqual(len(keys), 2)
        for key in keys:
            self.assertIn("rust-${{ github.repository }}-", key)

    def test_all_four_jobs_pass(self):
        result = self.run_gate("success")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_formal_job_blocks_release(self):
        result = self.run_gate("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_skipped_formal_job_blocks_release(self):
        result = self.run_gate("skipped")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_failed_formal_job_blocks_release(self):
        result = self.run_gate("failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_different_commit_blocks_release(self):
        result = self.run_gate("success", "b" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("headSha", result.stderr)

    def run_r2t2_gate(self, mode, *, archive, fingerprint_status, check_status):
        function = shell_function("engine_will_rebuild") + "\n" + shell_function(
            "require_r2t2_runtime_when_engine_builds")
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "calls.log"
            archive_path = Path(directory) / "engine.tar.xz"
            if archive:
                archive_path.write_bytes(b"archive")
            # python is stubbed so the gate's branching is tested without building anything.
            harness = r'''
set -euo pipefail
ENGINE_MODE="$1"
ENGINE_ARCHIVE="$2"
fail() { printf '%s\n' "$*" >&2; exit 1; }
python() {
    printf '%s\n' "$*" >>"$TEST_CALL_LOG"
    case "$*" in
        *engine_fingerprint.py*) return "$TEST_FINGERPRINT_STATUS" ;;
        *build_engine.py*) return "$TEST_R2T2_STATUS" ;;
    esac
}
'''
            environment = dict(os.environ, TEST_CALL_LOG=str(log),
                               TEST_FINGERPRINT_STATUS=str(fingerprint_status),
                               TEST_R2T2_STATUS=str(check_status))
            result = subprocess.run(
                [bash_executable(), "-c", harness + function + "\nrequire_r2t2_runtime_when_engine_builds",
                 "release-r2t2-test", mode, archive_path.as_posix()],
                env=environment, text=True, encoding="utf-8", capture_output=True, timeout=15)
            calls = log.read_text(encoding="utf-8").splitlines() if log.exists() else []
        return result, calls

    def test_r2t2_gate_is_wired_into_preflight_before_any_push(self):
        self.assertIn("require_r2t2_runtime_when_engine_builds", shell_function("preflight"))
        main = shell_function("main")
        self.assertLess(main.index("    preflight\n"), main.index("    commit_candidate\n"))

    def test_engine_build_and_gate_share_one_rebuild_decision(self):
        self.assertIn("engine_will_rebuild", shell_function("prepare_engine"))
        self.assertIn("engine_will_rebuild", shell_function("require_r2t2_runtime_when_engine_builds"))

    def test_reuse_engine_needs_no_r2t2_runtime(self):
        result, calls = self.run_r2t2_gate("--reuse-engine", archive=True, fingerprint_status=1, check_status=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls, [])

    def test_rebuild_is_blocked_when_r2t2_runtime_is_missing(self):
        result, calls = self.run_r2t2_gate("--rebuild-engine", archive=True, fingerprint_status=0, check_status=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(calls and calls[-1].endswith("build_engine.py --check-r2t2"), calls)
        self.assertIn("R2T2", result.stderr)

    def test_rebuild_proceeds_when_r2t2_runtime_is_complete(self):
        result, calls = self.run_r2t2_gate("--rebuild-engine", archive=True, fingerprint_status=0, check_status=0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(calls[-1].endswith("build_engine.py --check-r2t2"), calls)

    def test_auto_reuses_matching_archive_without_checking_r2t2(self):
        result, calls = self.run_r2t2_gate("--auto-engine", archive=True, fingerprint_status=0, check_status=1)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any("--check-r2t2" in call for call in calls), calls)

    def test_auto_blocks_when_fingerprint_changed_and_runtime_is_missing(self):
        result, calls = self.run_r2t2_gate("--auto-engine", archive=True, fingerprint_status=1, check_status=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(calls[-1].endswith("build_engine.py --check-r2t2"), calls)

    def test_auto_without_archive_requires_r2t2_runtime(self):
        result, calls = self.run_r2t2_gate("--auto-engine", archive=False, fingerprint_status=0, check_status=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(calls), 1, calls)
        self.assertTrue(calls[0].endswith("build_engine.py --check-r2t2"), calls)


if __name__ == "__main__":
    unittest.main()
