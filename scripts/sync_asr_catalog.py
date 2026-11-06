#!/usr/bin/env python3
"""Regenerate the local model catalog from the Handy model catalog.

T3 Code vendors the same catalog (generated from @handy-computer/models) as
apps/server/src/speech/catalog.json. The pinned commit below is the one that
last changed it. Python, Rust and TypeScript all read the generated files, so a
model is added or removed here and nowhere else.

Two files are written:

- `asr_catalog.json` names and pins every model. The engine, the Rust backend and
  the model picker read it.
- `asr_benchmarks.json` holds the published grades, error rates and speed. Only
  the model browser reads it, so it stays out of the startup bundle.

    uv run --no-sync python scripts/sync_asr_catalog.py
    uv run --no-sync python scripts/sync_asr_catalog.py --source path/to/catalog.json
"""

import argparse
import json
import re
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
OUTPUT = REPO_ROOT / "src-tauri" / "resources" / "asr_catalog.json"
BENCHMARK_OUTPUT = REPO_ROOT / "src-tauri" / "resources" / "asr_benchmarks.json"

SOURCE_COMMIT = "b8dce83214912ca48b50bcec0f299a1fd46fa39d"
SOURCE_URL = (
    "https://raw.githubusercontent.com/pingdotgg/t3code/"
    f"{SOURCE_COMMIT}/apps/server/src/speech/catalog.json"
)

# Engines SpeakFlow shipped before the catalog existed. Their snapshot folders
# are named after these revisions, their KV cache type is already validated, and
# they keep their established names, so none of that changes. Every other model
# uses the library default KV type: Moonshine Tiny aborts the whole process
# under f16.
LEGACY_PINS = {
    "qwen3-asr-0.6b": {"vocabulary": True},
    "qwen3-asr-1.7b": {"vocabulary": True},
    "parakeet-unified-en-0.6b": {
        "revision": "843b1b175ace04f5a89d379c79e2518c0d065b25",
        "name": "Parakeet Unified EN 0.6B",
        # THIRD_PARTY_NOTICES.md explains why the NVIDIA card governs, not CC-BY.
        "license": "nvidia-open-model-license",
    },
    "parakeet-tdt-0.6b-v2": {"revision": "58239cf6d0ecb0af921e7936c1e5e0a8dba91dbb"},
    "canary-qwen-2.5b": {"revision": "a4480da236b7d99d7a12186868de021ef4ff5ced"},
    "cohere-transcribe-03-2026": {"revision": "7353b6040bdc1be9e5a3823d99d5fe12afe88264"},
}

# The catalog reports "other" for these. The names below are the license each
# GGUF repository's own metadata declares (huggingface.co/api/models/<repo>).
LICENSE_OVERRIDES = {
    "nemotron-3.5-asr-streaming-0.6b": "openmdw-1.1",
    "nemotron-speech-streaming-en-0.6b": "nvidia-open-model-license",
    "multitalker-parakeet-streaming-0.6b-v1": "nvidia-open-model-license",
    "fun-asr-nano-2512": "funasr-model-license-v1.1",
    "fun-asr-mlt-nano-2512": "funasr-model-license-v1.1",
    "sensevoicesmall": "funasr-model-license",
    "medasr": "health-ai-developer-foundations",
}

_KEY_RE = re.compile(r"^[a-z0-9][a-z0-9._-]*$")


def engine_key(repo_id):
    repo = repo_id.split("/", 1)[1]
    if repo.endswith("-gguf"):
        repo = repo[: -len("-gguf")]
    key = repo.lower()
    if not _KEY_RE.match(key):
        raise ValueError(f"Cannot derive an engine key from {repo_id}")
    return key


def convert(source):
    models = []
    benchmarks = {}
    seen = set()
    for item in source["models"]:
        key = engine_key(item["id"])
        if key in seen:
            raise ValueError(f"Duplicate engine key {key}")
        seen.add(key)
        pin = LEGACY_PINS.get(key)
        model = {
            "key": key,
            "name": item["name"],
            "description": item["description"],
            "repo_id": item["id"],
            "filename": item["filename"],
            "revision": item["revision"],
            "size": item["size"],
            "sha256": item["sha256"],
            "license": LICENSE_OVERRIDES.get(key, item["license"]),
            "recommended": bool(item["recommended"]),
            "translation": bool(item["supportsTranslation"]),
            "languages": list(item["languages"]),
            "vocabulary": False,
        }
        if pin is not None:
            model.update(pin)
            model["kv_type"] = "f16"
        if model["license"] == "other":
            raise ValueError(f"{key} has no named license; add it to LICENSE_OVERRIDES")
        models.append(model)
        benchmarks[key] = {
            "speed": item["benchmark"]["secondsPer30s"],
            "accuracy": {
                language: [entry["grade"], entry["errorPercent"]]
                for language, entry in item["benchmark"]["accuracy"].items()
            },
        }
    missing = set(LEGACY_PINS) - seen
    if missing:
        raise ValueError(f"Catalog no longer offers shipped engines: {sorted(missing)}")
    catalog = {
        "source": source["source"] + " " + source["sourceVersion"],
        "engine_version": source["engineVersion"],
        "source_commit": SOURCE_COMMIT,
        "models": models,
    }
    return catalog, {"source_commit": SOURCE_COMMIT, "models": benchmarks}


def render(document, key):
    """One entry per line, so a regenerated diff shows only what changed."""
    head = {k: v for k, v in document.items() if k != key}
    entries = document[key]
    if isinstance(entries, list):
        lines = [json.dumps(entry, ensure_ascii=False, separators=(",", ":")) for entry in entries]
        body = "[\n" + ",\n".join(lines) + "\n]"
    else:
        lines = [
            f"{json.dumps(name, ensure_ascii=False)}:"
            f"{json.dumps(entry, ensure_ascii=False, separators=(',', ':'))}"
            for name, entry in entries.items()
        ]
        body = "{\n" + ",\n".join(lines) + "\n}"
    prefix = json.dumps(head, ensure_ascii=False, separators=(",", ":"))[:-1]
    return prefix + f',"{key}":' + body + "}\n"


def load_source(location):
    if location.startswith(("http://", "https://")):
        with urllib.request.urlopen(location, timeout=60) as response:
            return json.loads(response.read().decode("utf-8"))
    return json.loads(Path(location).read_text(encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--source", default=SOURCE_URL)
    parser.add_argument("--output", default=str(OUTPUT))
    parser.add_argument("--benchmark-output", default=str(BENCHMARK_OUTPUT))
    args = parser.parse_args()
    catalog, benchmarks = convert(load_source(args.source))
    Path(args.output).write_text(render(catalog, "models"), encoding="utf-8", newline="\n")
    Path(args.benchmark_output).write_text(
        render(benchmarks, "models"), encoding="utf-8", newline="\n"
    )
    print(f"Wrote {len(catalog['models'])} models to {args.output}")
    print(f"Wrote benchmarks to {args.benchmark_output}")


if __name__ == "__main__":
    main()
