#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Low-latency local GGUF ASR server backed by transcribe.cpp.

Serves every model in asr_catalog.json. All run through the same transcribe.cpp
session, FireRedVAD front end and offline transcription path.
"""

import importlib.metadata
import os
import subprocess
import time
import traceback

from firered_vad import FireRedVad

from server_common import (
    BaseASRServer,
    _has_nvidia_gpu,
    apply_hf_env_defaults,
    decode_inline_audio,
    ensure_safe_cuda_env,
    setup_rotating_logger,
)

apply_hf_env_defaults()
ensure_safe_cuda_env()
logger = setup_rotating_logger(__name__, "qwen3_asr_server.log", "Local ASR server")

from hf_cache_utils import TRANSCRIBE_CPP_ASR_MODELS, find_hf_snapshot_file

QWEN3_ASR_N_CTX = 32_768
QWEN3_VAD_SAMPLE_RATE = 16_000
# Clips longer than a model's input ceiling are cut near this quiet-search window.
SPLIT_SEARCH_SECONDS = 3.0
SPLIT_FRAME_SECONDS = 0.05
ENGINE_DISPLAY_NAMES = {key: model["name"] for key, model in TRANSCRIBE_CPP_ASR_MODELS.items()}


def _is_unspaced_script(char):
    """Chinese, Japanese kana and Thai are written without spaces between words."""
    code = ord(char)
    return (
        0x3040 <= code <= 0x30FF
        or 0x3400 <= code <= 0x9FFF
        or 0xF900 <= code <= 0xFAFF
        or 0xFF00 <= code <= 0xFFEF
        or 0x0E00 <= code <= 0x0E7F
        or 0x3000 <= code <= 0x303F
    )


class Qwen3ASRServer(BaseASRServer):
    """Keep one GGUF model and one KV session resident across requests.

    Idle unload, when the user turns it on, closes both and the next
    transcribe calls initialize() again. FireRedVAD stays loaded.
    """

    def __init__(self, engine=None):
        engine = engine or os.environ.get("SPEAK_FLOW_ASR_ENGINE", "qwen3-asr-1.7b")
        if engine not in TRANSCRIBE_CPP_ASR_MODELS:
            raise ValueError(f"Unsupported transcribe.cpp ASR engine: {engine}")
        self.model_config = TRANSCRIBE_CPP_ASR_MODELS[engine]
        self.display_name = ENGINE_DISPLAY_NAMES[engine]
        super().__init__(engine=engine, logger=logger)
        self.model = None
        self.session = None
        self.vad_model = None
        self.backend = "cuda" if self.device == "cuda" else "auto"
        self._last_load_error = None
        # Filled in once the runtime is loaded; empty means "catch nothing".
        self._truncated_errors = ()
        self._total_inference_ms = 0.0
        self._total_vad_ms = 0.0
        self._vad_calls = 0
        self._vad_rejected = 0

    def _detect_device(self):
        """Avoid importing the 2 GB PyTorch runtime just to probe one CUDA driver."""
        if _has_nvidia_gpu():
            logger.info("Detected NVIDIA CUDA driver; preferring CUDA for %s", self.display_name)
            return "cuda"
        logger.info("NVIDIA CUDA driver not detected; %s will automatically select Vulkan/CPU", self.display_name)
        return "cpu"

    def _get_gpu_device_info(self):
        info = {"device": self.device}
        if self.backend != "cuda":
            return info
        try:
            query = subprocess.run(
                [
                    "nvidia-smi",
                    "--query-gpu=name,memory.total",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                check=False,
                timeout=3,  # Optional metadata must not block model initialization.
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            name, memory_mb = [part.strip() for part in query.stdout.splitlines()[0].split(",", 1)]
            info["gpu_name"] = name
            info["gpu_memory_total"] = round(float(memory_mb) / 1024, 1)
        except (OSError, subprocess.TimeoutExpired, IndexError, ValueError):
            pass
        return info

    def _cleanup_memory(self):
        # transcribe.cpp owns its CUDA allocations; torch.empty_cache() cannot
        # release them and importing torch here would add substantial overhead.
        import gc

        gc.collect()

    def _get_model_repos(self):
        # Exact-file validation happens in initialize(); a repository may contain
        # Q4/Q6 files that must not be mistaken for the selected Q8 model.
        return []

    def _resolve_model_path(self):
        return find_hf_snapshot_file(
            self.model_config["repo_id"], self.model_config["filename"]
        )

    def _suspend_gpu_runtime(self):
        """Unload the GGUF model and KV session. FireRedVAD stays on CPU."""
        if not self.initialized:
            return False
        vad = self.vad_model
        self._close_runtime()
        self._gpu_suspended = True
        self.vad_model = vad
        self.logger.info("%s idle timeout reached; unloaded the GPU model; FireRedVAD remains on CPU", self.display_name)
        return True

    def _close_runtime(self):
        self.initialized = False
        if self.session is not None:
            try:
                self.session.close()
            except Exception:
                pass
            self.session = None
        if self.model is not None:
            try:
                self.model.close()
            except Exception:
                pass
            self.model = None

    def _preferred_backends(self):
        preferred = ["cuda", "vulkan", "cpu"] if self.device == "cuda" else ["auto", "cpu"]
        suffix = self.engine.upper().replace("-", "_").replace(".", "_")
        requested = (
            os.environ.get(f"SPEAK_FLOW_ASR_BACKEND_{suffix}")
            or os.environ.get("SPEAK_FLOW_ASR_BACKEND", "auto")
        ).strip().lower()
        if requested not in ("auto", "cuda", "vulkan", "cpu"):
            raise ValueError(f"Unsupported ASR backend preference: {requested}")
        if requested == "auto":
            return preferred
        return list(dict.fromkeys([requested, *preferred]))

    def _load_runtime(self, model_path):
        import transcribe_cpp

        # A truncated decode keeps the text it produced; callers prefer that
        # over losing the whole recording. OutputRepetition is a subclass, but
        # both names are read so a renamed base still matches.
        self._truncated_errors = tuple(
            error
            for name in ("OutputTruncated", "OutputRepetition")
            if (error := getattr(transcribe_cpp, name, None)) is not None
        )

        preferred = self._preferred_backends()
        errors = []
        for backend in preferred:
            try:
                with self.stdout_suppressor.suppress():
                    self.model = transcribe_cpp.Model(model_path, backend=backend)
                    self.session = self.model.session(**self._session_options())
                self.backend = backend
                self.device = "cuda" if backend == "cuda" else backend
                return
            except Exception as exc:
                errors.append(f"{backend}: {exc}")
                self._close_runtime()
                logger.warning("%s %s backend failed to load: %s", self.display_name, backend, exc)
        raise RuntimeError("; ".join(errors))

    def _session_options(self):
        """Models shipped before the catalog keep their validated f16 KV cache.

        Others use the library defaults: Moonshine Tiny aborts the process
        under f16, and a lowered n_ctx narrows no model that needs it.
        """
        if self.model_config.get("kv_type") == "f16":
            return {"kv_type": "f16", "n_ctx": QWEN3_ASR_N_CTX}
        return {}

    def _max_input_samples(self):
        """Longest clip the loaded session accepts, or 0 when it states none."""
        try:
            milliseconds = int(self.session.limits.effective_max_audio_ms)
        except Exception:
            return 0
        if milliseconds <= 0:
            return 0
        return milliseconds * QWEN3_VAD_SAMPLE_RATE // 1000

    @staticmethod
    def _split_audio(audio, max_samples):
        """Cut a long clip into pieces no longer than max_samples.

        Each cut lands on the quietest short frame near the limit so words are
        not split in the middle when the speaker pauses.
        """
        import numpy as np

        if max_samples <= 0 or len(audio) <= max_samples:
            return [audio]
        frame = max(1, int(SPLIT_FRAME_SECONDS * QWEN3_VAD_SAMPLE_RATE))
        search = min(max_samples // 2, int(SPLIT_SEARCH_SECONDS * QWEN3_VAD_SAMPLE_RATE))
        pieces = []
        start = 0
        while len(audio) - start > max_samples:
            limit = start + max_samples
            lo = max(start + frame, limit - search)
            cut = limit
            best = None
            for frame_start in range(lo, limit - frame + 1, frame):
                energy = float(np.mean(np.square(audio[frame_start:frame_start + frame])))
                if best is None or energy <= best:
                    best = energy
                    cut = frame_start + frame // 2
            pieces.append(np.ascontiguousarray(audio[start:cut]))
            start = cut
        pieces.append(np.ascontiguousarray(audio[start:]))
        return pieces

    @staticmethod
    def _join_pieces(texts):
        """Join piece transcripts, adding a space only between spaced scripts."""
        joined = ""
        for text in (t.strip() for t in texts):
            if not text:
                continue
            if joined:
                # Chinese and Japanese are written without spaces; Latin next to
                # them keeps one, as usual in mixed-script text.
                spaced = not (_is_unspaced_script(joined[-1]) and _is_unspaced_script(text[0]))
                joined += (" " if spaced else "") + text
            else:
                joined = text
        return joined

    def _warmup_inference(self):
        try:
            import numpy as np

            started = time.perf_counter()
            rng = np.random.default_rng(0)
            audio = rng.standard_normal(16_000).astype(np.float32) * 0.002
            self.vad_model.warmup()
            with self.stdout_suppressor.suppress():
                self.session.run(audio, timestamps="none")
            logger.info(
                "%s and FireRedVAD warmup completed in %.3f seconds",
                self.display_name,
                time.perf_counter() - started,
            )
        except Exception as exc:
            logger.warning(
                "%s warmup failed (the first transcription may be slower): %s",
                self.display_name,
                exc,
            )

    def _filter_speech(self, audio):
        import numpy as np

        started = time.perf_counter()
        chunks = self.vad_model.speech_timestamps(audio)
        vad_ms = (time.perf_counter() - started) * 1000
        self._vad_calls += 1
        self._total_vad_ms += vad_ms

        if not chunks:
            self._vad_rejected += 1
            return np.empty(0, dtype=np.float32), 0, vad_ms

        start = max(0, int(chunks[0]["start"]))
        end = min(len(audio), int(chunks[-1]["end"]))
        if end <= start:
            self._vad_rejected += 1
            return np.empty(0, dtype=np.float32), 0, vad_ms

        # Preserve pauses between spoken regions. Qwen still receives natural
        # phrase timing; only idle time before/after the utterance is removed.
        return np.ascontiguousarray(audio[start:end]), len(chunks), vad_ms

    def initialize(self):
        if self.initialized:
            return {"success": True, "message": "Model is initialized", "engine": self.engine}

        model_path = self._resolve_model_path()
        if not model_path:
            return {
                "success": False,
                "error": f"{self.display_name} Q8 model not downloaded: {self.model_config['filename']}",
                "type": "models_not_downloaded",
                "engine": self.engine,
            }

        started = time.perf_counter()
        try:
            logger.info("Loading %s: %s", self.display_name, model_path)
            self._load_runtime(model_path)
            if self.vad_model is None:
                self.vad_model = FireRedVad()
            self._warmup_inference()
            self.initialized = True
            self._gpu_suspended = False
            self._last_load_error = None
            elapsed = time.perf_counter() - started
            logger.info("%s initialization completed; backend: %s; elapsed: %.3f seconds", self.display_name, self.backend, elapsed)
            return {
                "success": True,
                "message": f"{self.display_name} initialized successfully in {elapsed:.2f} seconds",
                "model_loaded": True,
                "engine": self.engine,
                "backend": self.backend,
                **self._get_gpu_device_info(),
            }
        except ImportError as exc:
            self._close_runtime()
            self.vad_model = None
            self._last_load_error = str(exc)
            logger.error("Failed to load %s dependencies: %s", self.display_name, exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"Failed to load {self.display_name} dependencies: {exc}",
                "type": "import_error",
                "engine": self.engine,
            }
        except Exception as exc:
            self._close_runtime()
            self.vad_model = None
            self._last_load_error = str(exc)
            logger.error("%s initialization failed: %s", self.display_name, exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"{self.display_name} initialization failed: {exc}",
                "type": "init_error",
                "engine": self.engine,
            }

    @staticmethod
    def _resample(audio, source_rate):
        if source_rate == 16_000:
            return audio
        import numpy as np

        target_length = int(round(len(audio) * 16_000 / source_rate))
        if target_length <= 0:
            return np.empty(0, dtype=np.float32)
        return np.interp(
            np.linspace(0, max(0, len(audio) - 1), target_length),
            np.arange(len(audio), dtype=np.float64),
            audio,
        ).astype(np.float32)

    def _load_audio(self, audio_path, audio_base64, audio_format, sample_rate):
        import numpy as np

        if audio_base64:
            audio, duration = decode_inline_audio(audio_base64, audio_format, sample_rate)
            if not isinstance(audio, np.ndarray):
                raise ValueError(f"{self.display_name} in-memory input supports PCM only")
            source_rate = sample_rate or 16_000
            audio = self._resample(audio, source_rate)
            return np.ascontiguousarray(audio, dtype=np.float32), duration, "memory"

        if not audio_path or not os.path.exists(audio_path):
            raise FileNotFoundError(f"Audio file not found: {audio_path}")
        import soundfile as sf

        audio, source_rate = sf.read(
            audio_path,
            dtype="float32",
            always_2d=True,
        )
        audio = audio.mean(axis=1, dtype=np.float32)
        audio = self._resample(audio, source_rate)
        return np.ascontiguousarray(audio), len(audio) / 16_000.0, "path"

    def transcribe_audio(
        self,
        audio_path,
        options=None,
        hot_words=None,
        audio_base64=None,
        audio_format=None,
        sample_rate=None,
    ):
        if not self.initialized:
            init_result = self.initialize()
            if not init_result["success"]:
                return init_result

        input_mode = "memory" if audio_base64 else "path"
        try:
            audio, duration, input_mode = self._load_audio(
                audio_path, audio_base64, audio_format, sample_rate
            )
            self.total_audio_duration += duration
            if duration < 0.5:
                return {
                    "success": True,
                    "text": "",
                    "duration": duration,
                    "engine": self.engine,
                    "input_mode": input_mode,
                }

            audio, vad_segments, vad_ms = self._filter_speech(audio)
            speech_duration = len(audio) / float(QWEN3_VAD_SAMPLE_RATE)
            if not vad_segments:
                return {
                    "success": True,
                    "text": "",
                    "raw_text": "",
                    "duration": duration,
                    "speech_duration": 0.0,
                    "language": "unknown",
                    "engine": self.engine,
                    "model_type": self.engine,
                    "backend": self.backend,
                    "input_mode": input_mode,
                    "vad_segments": 0,
                    "vad_ms": round(vad_ms, 3),
                    "inference_ms": 0.0,
                }

            run_options = {"timestamps": "none"}
            if hot_words and self.model.supports("vocabulary"):
                run_options["vocabulary"] = hot_words
            pieces = self._split_audio(audio, self._max_input_samples())
            started = time.perf_counter()
            results = []
            with self.stdout_suppressor.suppress():
                # These models do not advertise speculative decoding. Keeping a
                # persistent KV session is the supported low-latency path.
                for piece in pieces:
                    try:
                        results.append(self.session.run(piece, **run_options))
                    except self._truncated_errors as exc:
                        partial = getattr(exc, "partial_result", None)
                        if partial is None or not (partial.text or "").strip():
                            raise
                        logger.warning(
                            "%s stopped early (%s); keeping the partial transcript",
                            self.display_name,
                            type(exc).__name__,
                        )
                        results.append(partial)
            inference_ms = (time.perf_counter() - started) * 1000
            self._total_inference_ms += inference_ms
            self.transcription_count += 1

            text = self._join_pieces(r.text for r in results)
            language = results[0].language or "unknown"
            self._maybe_cleanup(duration)
            return {
                "success": True,
                "text": text,
                "raw_text": text,
                "confidence": 0.0,
                "duration": duration,
                "speech_duration": round(speech_duration, 3),
                "language": language,
                "engine": self.engine,
                "model_type": self.engine,
                "backend": self.backend,
                "input_mode": input_mode,
                "vad_segments": vad_segments,
                "vad_ms": round(vad_ms, 3),
                "inference_ms": round(inference_ms, 3),
            }
        except Exception as exc:
            logger.error("%s transcription failed: %s", self.display_name, exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"Audio transcription failed: {exc}",
                "type": "transcription_error",
                "input_mode": input_mode,
            }

    def get_performance_stats(self):
        return {
            "transcription_count": self.transcription_count,
            "total_audio_duration": round(self.total_audio_duration, 2),
            "average_inference_ms": round(
                self._total_inference_ms / max(1, self.transcription_count), 3
            ),
            "average_vad_ms": round(self._total_vad_ms / max(1, self._vad_calls), 3),
            "vad_calls": self._vad_calls,
            "vad_rejected": self._vad_rejected,
            "initialized": self.initialized,
            "engine": self.engine,
            "backend": self.backend,
            "speculative_decoding": False,
            "models_loaded": {
                "asr": self.model is not None,
                "vad": self.vad_model is not None,
                "punc": True,
            },
        }

    def check_status(self):
        try:
            version = importlib.metadata.version("transcribe-cpp")
            model_loaded = self.model is not None and self.session is not None
            return self._with_gpu_suspend_flag({
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": version,
                "engine": self.engine,
                "backend": self.backend,
                "model_loaded": model_loaded,
                "models": {
                    "asr": model_loaded,
                    "vad": self.vad_model is not None,
                    "punc": True,
                },
                **self._get_gpu_device_info(),
            })
        except importlib.metadata.PackageNotFoundError as exc:
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": f"Failed to load {self.display_name} dependencies: {exc}",
                "engine": self.engine,
            }


if __name__ == "__main__":
    Qwen3ASRServer().run()
