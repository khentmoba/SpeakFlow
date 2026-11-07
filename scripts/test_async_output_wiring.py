"""Source correspondence only; actual queue decisions have Rust replay tests."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def body(path: str, name: str) -> str:
    source = (ROOT / path).read_text(encoding="utf-8")
    start = re.search(rf"(?m)^(?:pub(?:\([^)]*\))? )?(?:async )?fn {name}\(", source)
    if not start:
        raise AssertionError(f"missing production function {name}")
    remainder = source[start.end():]
    end = re.search(r"(?m)^(?:pub(?:\([^)]*\))? )?(?:async )?fn \w+\(", remainder)
    return remainder[:end.start()] if end else remainder


class FixtureIsolation(unittest.TestCase):
    def test_child_fixtures_use_speakflow_data_directory_override(self):
        for path, functions in {
            "src-tauri/src/services/history_service.rs": [
                "run_audio_lease_test_in_child",
                "tlc_history_leases_replay_public_sqlite_operations",
            ],
            "src-tauri/src/services/funasr_service/model_readiness_tests.rs": ["run_in_isolated_cache"],
        }.items():
            for name in functions:
                # The helper closes at its own indentation: 4 spaces inside a test module, column 0 in a file module.
                source = (ROOT / path).read_text(encoding="utf-8")
                start = source.index(f"fn {name}(")
                indent = re.match(r" *", source[source.rfind("\n", 0, start) + 1 : start]).group(0)
                fixture = source[start:source.index(f"\n{indent}}}", start)]
                self.assertIn('.env("SPEAK_FLOW_DATA_DIR", &directory)', fixture)
                self.assertNotIn('"LIGHT_WHISPER_DATA_DIR"', fixture)


class SubtitleFocusWiring(unittest.TestCase):
    def test_windows_subtitle_show_never_uses_activating_tauri_show(self):
        source = body("src-tauri/src/commands/window.rs", "show_subtitle_window_unlocked")
        self.assertNotIn(".show()", source)
        self.assertNotIn(".set_always_on_top(", source)
        self.assertIn("show_overlay_without_activation(&window)", source)

    def test_windows_subtitle_visibility_and_interaction_use_native_operations(self):
        show = body("src-tauri/src/commands/window.rs", "show_overlay_without_activation")
        self.assertIn("SWP_NOACTIVATE", show)
        self.assertIn("SWP_SHOWWINDOW", show)
        interaction = body("src-tauri/src/commands/window.rs", "set_subtitle_window_interactive")
        self.assertIn("WS_EX_NOACTIVATE", interaction)
        self.assertIn("WS_EX_TRANSPARENT | WS_EX_LAYERED", interaction)
        self.assertIn("SetWindowLongPtrW", interaction)
        create = body("src-tauri/src/commands/window.rs", "create_subtitle_window_unlocked")
        self.assertIn(".focusable(false)", create)
        hide = body("src-tauri/src/commands/window.rs", "hide_subtitle_window_inner")
        self.assertIn("SW_HIDE", hide)


class OutputDrainWiring(unittest.TestCase):
    def test_start_error_schedules_the_shared_drain(self):
        self.assertRegex(body("src-tauri/src/commands/audio.rs", "emit_start_error"),
                         r"audio_service::flush_pending_paste\(app_handle\)")

    def test_start_cancel_schedules_before_the_terminal_branch(self):
        source = body("src-tauri/src/commands/audio.rs", "stop_recording_inner")
        branch = source.index("let session = match recording {")
        self.assertIn("audio_service::flush_pending_paste(&app_handle)", source[:branch])

    def test_success_and_error_terminal_helpers_schedule_the_shared_drain(self):
        for name in ["emit_done_with_timing", "emit_terminal_outcome"]:
            self.assertIn("flush_pending_paste(app)",
                          body("src-tauri/src/services/audio_service/finalize.rs", name))


if __name__ == "__main__":
    unittest.main()
