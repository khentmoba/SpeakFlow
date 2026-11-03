import { describe, expect, it } from "vitest";
import {
  formatAsrEngineDescription,
  getAsrEngineCapability,
} from "@/lib/asrEngineCapabilities";

describe("ASR engine capabilities", () => {
  it.each([
    ["qwen3-asr-0.6b", { execution: "local", final: true, interim: true, downloadSize: "850 MB", vocabulary: true }],
    ["confucius4-r2t2", { execution: "local", final: true, interim: true, downloadSize: "2.48 GB", vocabulary: true }],
    ["qwen3-asr-1.7b", { execution: "local", final: true, interim: true, downloadSize: "2.19 GB", vocabulary: true }],
    ["parakeet-unified-en-0.6b", { execution: "local", final: true, interim: true, downloadSize: "731 MB", vocabulary: false }],
    ["parakeet-tdt-0.6b-v2", { execution: "local", final: true, interim: true, downloadSize: "730 MB", vocabulary: false }],
    ["canary-qwen-2.5b", { execution: "local", final: true, interim: true, downloadSize: "2.80 GB", vocabulary: false }],
    ["cohere-transcribe-03-2026", { execution: "local", final: true, interim: true, downloadSize: "2.41 GB", vocabulary: false }],
    ["glm-asr", { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true }],
    ["alibaba-asr", { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true }],
  ] as const)("%s exposes its product capability contract", (engineKey, expected) => {
    expect(getAsrEngineCapability(engineKey)).toEqual(expected);
  });

  it("shows a download size for every local model and leaves cloud descriptions unchanged", () => {
    expect(formatAsrEngineDescription("qwen3-asr-0.6b", "最快 · 推荐"))
      .toBe("850 MB · 最快 · 推荐");
    expect(formatAsrEngineDescription("confucius4-r2t2", "更高精度"))
      .toBe("2.48 GB · 更高精度");
    expect(formatAsrEngineDescription("glm-asr", "智谱在线语音识别"))
      .toBe("智谱在线语音识别");
  });

  it("does not advertise the retired SenseVoice engine", () => {
    expect(getAsrEngineCapability("sensevoice")).toBeNull();
  });

  it("does not advertise the retired Whisper engine", () => {
    expect(getAsrEngineCapability("whisper")).toBeNull();
  });
});
