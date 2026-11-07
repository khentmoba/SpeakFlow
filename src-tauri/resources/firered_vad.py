#!/usr/bin/env python3
# -*- coding: utf-8 -*-

# Adapted from FireRedTeam/FireRedVAD, licensed under Apache-2.0.
# Modified by SpeakFlow contributors; see THIRD_PARTY_NOTICES.md.

"""Small offline FireRedVAD runtime for the bundled Qwen3-ASR engines."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path

import numpy as np


SAMPLE_RATE = 16_000
FRAME_SHIFT_SAMPLES = 160
FRAME_LENGTH_SAMPLES = 400
MODEL_FILENAME = "fireredvad_vad.onnx"
CMVN_FILENAME = "fireredvad_cmvn.json"


@dataclass(frozen=True)
class FireRedVadOptions:
    threshold: float = 0.5
    smooth_window_frames: int = 5
    min_speech_duration_ms: int = 120
    min_silence_duration_ms: int = 300
    speech_pad_ms: int = 200


def _resource_path(filename: str) -> Path:
    return Path(__file__).resolve().with_name(filename)


class FireRedVad:
    """Run the official non-streaming FireRedVAD ONNX model on 16 kHz PCM."""

    def __init__(
        self,
        model_path: str | Path | None = None,
        cmvn_path: str | Path | None = None,
        options: FireRedVadOptions | None = None,
        intra_op_num_threads: int | None = None,
    ):
        import kaldi_native_fbank as knf
        import onnxruntime as ort

        self.options = options or FireRedVadOptions()
        self.model_path = Path(model_path) if model_path else _resource_path(MODEL_FILENAME)
        self.cmvn_path = Path(cmvn_path) if cmvn_path else _resource_path(CMVN_FILENAME)

        if not self.model_path.is_file():
            raise FileNotFoundError(f"FireRedVAD model not found: {self.model_path}")
        if not self.cmvn_path.is_file():
            raise FileNotFoundError(f"FireRedVAD CMVN file not found: {self.cmvn_path}")

        cmvn = json.loads(self.cmvn_path.read_text(encoding="utf-8"))
        self._mean = np.asarray(cmvn["mean"], dtype=np.float32)
        self._inverse_std = np.asarray(cmvn["inverse_std"], dtype=np.float32)
        if self._mean.shape != (80,) or self._inverse_std.shape != (80,):
            raise ValueError("FireRedVAD CMVN must contain 80-dimensional mean and inverse_std arrays")

        fbank_options = knf.FbankOptions()
        fbank_options.frame_opts.samp_freq = SAMPLE_RATE
        fbank_options.frame_opts.frame_length_ms = 25
        fbank_options.frame_opts.frame_shift_ms = 10
        fbank_options.frame_opts.dither = 0
        fbank_options.frame_opts.snip_edges = True
        fbank_options.mel_opts.num_bins = 80
        fbank_options.mel_opts.debug_mel = False
        self._knf = knf
        self._fbank_options = fbank_options

        session_options = ort.SessionOptions()
        session_options.inter_op_num_threads = 1
        session_options.intra_op_num_threads = (
            min(2, os.cpu_count() or 1)
            if intra_op_num_threads is None else intra_op_num_threads
        )
        session_options.enable_cpu_mem_arena = False
        session_options.log_severity_level = 4
        self._session = ort.InferenceSession(
            str(self.model_path),
            providers=["CPUExecutionProvider"],
            sess_options=session_options,
        )

        self._cache_options()

    def _cache_options(self) -> None:
        self._min_speech_frames = max(1, self.options.min_speech_duration_ms // 10)
        self._min_silence_frames = max(1, self.options.min_silence_duration_ms // 10)
        self._pad_samples = max(0, self.options.speech_pad_ms * SAMPLE_RATE // 1000)
        self._smooth_window = max(1, int(self.options.smooth_window_frames))
        self._smooth_kernel = (
            np.ones(self._smooth_window, dtype=np.float32) / self._smooth_window
            if self._smooth_window > 1
            else None
        )

    def _extract_features(self, audio: np.ndarray) -> np.ndarray:
        samples = np.asarray(audio, dtype=np.float32).reshape(-1)
        pcm = np.clip(samples * 32768.0, -32768.0, 32767.0)

        fbank = self._knf.OnlineFbank(self._fbank_options)
        fbank.accept_waveform(SAMPLE_RATE, pcm.tolist())
        frame_count = fbank.num_frames_ready
        if frame_count == 0:
            return np.empty((0, 80), dtype=np.float32)

        features = np.asarray(
            [fbank.get_frame(index) for index in range(frame_count)],
            dtype=np.float32,
        )
        features -= self._mean
        features *= self._inverse_std
        return features

    def probabilities(self, audio: np.ndarray) -> np.ndarray:
        features = self._extract_features(audio)
        if features.shape[0] == 0:
            return np.empty(0, dtype=np.float32)
        output = self._session.run(None, {"feat": features[np.newaxis, :, :]})[0]
        return np.asarray(output, dtype=np.float32).reshape(-1)

    def warmup(self) -> None:
        self.probabilities(np.zeros(SAMPLE_RATE, dtype=np.float32))

    def speech_timestamps(self, audio: np.ndarray) -> list[dict[str, int]]:
        samples = np.asarray(audio, dtype=np.float32).reshape(-1)
        probabilities = self.probabilities(samples)
        return self._timestamps_from_probabilities(probabilities, len(samples))

    def _timestamps_from_probabilities(
        self,
        raw_probabilities: np.ndarray,
        audio_length_samples: int,
    ) -> list[dict[str, int]]:
        probabilities = np.asarray(raw_probabilities, dtype=np.float32).reshape(-1)
        if probabilities.size == 0:
            return []

        if not hasattr(self, "_smooth_window"):
            self._cache_options()

        if self._smooth_window > 1:
            smoothed = np.convolve(probabilities, self._smooth_kernel, mode="full")[: probabilities.size]
            for index in range(min(self._smooth_window - 1, probabilities.size)):
                smoothed[index] = probabilities[: index + 1].mean()
        else:
            smoothed = probabilities

        speech_flags = smoothed >= self.options.threshold
        min_speech_frames = self._min_speech_frames
        min_silence_frames = self._min_silence_frames
        pad_samples = self._pad_samples

        segments: list[tuple[int, int]] = []
        candidate_start: int | None = None
        speech_start: int | None = None
        silence_start: int | None = None

        for index, is_speech in enumerate(speech_flags):
            if speech_start is None:
                if is_speech:
                    if candidate_start is None:
                        candidate_start = index
                    if index - candidate_start + 1 >= min_speech_frames:
                        speech_start = candidate_start
                        silence_start = None
                else:
                    candidate_start = None
                continue

            if is_speech:
                silence_start = None
                continue

            if silence_start is None:
                silence_start = index
                continue

            if index - silence_start + 1 >= min_silence_frames:
                segments.append((speech_start, silence_start))
                speech_start = None
                candidate_start = None
                silence_start = None

        if speech_start is not None:
            segments.append((speech_start, probabilities.size))

        padded: list[dict[str, int]] = []
        for start_frame, end_frame in segments:
            start = max(0, start_frame * FRAME_SHIFT_SAMPLES - pad_samples)
            end = min(
                audio_length_samples,
                end_frame * FRAME_SHIFT_SAMPLES + pad_samples,
            )
            if end <= start:
                continue
            if padded and start <= padded[-1]["end"]:
                padded[-1]["end"] = max(padded[-1]["end"], end)
            else:
                padded.append({"start": start, "end": end})
        return padded
