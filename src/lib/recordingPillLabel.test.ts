import { describe, expect, it } from "vitest";
import { pillLabelKey } from "@/lib/recordingPillLabel";

describe("pillLabelKey", () => {
  it("keeps the plain listening label for dictation", () => {
    expect(pillLabelKey("recording", "dictation")).toBe("pill.recording");
    expect(pillLabelKey("recording", null)).toBe("pill.recording");
    expect(pillLabelKey("recording")).toBe("pill.recording");
  });

  it("says the AI is listening for assistant recordings", () => {
    expect(pillLabelKey("recording", "assistant")).toBe("pill.assistantRecording");
  });

  it("uses the phase label for every other phase, even in assistant mode", () => {
    expect(pillLabelKey("starting", "assistant")).toBe("pill.starting");
    expect(pillLabelKey("processing", "assistant")).toBe("pill.processing");
    expect(pillLabelKey("idle", "assistant")).toBe("pill.idle");
  });
});
