"""Fingerprint the inputs of engine.tar.xz so a release can reuse an unchanged archive.

The app version is normalized out of pyproject.toml and uv.lock: a release bump
alone does not change the packaged engine.

    python scripts/engine_fingerprint.py            # print the fingerprint
    python scripts/engine_fingerprint.py --check    # exit 0 if the stored one matches
    python scripts/engine_fingerprint.py --write    # store it beside the archive
"""

import argparse
import hashlib
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STORED = ROOT / "src-tauri/resources/engine.tar.xz.inputs"
INPUT_SPECS = (
    "pyproject.toml",
    "uv.lock",
    ".python-version",
    "scripts/build_engine.py",
    "scripts/build_r2t2_runtime.py",
    "scripts/patches",
    "src-tauri/resources",
    "src-tauri/windows-app-manifest.xml",
)
VERSION_LINES = (
    (re.compile(r'^(version = ")[^"]+(")', re.M), "pyproject.toml"),
    (re.compile(r'(name = "speak-flow"\nversion = ")[^"]+(")'), "uv.lock"),
)


def tracked_inputs(root: Path = ROOT) -> list[str]:
    listed = subprocess.run(["git", "ls-files", "-z", "--", *INPUT_SPECS], cwd=root,
                            check=True, capture_output=True).stdout.decode("utf-8")
    return sorted(path for path in listed.split("\0")
                  if path and not Path(path).name.startswith("test_"))


def normalized(path: str, content: bytes) -> bytes:
    for pattern, name in VERSION_LINES:
        if path == name:
            text = content.decode("utf-8").replace("\r\n", "\n")
            content = pattern.sub(r"\g<1>0.0.0\g<2>", text, count=1).encode("utf-8")
    return content


def fingerprint(root: Path = ROOT) -> str:
    digest = hashlib.sha256()
    for path in tracked_inputs(root):
        content = normalized(path, (root / path).read_bytes())
        digest.update(path.encode("utf-8") + b"\0" + hashlib.sha256(content).digest())
    for manifest in sorted((root / "src-tauri/resources/r2t2-native").glob("*/runtime-manifest.json")):
        digest.update(manifest.relative_to(root).as_posix().encode("utf-8") + b"\0" + manifest.read_bytes())
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true")
    action.add_argument("--write", action="store_true")
    args = parser.parse_args()
    current = fingerprint()
    if args.write:
        STORED.write_text(current + "\n", encoding="utf-8")
    elif args.check:
        try:
            stored = STORED.read_text(encoding="utf-8").strip()
        except OSError:
            return 1
        return 0 if stored == current else 1
    else:
        print(current)
    return 0


if __name__ == "__main__":
    sys.exit(main())
