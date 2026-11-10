import { describe, expect, it } from "vitest";

import {
  clampDuckVolume,
  clampKeepAudioDays,
  clampTranscriptFontSize,
  DEFAULT_AUDIO_BEHAVIOR,
  DEFAULT_LABS,
  DEFAULT_MEETINGS,
  formatDuckVolume,
  formatFileSize,
  formatMeetingDuration,
  normalizeHotkey,
  parseListField,
  withAudioBehaviorDefaults,
  withLabsDefaults,
  withMeetingsDefaults,
  withOutputBehaviorDefaults,
  withShortcutsBehaviorDefaults,
} from "@/lib/behaviorConfig";

describe("behavior defaults", () => {
  it("keeps backend defaults when the profile has no value", () => {
    expect(withAudioBehaviorDefaults(null)).toEqual(DEFAULT_AUDIO_BEHAVIOR);
    expect(withLabsDefaults(undefined)).toEqual(DEFAULT_LABS);
    expect(withMeetingsDefaults(null)).toEqual(DEFAULT_MEETINGS);
    expect(withOutputBehaviorDefaults(null).copyOnly).toBe(false);
    expect(withShortcutsBehaviorDefaults(null).shareDictationShortcut).toBe(false);
  });

  it("lets a stored value win over the default", () => {
    expect(withAudioBehaviorDefaults({ duckingMode: "mute" }).duckingMode).toBe("mute");
    expect(withLabsDefaults({ failureNotices: false }).failureNotices).toBe(false);
    expect(withMeetingsDefaults({ keepAudioDays: 7 }).keepAudioDays).toBe(7);
  });

  it("does not let an explicit false collapse into the default", () => {
    expect(withLabsDefaults({ outputFixes: false }).outputFixes).toBe(false);
    expect(withMeetingsDefaults({ autoSummarize: false }).autoSummarize).toBe(false);
  });
});

describe("clamps", () => {
  it("keeps duck volume inside 0..1", () => {
    expect(clampDuckVolume(-1)).toBe(0);
    expect(clampDuckVolume(2)).toBe(1);
    expect(clampDuckVolume(Number.NaN)).toBe(DEFAULT_AUDIO_BEHAVIOR.duckVolume);
  });

  it("keeps retention days and font size inside the backend range", () => {
    expect(clampKeepAudioDays(-5)).toBe(0);
    expect(clampKeepAudioDays(9999)).toBe(3650);
    expect(clampTranscriptFontSize(2)).toBe(10);
    expect(clampTranscriptFontSize(99)).toBe(24);
  });

  it("formats the duck volume as a percentage", () => {
    expect(formatDuckVolume(0.2)).toBe("20%");
    expect(formatDuckVolume(1.4)).toBe("100%");
  });
});

describe("hotkeys", () => {
  it("treats blank shortcut fields as disabled", () => {
    expect(normalizeHotkey("")).toBeNull();
    expect(normalizeHotkey("   ")).toBeNull();
    expect(normalizeHotkey(null)).toBeNull();
    expect(normalizeHotkey(undefined)).toBeNull();
  });

  it("keeps a real accelerator", () => {
    expect(normalizeHotkey("  Ctrl+Shift+D ")).toBe("Ctrl+Shift+D");
  });
});

describe("formatting", () => {
  it("formats meeting durations", () => {
    expect(formatMeetingDuration(12.4)).toBe("12s");
    expect(formatMeetingDuration(95)).toBe("1:35");
    expect(formatMeetingDuration(3725)).toBe("1:02:05");
    expect(formatMeetingDuration(-3)).toBe("0s");
  });

  it("formats model file sizes", () => {
    expect(formatFileSize(0)).toBe("0 B");
    expect(formatFileSize(512)).toBe("512 B");
    expect(formatFileSize(1536)).toBe("1.5 KB");
    expect(formatFileSize(2 * 1024 * 1024 * 1024)).toBe("2 GB");
  });
});

describe("list fields", () => {
  it("splits, trims, and drops empty entries", () => {
    expect(parseListField(" Tauri , , SpeakFlow ")).toEqual(["Tauri", "SpeakFlow"]);
    expect(parseListField("")).toEqual([]);
  });
});
