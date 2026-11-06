# Local ASR models

SpeakFlow offers every local model in `src-tauri/resources/asr_catalog.json`.
The file is generated, not hand-edited, and it is the only list of models:

- `src-tauri/resources/hf_cache_utils.py` builds `TRANSCRIBE_CPP_ASR_MODELS` from it.
- `src-tauri/src/utils/asr_catalog.rs` embeds it with `include_str!`.
- `src/lib/asrCatalog.ts` imports it for the model browser, capabilities and picker.

`src-tauri/resources/test_local_engine_contract.py` fails when a consumer drifts
from the catalog or the catalog is malformed.

## Regenerating

The catalog is generated from the Handy model source that T3 Code publishes,
pinned to one commit:

```bash
uv run --no-sync python scripts/sync_asr_catalog.py
```

To work from a local copy instead of GitHub:

```bash
uv run --no-sync python scripts/sync_asr_catalog.py --source path/to/catalog.json
```

The script keeps a few decisions that the upstream catalog does not carry:

- `LEGACY_PINS` keeps the six engines SpeakFlow already shipped on their existing
  revisions, so their snapshot folders stay valid and nobody re-downloads. It
  also pins their KV cache type to `f16`, which is what those models were
  validated with, and keeps the established display names.
- Every other model uses the library's default session options. Moonshine Tiny
  aborts the process under `f16`, so a blanket `f16` is not safe.
- `LICENSE_OVERRIDES` names the license for models the upstream catalog reports
  as `other`, using what each GGUF repository's own metadata declares. The script
  refuses to write a catalog that still contains `other`.

## Adding a model

Add it upstream, regenerate, and run the checks. No Rust, Python or TypeScript
list needs editing. A model whose engine key would change an existing engine is
rejected by the shipped-engine test, so renaming a model upstream is a deliberate
decision rather than a silent settings migration.

## Long recordings

`qwen3_asr_server.py` asks the loaded session for its input ceiling
(`session.limits.effective_max_audio_ms`). A longer recording is cut into pieces
no longer than that ceiling, each cut landing on the quietest short frame near the
limit so words are not split mid-syllable. Pieces are transcribed in order and
joined without a space between Chinese or Japanese characters.

Models that report no ceiling (`0`) receive the whole clip, as before. A decode
that stops early with a partial transcript keeps that text instead of failing the
request.

## Large downloads

Selecting a model of 4 GB or more asks for confirmation first, naming the model
and its size. Declining leaves the current engine selected, so nothing downloads.
A model that is already on disk never asks. The threshold is
`LARGE_MODEL_DOWNLOAD_BYTES` in `src/lib/asrCatalog.ts`.

## Benchmarks

The browser shows each model's grade and error percentage for the selected
language, plus its speed per 30 seconds of audio. These come from Handy's
published benchmark, not from SpeakFlow. The interface says so under the list.

## Retired families

Whisper and SenseVoice are not offered. `EXCLUDED_PREFIXES` in the sync script
drops them, and `src-tauri/tests/retired_*_contract.rs` fails if either returns.
Their GGUF ports would run on transcribe.cpp, so removing them from that list is
a product decision, not a technical one.

## Licenses

`THIRD_PARTY_NOTICES.md` summarises the licenses by family. Weights are never
bundled; each model is downloaded from its own repository.
