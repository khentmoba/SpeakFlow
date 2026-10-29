# Qwen vocabulary recognition check

The `transcribe.cpp` 0.4.1 runtime accepts vocabulary during Qwen3-ASR decoding. SpeakFlow forwards the profile's selected vocabulary to `Session.run(vocabulary=...)` only when the loaded model advertises that capability. The list belongs to each request; removing words does not leave them in the next request.

Upstream documents the Qwen [system-message vocabulary format](https://github.com/handy-computer/transcribe.cpp/blob/v0.4.1/docs/models/qwen3-asr-1.7b.md) and [capability contract](https://github.com/handy-computer/transcribe.cpp/blob/v0.4.1/docs/prompting.md).

## Observed result

Tested on Windows with Qwen3-ASR 1.7B Q8_0 and runtime 0.4.1. Vulkan and CUDA produced the same results below. The CUDA check used the actual pinned provider installed by the engine packaging helper. The same 16 kHz mono Windows synthetic-speech recording was processed three times through the real Python ASR server. Rust vocabulary correction and AI polish were bypassed; these are the model's raw transcripts.

Synthetic speech input:

> Tauri uses Qwen to recognize speech. The app was built by Kent. Please open LibWhisper.

| Request | Vocabulary | Raw model transcript |
| --- | --- | --- |
| First | Empty | Tora uses Quen to recognize speech. The app was built by Kant. Please open Lib Whisper. |
| Second | Tauri, Qwen, Khent, libwhisper | Tauri uses Qwen to recognize speech. The app was built by Kent. Please open Libwhisper. |
| Third | Empty again | Tora uses Quen to recognize speech. The app was built by Kant. Please open Lib Whisper. |

The change to `Tauri` and `Qwen`, followed by the return to `Tora` and `Quen` after clearing the list, demonstrates conditioning inside recognition. The requested spelling `Khent` was not adopted. A separate synthetic recording about Kent and SpeakFlow also retained `Kent` with that spelling in the vocabulary. Hints influence recognition but do not force exact substitutions. These small synthetic probes do not establish a general word-error-rate improvement or performance comparison.

## Reproduce

With dependencies installed using `uv sync --frozen` and the Qwen 1.7B model cached, create the recording in PowerShell:

```powershell
Add-Type -AssemblyName System.Speech
$voice = New-Object System.Speech.Synthesis.SpeechSynthesizer
$format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$voice.SetOutputToWaveFile("$env:TEMP\qwen-vocabulary.wav", $format)
$voice.Speak("Tauri uses Qwen to recognize speech. The app was built by Kent. Please open LibWhisper.")
$voice.Dispose()
```

Run this Python probe from the repository root using `uv run --no-sync python`:

```python
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path.cwd() / "src-tauri" / "resources"))
os.environ["SPEAK_FLOW_ASR_BACKEND_QWEN3_ASR_1_7B"] = "vulkan"
from qwen3_asr_server import Qwen3ASRServer

server = Qwen3ASRServer(engine="qwen3-asr-1.7b")
assert server.initialize()["success"]
try:
    assert server.model.supports("vocabulary")
    audio = str(Path(os.environ["TEMP"]) / "qwen-vocabulary.wav")
    for words in ([], ["Tauri", "Qwen", "Khent", "libwhisper"], []):
        result = server.transcribe_audio(audio, hot_words=words)
        assert result["success"], result
        print(words, result["raw_text"])
finally:
    server._close_runtime()
```

Voice choice and runtime/backend differences can change the transcript. The automated regression uses the server's real transcription path with a fake native session to assert the exact per-request vocabulary arguments for both Qwen sizes and the absence of those arguments for an unsupported model.

The API and default native provider are pinned together at 0.4.1 in `uv.lock`. The Windows CUDA provider is separately pinned to 0.4.1 with the release asset's SHA-256 in `scripts/build_engine.py`. A release must rebuild its engine archive to distribute this runtime upgrade.
