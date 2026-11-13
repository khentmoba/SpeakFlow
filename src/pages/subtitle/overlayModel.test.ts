import { describe, expect, it } from "vitest";
import type { RecordingSnapshot } from "@/api/tauri";
import {
  admitLiveEvent,
  isPillOwnedPhase,
  shouldHydrateSnapshot,
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

describe("live event fence", () => {
  const fence = { latestSessionId: 5, latestRevision: 3 };

  it("rejects older sessions and malformed ids", () => {
    expect(admitLiveEvent(fence, 4, 9)).toBeNull();
    expect(admitLiveEvent(fence, 0, 0)).toBeNull();
    expect(admitLiveEvent(fence, 5.5, 4)).toBeNull();
    expect(admitLiveEvent(fence, 5, -1)).toBeNull();
  });

  it("admits a newer session even with a low revision", () => {
    expect(admitLiveEvent(fence, 6, 0)).toEqual({ sessionId: 6, revision: 0, isNewSession: true });
    expect(admitLiveEvent(fence, 6, undefined)).toEqual({ sessionId: 6, revision: null, isNewSession: true });
  });

  it("orders revisions within the current session", () => {
    expect(admitLiveEvent(fence, 5, 2)).toBeNull();
    expect(admitLiveEvent(fence, 5, 3)).toBeNull();
    expect(admitLiveEvent(fence, 5, 3, true)).toEqual({ sessionId: 5, revision: 3, isNewSession: false });
    expect(admitLiveEvent(fence, 5, 4)).toEqual({ sessionId: 5, revision: 4, isNewSession: false });
  });

  it("blocks unversioned events once a revision is fenced", () => {
    expect(admitLiveEvent(fence, 5, undefined)).toBeNull();
    expect(admitLiveEvent({ latestSessionId: 5, latestRevision: -1 }, 5, undefined))
      .toEqual({ sessionId: 5, revision: null, isNewSession: false });
  });
});

describe("snapshot hydration", () => {
  const snapshot = (sessionId: number, revision = 0): RecordingSnapshot => ({
    sessionId, revision, phase: "recording", mode: "dictation",
  });

  it("hydrates only sessions the overlay has never observed", () => {
    expect(shouldHydrateSnapshot(snapshot(6), 5)).toBe(true);
    expect(shouldHydrateSnapshot(snapshot(5), 5)).toBe(false);
    expect(shouldHydrateSnapshot(null, 0)).toBe(false);
    expect(shouldHydrateSnapshot(snapshot(6, -1), 5)).toBe(false);
  });
});
