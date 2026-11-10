"""The formal gate must distinguish checked properties from reached actions."""

import hashlib
import json
import re
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_formal


class ToolPinTests(unittest.TestCase):
    def test_ci_and_verifier_require_the_same_tlc_binary(self):
        workflow = (check_formal.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        pins = [match.group(0)
                for line in workflow.splitlines()
                if '"$RUNNER_TEMP/tla2tools.jar" | sha256sum --check' in line
                and (match := re.search(r"[0-9a-f]{64}", line))]
        self.assertEqual(pins, [check_formal.TLC_SHA256])


class ModelBindingTests(unittest.TestCase):
    def check(self, body: str, configuration: str, bound: str = "NoLeaks",
              observed: str = "", actions: list[str] | None = None) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "formal").mkdir()
            (root / "logs" / "Fixture").mkdir(parents=True)
            (root / "formal" / "Fixture.tla").write_text(body, encoding="utf-8")
            (root / "formal" / "Fixture.cfg").write_text(configuration, encoding="utf-8")
            (root / "logs" / "Fixture" / "tlc.log").write_text(observed, encoding="utf-8")
            manifest = {"runs": {"Fixture": ["Fixture.cfg"]},
                        "contracts": {"fixture": {"models": {"Fixture": [bound]}}},
                        "actions": {"Fixture": actions or []}}
            with patch.object(check_formal, "ROOT", root):
                check_formal.check_model_bindings(manifest, "Fixture", root / "logs")

    def test_reached_predicate_is_not_a_checked_invariant(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\n", "CHECK_DEADLOCK FALSE\n",
                       observed="<NoLeaks line 1, col 1 to line 1, col 15 of module Fixture>: 1:1\n")

    def test_negation_does_not_prove_its_operand(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nSafety == ~NoLeaks\n", "INVARIANT Safety\n")

    def test_comment_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == Other \\* NoLeaks\n",
                       "INVARIANT Safety\n")

    def test_string_literal_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check('NoLeaks == TRUE\nSafety == "NoLeaks" = "NoLeaks"\n',
                       "INVARIANT Safety\n")

    def test_block_comment_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == Other (* NoLeaks *)\n",
                       "INVARIANT Safety\n")

    def test_config_comment_does_not_check_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\n", "(*\nINVARIANT NoLeaks\n*)\n")

    def test_positive_conjunction_proves_each_named_conjunct(self):
        self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == /\\ NoLeaks /\\ Other\n",
                   "INVARIANT Safety\n")

    def test_direct_temporal_property_is_checked(self):
        self.check("Settles == [] TRUE\n", "PROPERTY Settles\n", bound="Settles")

    def test_declared_action_must_really_be_an_action(self):
        with self.assertRaises(ValueError):
            self.check("Begin == TRUE\n", "", bound="Begin", actions=["Begin"],
                       observed="<Begin line 1, col 1 to line 1, col 13 of module Fixture>: 1:1\n")

    def test_reachable_state_transition_is_accepted(self):
        self.check("Begin == h' = h + 1\n", "", bound="Begin", actions=["Begin"],
                   observed="<Begin line 1, col 1 to line 1, col 20 of module Fixture>: 1:1\n")

    def test_inventory_check_rejects_unlisted_action_without_tlc_logs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "formal").mkdir()
            (root / "src-tauri/src").mkdir(parents=True)
            (root / "src").mkdir()
            (root / "src-tauri/src/lib.rs").write_text(
                "fn run() { tauri::generate_handler![commands::support::open_log_folder]; }\n"
                "fn open_log_folder() {}\n",
                encoding="utf-8",
            )
            (root / "formal" / "Fixture.tla").write_text(
                "ReadSnapshot == s' = s\nOpenUrl == s' = s\n",
                encoding="utf-8",
            )
            (root / "formal" / "Fixture.cfg").write_text("CHECK_DEADLOCK FALSE\n", encoding="utf-8")
            manifest = {
                "commands": {"commands::support::open_log_folder": ["support"]},
                "command_implementation": {
                    "commands::support::open_log_folder": "src-tauri/src/lib.rs#open_log_folder",
                },
                "events": {},
                "lifecycle": {},
                "protocols": {},
                "negative": [],
                "runs": {"Fixture": ["Fixture.cfg"]},
                "contracts": {
                    "support": {
                        "obligation": "support obligation",
                        "models": {"Fixture": ["ReadSnapshot", "OpenUrl"]},
                        "implementation": ["src-tauri/src/lib.rs#open_log_folder"],
                        "evidence": ["src-tauri/src/lib.rs#open_log_folder"],
                        "assumptions": "none",
                    },
                },
                "actions": {"Fixture": ["OpenUrl"]},
            }
            with patch.object(check_formal, "ROOT", root):
                with self.assertRaisesRegex(
                    ValueError,
                    r"unexercised/unasserted obligations in Fixture: \['ReadSnapshot'\]",
                ):
                    check_formal.check_inventory(manifest)

    def test_reusable_requires_matching_source_config_tlc_and_pass_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "Fixture.tla"
            config = root / "Fixture.cfg"
            source.write_text("Spec == TRUE\n", encoding="utf-8")
            config.write_text("SPECIFICATION Spec\n", encoding="utf-8")
            record = {
                "model": "Fixture",
                "config": "Fixture.cfg",
                "distinct": 1,
                "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "config_sha256": hashlib.sha256(config.read_bytes()).hexdigest(),
                "tlc_sha256": check_formal.TLC_SHA256,
                "expected_violation": None,
            }
            record_path = root / "result.json"
            log_path = root / "tlc.log"
            record_path.write_text(json.dumps(record), encoding="utf-8")
            log_path.write_text("Model checking completed. No error has been found.\n", encoding="utf-8")
            self.assertEqual(check_formal.reusable(record_path, source, config), record)

            source.write_text("Spec == FALSE\n", encoding="utf-8")
            self.assertIsNone(check_formal.reusable(record_path, source, config))
            source.write_text("Spec == TRUE\n", encoding="utf-8")

            for mutated in (
                {**record, "tlc_sha256": "0" * 64},
                {**record, "expected_violation": "Safety"},
            ):
                record_path.write_text(json.dumps(mutated), encoding="utf-8")
                self.assertIsNone(check_formal.reusable(record_path, source, config))


if __name__ == "__main__":
    unittest.main()
