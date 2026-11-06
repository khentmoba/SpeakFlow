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

## Model folders

Settings → ASR Engine → Model folders lists extra folders that are searched for
model files after the models directory. They are read-only: SpeakFlow never
downloads into them or deletes from them. A file counts as the model only when
its name matches the catalog exactly and its size matches the catalog size, so a
half-copied file or another quantization is ignored.

The list is stored in `engine.json` as `model_dirs`. Rust uses it for readiness
and the installed list; the Python engine receives it as
`SPEAK_FLOW_MODEL_DIRS` and only looks there when the cache has no complete copy.

## Speech processing

Two preferences in the same section change how a recording is transcribed:

- **Skip silence before recognition** (on by default). When off, the whole
  recording is sent to the model instead of the speech regions FireRedVAD keeps.
  Confucius4-R2T2 is the exception: it always segments with its own detector to
  keep the native stream bounded, so the switch does not change it. The detector
  still runs when the switch is off, so a clip with no speech returns nothing
  instead of whatever the model invents.
- **Remove filler words** (on by default). Removes `um`, `uh`, `hmm` and similar
  sounds from the finished transcript, and only when the transcript is English:
  `um` is the article "a" in Portuguese and `er` means "he" in German, so
  neither is on the list and other languages are left alone. Words that can carry
  meaning (`like`, `so`, `you know`) are never touched, and any word inside a
  dictionary entry is never removed. Live previews keep their fillers until the
  final transcript arrives. Cloud engines and R2T2 streaming produce their own
  text and are not affected.

Both are stored in `engine.json`; a missing value means on.

## CPU recovery

If a decode fails on the GPU after the model loaded (a lost device or driver
reset), the engine reloads the model on the CPU and retries the request once.
The retry is marked in the log and reported as `backend: cpu`. A failure on the
CPU itself is returned as an error instead of looping.

## Model test

Settings → ASR Engine → Test model records five seconds from the selected
microphone and returns what the model heard. It does not run AI polish, does not
write history, and never pastes into the active application.

## Benchmarks

The browser shows each model's grade and error percentage for the selected
language, plus its speed per 30 seconds of audio. These come from Handy's
published benchmark, not from SpeakFlow. The interface says so under the list.

## Language coverage

The catalog covers 103 language codes across 58 models. The 28-, 30- and
31-language models (Nemotron, Qwen3-ASR, Fun-ASR) carry most of the useful
coverage. The multilingual Whisper variants fill the remaining 61 codes and are
marked `long_tail` in the catalog: they are never recommended, and the UI labels
them as experimental because Whisper's own benchmark grades most of those
languages D or F. The four English-only Whisper variants are offered too, but
they add no language and are not marked `long_tail`.

The selected language is saved once and resolved against the selected model at
transcription time (`resolve_language` in `src-tauri/src/utils/asr_catalog.rs`):

- a single-language model always uses that language;
- a language the model lists is used as requested;
- otherwise a model that supports language detection uses `auto`;
- otherwise English is used, then the model's first language.

Models with `translation: true` can also write English from any spoken language.
The Settings toggle only appears for those models, and the engine sends
`task: translate` with `target_language: en`.

## Retired families

SenseVoice is not offered. `EXCLUDED_PREFIXES` in the sync script drops it, and
`src-tauri/tests/retired_sensevoice_contract.rs` fails if it returns. Whisper was
retired before the catalog existed and is now offered again as the labelled
long-tail family; `src-tauri/tests/whisper_long_tail_contract.rs` keeps it out of
the recommended picks and out of a dedicated runtime.

## Licenses

`THIRD_PARTY_NOTICES.md` summarises the licenses by family. Weights are never
bundled; each model is downloaded from its own repository.
