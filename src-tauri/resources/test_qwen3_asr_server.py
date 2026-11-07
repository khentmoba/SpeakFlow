import base64
import os
import tempfile
import subprocess
import sys
import types
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server


class FakeSession:
    def __init__(self):
        self.calls = 0
        self.inputs = []
        self.options = []

    def run(self, audio, **kwargs):
        self.calls += 1
        self.inputs.append(np.array(audio, copy=True))
        self.options.append(kwargs)
        return types.SimpleNamespace(text="测试文本", language="zh")

    def close(self):
        pass


class FakeModel:
    instances = []

    def __init__(self, path, backend):
        self.path = path
        self.backend = backend
        self.session_instance = FakeSession()
        self.session_calls = 0
        self.session_options = None
        self.__class__.instances.append(self)

    def session(self, **kwargs):
        self.session_calls += 1
        self.session_options = kwargs
        return self.session_instance

    def close(self):
        pass


class FakeVad:
    def __init__(self, chunks):
        self.chunks = chunks

    def warmup(self):
        pass

    def speech_timestamps(self, _audio):
        return self.chunks


class Qwen3ASRServerTests(unittest.TestCase):
    def setUp(self):
        FakeModel.instances.clear()
        # Model and VAD tests must not query GPU metadata from the host machine.
        gpu_info_patcher = mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer,
            "_get_gpu_device_info",
            return_value={
                "device": "cuda",
                "gpu_name": "Test GPU",
                "gpu_memory_total": 24.0,
            },
        )
        gpu_info_patcher.start()
        self.gpu_info_patcher = gpu_info_patcher
        self.addCleanup(gpu_info_patcher.stop)

    def test_vocabulary_conditions_recognition_without_leaking_between_requests(self):
        for engine in qwen3_asr_server.TRANSCRIBE_CPP_ASR_MODELS:
            supported = engine.startswith("qwen3-asr-")
            with (
                self.subTest(engine=engine),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"),
            ):
                server = qwen3_asr_server.Qwen3ASRServer(engine=engine)
                server.initialized = True
                server.model = mock.Mock()
                server.model.supports.return_value = supported
                server.session = FakeSession()
                server.vad_model = FakeVad([{"start": 0, "end": 16_000}])
                payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
                for words in (["Khent", "SpeakFlow"], ["Qwen"], []):
                    result = server.transcribe_audio(
                        None, hot_words=words, audio_base64=payload,
                        audio_format="pcm_s16le", sample_rate=16_000,
                    )
                    self.assertTrue(result["success"], result)
                expected = [
                    {"timestamps": "none", "vocabulary": ["Khent", "SpeakFlow"]},
                    {"timestamps": "none", "vocabulary": ["Qwen"]},
                    {"timestamps": "none"},
                ] if supported else [{"timestamps": "none"}] * 3
                self.assertEqual(server.session.options, expected)

    def test_language_and_translation_reach_the_model(self):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"):
            server = qwen3_asr_server.Qwen3ASRServer(engine="whisper-large-v3")
            server.initialized = True
            server.model = mock.Mock()
            server.model.supports.return_value = False
            server.session = FakeSession()
            server.vad_model = FakeVad([{"start": 0, "end": 16_000}])
            payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
            result = server.transcribe_audio(
                None, hot_words=["SpeakFlow"], audio_base64=payload,
                audio_format="pcm_s16le", sample_rate=16_000,
                language="ca", translate_to_english=True,
            )
        self.assertTrue(result["success"], result)
        self.assertEqual(server.session.options, [{
            "timestamps": "none",
            "language": "ca",
            "task": "translate",
            "target_language": "en",
        }])
        # A translated transcript is English text, whatever the model detected.
        self.assertEqual(result["language"], "en")

    def test_translation_is_dropped_when_the_runtime_does_not_support_it(self):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"):
            server = qwen3_asr_server.Qwen3ASRServer(engine="whisper-large-v3")
            server.initialized = True
            server.model = mock.Mock()
            server.model.capabilities.supports_translate = False
            server.model.supports.return_value = False
            server.session = FakeSession()
            server.vad_model = FakeVad([{"start": 0, "end": 16_000}])
            payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
            result = server.transcribe_audio(
                None, audio_base64=payload, audio_format="pcm_s16le",
                sample_rate=16_000, language="ca", translate_to_english=True,
            )
        self.assertTrue(result["success"], result)
        self.assertNotIn("task", server.session.options[0])
        self.assertEqual(server.session.options[0]["language"], "ca")

    def test_translation_drops_vocabulary_but_plain_runs_keep_it(self):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            server.initialized = True
            server.model = mock.Mock()
            server.model.supports.return_value = True
            server.session = FakeSession()
            server.vad_model = FakeVad([{"start": 0, "end": 16_000}])
            payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
            server.transcribe_audio(
                None, hot_words=["SpeakFlow"], audio_base64=payload,
                audio_format="pcm_s16le", sample_rate=16_000,
                language="fr", translate_to_english=True,
            )
            server.transcribe_audio(
                None, hot_words=["SpeakFlow"], audio_base64=payload,
                audio_format="pcm_s16le", sample_rate=16_000,
                language="fr", translate_to_english=False,
            )
        self.assertNotIn("vocabulary", server.session.options[0])
        self.assertEqual(server.session.options[1]["vocabulary"], ["SpeakFlow"])

    def test_backend_preferences_are_scoped_to_the_selected_model(self):
        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
            mock.patch.dict(os.environ, {
                "SPEAK_FLOW_ASR_BACKEND": "cuda",
                "SPEAK_FLOW_ASR_BACKEND_QWEN3_ASR_1_7B": " Vulkan ",
            }),
        ):
            large = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
            small = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertEqual(large._preferred_backends(), ["vulkan", "cuda", "cpu"])
            self.assertEqual(small._preferred_backends(), ["cuda", "vulkan", "cpu"])

    def test_auto_preference_preserves_device_fallbacks(self):
        for device, expected in [("cuda", ["cuda", "vulkan", "cpu"]), ("cpu", ["auto", "cpu"])]:
            with (
                self.subTest(device=device),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value=device),
                mock.patch.dict(os.environ, {"SPEAK_FLOW_ASR_BACKEND": "auto"}),
            ):
                server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
                self.assertEqual(server._preferred_backends(), expected)

    def test_failed_preferred_session_closes_model_before_fallback(self):
        failed_model = mock.Mock()
        failed_model.session.side_effect = RuntimeError("session allocation failed")
        working_model = FakeModel("model.gguf", "vulkan")

        def load_model(_path, backend):
            if backend == "cuda":
                return failed_model
            failed_model.close.assert_called_once()
            return working_model

        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
            mock.patch.dict(os.environ, {"SPEAK_FLOW_ASR_BACKEND_QWEN3_ASR_1_7B": "auto"}),
            mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=load_model)}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
            server._load_runtime("model.gguf")
        self.assertEqual(server.backend, "vulkan")
        self.assertIs(server.model, working_model)
        self.assertIs(server.session, working_model.session_instance)

    def test_invalid_backend_preference_does_not_load_a_model(self):
        with (
            mock.patch.dict(os.environ, {"SPEAK_FLOW_ASR_BACKEND_QWEN3_ASR_1_7B": "typo"}),
            mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=mock.Mock())}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
            with self.assertRaisesRegex(ValueError, "Unsupported ASR backend preference"):
                server._load_runtime("model.gguf")
            sys.modules["transcribe_cpp"].Model.assert_not_called()

    def test_gpu_metadata_failure_does_not_discard_loaded_runtime(self):
        self.gpu_info_patcher.stop()
        for error in (FileNotFoundError("nvidia-smi"), subprocess.TimeoutExpired("nvidia-smi", 3)):
            with (
                self.subTest(error=type(error).__name__),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
                mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
                mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=FakeModel)}),
                mock.patch.object(qwen3_asr_server.subprocess, "run", side_effect=error) as query,
            ):
                server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
                self.assertTrue(server.initialize()["success"])
                self.assertTrue(server.initialized)
                self.assertIsNotNone(server.model)
                self.assertIsNotNone(server.session)
                self.assertGreater(query.call_args.kwargs["timeout"], 0)
                self.assertLessEqual(query.call_args.kwargs["timeout"], 3)

    def test_failed_initialization_resets_state_and_can_retry(self):
        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
            mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=FakeModel)}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            with mock.patch.object(server, "_get_gpu_device_info", side_effect=RuntimeError("metadata failure")):
                self.assertFalse(server.initialize()["success"])
            self.assertFalse(server.initialized)
            self.assertIsNone(server.model)
            self.assertIsNone(server.session)
            self.assertIsNone(server.vad_model)
            self.assertTrue(server.initialize()["success"])
            self.assertIsNotNone(server.model)
            self.assertIsNotNone(server.session)

    def test_reuses_one_model_and_session_for_inline_pcm_requests(self):
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
            ),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer,
                "_resolve_model_path",
                return_value="model.gguf",
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad([{"start": 0, "end": 16_000}]),
            ),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.zeros(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            first = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )
            second = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        self.assertEqual((first["text"], second["text"]), ("测试文本", "测试文本"))
        self.assertEqual(len(FakeModel.instances), 1)
        self.assertEqual(FakeModel.instances[0].session_calls, 1)
        self.assertEqual(
            FakeModel.instances[0].session_options,
            {"kv_type": "f16", "n_ctx": 32_768},
        )
        self.assertEqual(FakeModel.instances[0].session_instance.calls, 2)
        self.assertEqual(first["input_mode"], "memory")
        self.assertEqual(first["engine"], "qwen3-asr-0.6b")

    def test_vad_rejects_silence_without_running_qwen(self):
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
            ),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer,
                "_resolve_model_path",
                return_value="model.gguf",
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad([]),
            ),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.zeros(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            result = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        self.assertEqual(result["text"], "")
        self.assertEqual(result["speech_duration"], 0.0)
        self.assertEqual(result["vad_segments"], 0)
        self.assertEqual(result["inference_ms"], 0.0)
        self.assertEqual(FakeModel.instances[0].session_instance.calls, 0)
        stats = server.get_performance_stats()
        self.assertEqual(stats["vad_rejected"], 1)
        self.assertTrue(stats["models_loaded"]["vad"])

    def test_vad_trims_only_outer_silence_before_qwen(self):
        fake_module = types.SimpleNamespace(Model=FakeModel)
        chunks = [
            {"start": 1_600, "end": 6_400},
            {"start": 9_600, "end": 14_400},
        ]
        with (
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
            ),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer,
                "_resolve_model_path",
                return_value="model.gguf",
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad(chunks),
            ),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.arange(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            result = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        sent = FakeModel.instances[0].session_instance.inputs[0]
        self.assertEqual(len(sent), 12_800)
        self.assertAlmostEqual(float(sent[0]), 1_600 / 32768.0)
        self.assertAlmostEqual(float(sent[-1]), 14_399 / 32768.0)
        self.assertEqual(result["vad_segments"], 2)
        self.assertEqual(result["speech_duration"], 0.8)
        self.assertTrue(server.check_status()["models"]["vad"])

    def test_idle_resume_reuses_vad_while_reloading_native_runtime(self):
        with (
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
            ),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer,
                "_resolve_model_path",
                return_value="model.gguf",
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(
                qwen3_asr_server, "FireRedVad", side_effect=lambda: FakeVad([])
            ) as vad_factory,
            mock.patch.dict(
                sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=FakeModel)}
            ),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertTrue(server.initialize()["success"])
            vad = server.vad_model
            self.assertIsNotNone(vad)
            vad_factory.assert_called_once_with()

            for cycle in range(2):
                model = server.model
                session = server.session
                self.assertTrue(server._suspend_gpu_runtime())
                self.assertIs(server.vad_model, vad)
                self.assertIsNone(server.model)
                self.assertIsNone(server.session)
                self.assertFalse(server.initialized)
                self.assertTrue(server._gpu_suspended)

                self.assertTrue(server.initialize()["success"])
                self.assertTrue(server.initialized)
                self.assertFalse(server._gpu_suspended)
                self.assertIsNot(server.model, model)
                self.assertIsNot(server.session, session)
                self.assertEqual(len(FakeModel.instances), cycle + 2)
                vad_factory.assert_called_once_with()
                self.assertIs(server.vad_model, vad)

    def test_idle_unload_closes_runtime_and_keeps_vad(self):
        from gpu_idle import status_keeps_process_ready

        with mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
        model = mock.Mock()
        session = mock.Mock()
        vad = object()
        server.model = model
        server.session = session
        server.vad_model = vad
        server.initialized = True

        self.assertTrue(server._suspend_gpu_runtime())
        model.close.assert_called_once()
        session.close.assert_called_once()
        self.assertIsNone(server.model)
        self.assertIsNone(server.session)
        self.assertIs(server.vad_model, vad)
        self.assertFalse(server.initialized)
        self.assertTrue(server._gpu_suspended)
        self.assertFalse(server._suspend_gpu_runtime())

        with mock.patch("qwen3_asr_server.importlib.metadata.version", return_value="9"):
            status = server.check_status()
        self.assertTrue(status["success"])
        self.assertFalse(status["model_loaded"])
        self.assertFalse(status["initialized"])
        self.assertTrue(status["models"]["vad"])
        self.assertFalse(status["models"]["asr"])
        self.assertTrue(status["gpu_suspended"])
        self.assertTrue(status_keeps_process_ready(True, status))

        with (
            mock.patch.object(server, "initialize", return_value={"success": False, "error": "reload"}) as initialize,
        ):
            result = server.transcribe_audio("clip.wav")
        initialize.assert_called_once()
        self.assertEqual(result["error"], "reload")


class TranscribeCppEngineTests(unittest.TestCase):
    def test_parakeet_engine_loads_through_transcribe_cpp_with_its_display_name(self):
        FakeModel.instances = []
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="parakeet-unified-en-0.6b")
            result = server.initialize()

        self.assertEqual(server.display_name, "Parakeet Unified EN 0.6B")
        self.assertTrue(result["success"])
        self.assertTrue(result["message"].startswith("Parakeet Unified EN 0.6B initialized successfully in "))
        self.assertTrue(result["message"].endswith(" seconds"))
        self.assertEqual(FakeModel.instances[0].path, "model.gguf")

    def test_missing_parakeet_model_names_the_engine_and_file(self):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value=None):
            server = qwen3_asr_server.Qwen3ASRServer(engine="parakeet-tdt-0.6b-v2")
            result = server.initialize()

        self.assertFalse(result["success"])
        self.assertEqual(
            result["error"],
            "Parakeet TDT 0.6B v2 Q8 model not downloaded: parakeet-tdt-0.6b-v2-Q8_0.gguf",
        )
        self.assertEqual(result["type"], "models_not_downloaded")

    def test_canary_qwen_engine_loads_through_transcribe_cpp_with_its_display_name(self):
        FakeModel.instances = []
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="canary-qwen-2.5b")
            result = server.initialize()

        self.assertEqual(server.display_name, "Canary-Qwen 2.5B")
        self.assertTrue(result["success"])
        self.assertTrue(result["message"].startswith("Canary-Qwen 2.5B initialized successfully in "))
        self.assertEqual(FakeModel.instances[0].path, "model.gguf")

    def test_missing_canary_model_names_the_engine_and_file(self):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value=None):
            server = qwen3_asr_server.Qwen3ASRServer(engine="canary-qwen-2.5b")
            result = server.initialize()

        self.assertFalse(result["success"])
        self.assertEqual(
            result["error"],
            "Canary-Qwen 2.5B Q8 model not downloaded: canary-qwen-2.5b-Q8_0.gguf",
        )
        self.assertEqual(result["type"], "models_not_downloaded")

    def test_unknown_engine_is_rejected_by_the_shared_server(self):
        with self.assertRaisesRegex(ValueError, "Unsupported transcribe.cpp ASR engine: whisper"):
            qwen3_asr_server.Qwen3ASRServer(engine="whisper")


class LongClipTests(unittest.TestCase):
    """Models with an input ceiling get split instead of failing the request."""

    def _server(self, engine="moonshine-streaming-tiny"):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"):
            server = qwen3_asr_server.Qwen3ASRServer(engine=engine)
        server.model = mock.Mock()
        server.model.supports.return_value = False
        server.session = FakeSession()
        server.vad_model = FakeVad([{"start": 0, "end": 10**9}])
        server.initialized = True
        return server

    def test_short_clip_is_sent_once(self):
        audio = np.zeros(16_000 * 5, dtype=np.float32)
        self.assertEqual(len(qwen3_asr_server.Qwen3ASRServer._split_audio(audio, 16_000 * 10)), 1)
        self.assertEqual(len(qwen3_asr_server.Qwen3ASRServer._split_audio(audio, 0)), 1)

    def test_long_clip_is_cut_into_complete_pieces(self):
        audio = np.zeros(16_000 * 25, dtype=np.float32)
        pieces = qwen3_asr_server.Qwen3ASRServer._split_audio(audio, 16_000 * 10)
        self.assertEqual(len(pieces), 3)
        self.assertTrue(all(len(piece) <= 16_000 * 10 for piece in pieces))
        self.assertEqual(sum(len(piece) for piece in pieces), len(audio))

    def test_cut_lands_on_the_quietest_frame_near_the_limit(self):
        limit = 16_000 * 10
        audio = np.full(16_000 * 25, 0.4, dtype=np.float32)
        # A pause just before the limit should win over the loud audio after it.
        audio[limit - 8000:limit - 4000] = 0.0
        pieces = qwen3_asr_server.Qwen3ASRServer._split_audio(audio, limit)
        self.assertLessEqual(len(pieces[0]), limit)
        self.assertLess(abs(len(pieces[0]) - (limit - 6000)), 4000)

    def test_pieces_join_without_spaces_between_unspaced_scripts(self):
        join = qwen3_asr_server.Qwen3ASRServer._join_pieces
        self.assertEqual(join(["Hello", "world"]), "Hello world")
        self.assertEqual(join(["你好", "世界"]), "你好世界")
        self.assertEqual(join(["こんにちは", "世界"]), "こんにちは世界")
        self.assertEqual(join(["Hello", "世界"]), "Hello 世界")
        self.assertEqual(join(["", "  "]), "")

    def test_session_options_follow_the_catalog_kv_pin(self):
        pinned = self._server("qwen3-asr-0.6b")
        self.assertEqual(pinned._session_options(), {"kv_type": "f16", "n_ctx": 32_768})
        default = self._server("moonshine-tiny")
        self.assertEqual(default._session_options(), {})

    def test_long_clip_transcribes_every_piece(self):
        server = self._server()
        server.session.limits = types.SimpleNamespace(effective_max_audio_ms=10_000)
        payload = base64.b64encode(np.zeros(16_000 * 25, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(server.session.calls, 3)
        self.assertEqual(result["text"], "测试文本测试文本测试文本")

    def test_truncated_decode_keeps_the_partial_transcript(self):
        class OutputTruncated(Exception):
            pass

        class RepeatingSession(FakeSession):
            def run(self, audio, **kwargs):
                self.calls += 1
                if self.calls == 2:
                    error = OutputTruncated("output repetition")
                    error.partial_result = types.SimpleNamespace(text="partial tail", language="en")
                    raise error
                return types.SimpleNamespace(text="first piece", language="en")

        server = self._server()
        server.session = RepeatingSession()
        server.session.limits = types.SimpleNamespace(effective_max_audio_ms=10_000)
        server._truncated_errors = (OutputTruncated,)
        payload = base64.b64encode(np.zeros(16_000 * 25, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(result["text"], "first piece partial tail first piece")

    def test_empty_partial_transcript_still_fails(self):
        class OutputTruncated(Exception):
            pass

        class EmptySession(FakeSession):
            def run(self, audio, **kwargs):
                error = OutputTruncated("output repetition")
                error.partial_result = types.SimpleNamespace(text="   ", language="en")
                raise error

        server = self._server()
        server.session = EmptySession()
        server.session.limits = types.SimpleNamespace(effective_max_audio_ms=0)
        server._truncated_errors = (OutputTruncated,)
        payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertFalse(result["success"])
        self.assertEqual(result["type"], "transcription_error")

    def test_input_ceiling_of_zero_sends_the_whole_clip(self):
        server = self._server("moonshine-tiny")
        server.session.limits = types.SimpleNamespace(effective_max_audio_ms=0)
        payload = base64.b64encode(np.zeros(16_000 * 45, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(server.session.calls, 1)


class ModelFolderUpdateTests(unittest.TestCase):
    def test_a_running_engine_takes_the_new_folder_list(self):
        import server_common

        with mock.patch.dict(os.environ, {"SPEAK_FLOW_MODEL_DIRS": ""}):
            server = object.__new__(server_common.BaseASRServer)
            server.logger = mock.Mock()
            result = server._handle_set_model_dirs(
                {"dirs": ["  D:\hex\models  ", "", "E:\models"]}
            )
            self.assertTrue(result["success"], result)
            self.assertEqual(
                os.environ["SPEAK_FLOW_MODEL_DIRS"],
                os.pathsep.join(["D:\hex\models", "E:\models"]),
            )
            self.assertFalse(server._handle_set_model_dirs({"dirs": "nope"})["success"])


class TranscribeDispatchContractTests(unittest.TestCase):
    def test_every_server_accepts_the_kwargs_the_dispatcher_sends(self):
        import inspect

        import r2t2_asr_server
        import server_common

        base_params = list(inspect.signature(server_common.BaseASRServer.transcribe_audio).parameters)
        for server_class in (qwen3_asr_server.Qwen3ASRServer, r2t2_asr_server.R2T2ASRServer):
            sig = inspect.signature(server_class.transcribe_audio)
            self.assertEqual(
                list(sig.parameters),
                base_params,
                f"{server_class.__name__}.transcribe_audio parameters drifted from BaseASRServer",
            )
            bound_calls = []

            class Probe:
                def transcribe_audio(self_probe, *args, **kwargs):
                    bound_calls.append(sig.bind(None, *args, **kwargs))
                    return {"success": True}

            server_common.BaseASRServer._execute_action(
                Probe(),
                {"action": "transcribe", "audio_path": "clip.wav"},
                "transcribe",
            )
            self.assertEqual(len(bound_calls), 1)


class SpeechProcessingTests(unittest.TestCase):
    """Silence filtering, shared model folders and CPU recovery."""

    def _server(self, engine="moonshine-tiny"):
        with mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cpu"):
            server = qwen3_asr_server.Qwen3ASRServer(engine=engine)
        server.model = mock.Mock()
        server.model.supports.return_value = False
        server.session = FakeSession()
        server.vad_model = FakeVad([{"start": 0, "end": 16_000}])
        server.initialized = True
        return server

    def test_silence_filter_off_sends_every_sample(self):
        server = self._server()
        payload = base64.b64encode(np.zeros(16_000 * 3, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
            filter_silence=False,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(result["vad_segments"], 1)
        self.assertEqual(len(server.session.inputs[0]), 16_000 * 3)

    def test_silence_filter_on_trims_to_speech(self):
        server = self._server()
        server.vad_model = FakeVad([{"start": 1600, "end": 4800}])
        payload = base64.b64encode(np.zeros(16_000 * 3, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(len(server.session.inputs[0]), 3200)

    def test_silence_filter_off_still_rejects_a_clip_without_speech(self):
        # The detector is the only "is there speech?" check, so it keeps running
        # even when its trimming is switched off.
        server = self._server()
        server.vad_model = FakeVad([])
        payload = base64.b64encode(np.zeros(16_000 * 3, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
            filter_silence=False,
        )
        self.assertTrue(result["success"], result)
        self.assertEqual(result["text"], "")
        self.assertEqual(result["vad_segments"], 0)
        self.assertEqual(server.session.calls, 0)

    def test_gpu_decode_failure_retries_once_on_cpu(self):
        server = self._server()
        server.backend = "cuda"
        server.device = "cuda"
        server.model_path = "model.gguf"
        calls = {"run": 0}

        class FlakySession(FakeSession):
            def run(self, audio, **kwargs):
                calls["run"] += 1
                if calls["run"] == 1:
                    raise RuntimeError("device lost")
                return types.SimpleNamespace(text="recovered", language="en")

        server.session = FlakySession()
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
            # Recovery resolves the file again, so the folder can have changed.
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"
            ),
        ):
            payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
            result = server.transcribe_audio(
                None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
            )
        self.assertTrue(result["success"], result)
        # The retry runs on the freshly loaded CPU session.
        self.assertEqual(result["text"], "测试文本")
        self.assertEqual(result["backend"], "cpu")
        self.assertEqual(calls["run"], 1)
        self.assertEqual(FakeModel.instances[-1].backend, "cpu")
        self.assertTrue(server._cpu_recovery_used)

    def test_cpu_failure_is_not_retried(self):
        server = self._server()
        server.backend = "cpu"

        class BrokenSession(FakeSession):
            def run(self, audio, **kwargs):
                raise RuntimeError("decode failed")

        server.session = BrokenSession()
        payload = base64.b64encode(np.zeros(16_000, dtype="<i2").tobytes()).decode("ascii")
        result = server.transcribe_audio(
            None, audio_base64=payload, audio_format="pcm_s16le", sample_rate=16_000,
        )
        self.assertFalse(result["success"])
        self.assertEqual(result["type"], "transcription_error")

    def test_shared_model_folder_matches_the_catalog_name_and_size(self):
        import hf_cache_utils

        catalog = {"models": [{"filename": "model-Q8_0.gguf", "size": 8}]}
        with mock.patch.object(hf_cache_utils, "ASR_CATALOG", catalog):
            with mock.patch.dict(os.environ, {"SPEAK_FLOW_MODEL_DIRS": ""}):
                self.assertIsNone(hf_cache_utils.find_shared_model_file("model-Q8_0.gguf"))
            with tempfile.TemporaryDirectory() as directory:
                shared = os.path.join(directory, "model-Q8_0.gguf")
                with open(shared, "wb") as handle:
                    handle.write(b"0" * 8)
                with mock.patch.dict(os.environ, {"SPEAK_FLOW_MODEL_DIRS": directory}):
                    self.assertEqual(hf_cache_utils.find_shared_model_file("model-Q8_0.gguf"), shared)
                    # A name outside the catalog is never loaded from a folder.
                    self.assertIsNone(hf_cache_utils.find_shared_model_file("other.gguf"))
                # A size mismatch is not the pinned model.
                with open(shared, "wb") as handle:
                    handle.write(b"0" * 9)
                with mock.patch.dict(os.environ, {"SPEAK_FLOW_MODEL_DIRS": directory}):
                    self.assertIsNone(hf_cache_utils.find_shared_model_file("model-Q8_0.gguf"))


if __name__ == "__main__":
    unittest.main()
