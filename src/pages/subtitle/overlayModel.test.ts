import { describe, expect, it } from "vitest";
import {
  isPillOwnedPhase,
  subtitleIndicatorClass,
  type Phase,
} from "@/pages/subtitle/overlayModel";

describe("recording pill ownership", () => {
  it.each<Phase>(["starting", "recording"])("gives %s to the recording pill", (phase) => {
    expect(isPillOwnedPhase(phase)).toBe(true);
  });

  it.each<Phase>(["idle", "processing", "searching", "polishing", "result", "outcome"])(
    "does not claim %s for the pill",
    (phase) => {
      expect(isPillOwnedPhase(phase)).toBe(false);
    },
  );
});

describe("subtitleIndicatorClass", () => {
  it.each<[boolean]>([[false], [true]])(
    "draws no listening indicator in the subtitle (assistant=%s)",
    (isAssistant) => {
      expect(subtitleIndicatorClass("starting", isAssistant)).toBeNull();
      expect(subtitleIndicatorClass("recording", isAssistant)).toBeNull();
    },
  );

  it("keeps the dictation and assistant processing indicators apart", () => {
    expect(subtitleIndicatorClass("processing", false)).toBe("subtitle-dot-processing");
    expect(subtitleIndicatorClass("processing", true)).toBe("subtitle-dot-assistant-processing");
  });

  it("maps searching and polishing to their dots", () => {
    expect(subtitleIndicatorClass("searching", false)).toBe("subtitle-dot-assistant");
    expect(subtitleIndicatorClass("polishing", false)).toBe("subtitle-dot-polishing");
    expect(subtitleIndicatorClass("polishing", true)).toBe("subtitle-dot-assistant");
  });

  it("draws nothing when idle or showing a result", () => {
    expect(subtitleIndicatorClass("idle", false)).toBeNull();
    expect(subtitleIndicatorClass("result", true)).toBeNull();
    expect(subtitleIndicatorClass("outcome", false)).toBeNull();
  });
});
