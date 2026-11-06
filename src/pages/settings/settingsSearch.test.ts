import { describe, expect, it } from "vitest";
import { settingsSectionMatches } from "@/pages/settings/settingsSearch";

describe("settingsSectionMatches", () => {
  it("matches everything for an empty query", () => {
    expect(settingsSectionMatches("engine", "Engine", "")).toBe(true);
    expect(settingsSectionMatches("engine", "Engine", "   ")).toBe(true);
  });

  it("matches the section label case-insensitively", () => {
    expect(settingsSectionMatches("microphone", "Microphone", "MIC")).toBe(true);
    expect(settingsSectionMatches("hotkey", "Speech Hotkey", "hotkey")).toBe(true);
  });

  it("matches everyday keywords the label does not contain", () => {
    expect(settingsSectionMatches("microphone", "Microphone", "mic")).toBe(true);
    expect(settingsSectionMatches("support", "Support", "logs")).toBe(true);
    expect(settingsSectionMatches("support", "Support", "diagnostics")).toBe(true);
    expect(settingsSectionMatches("startup", "Startup", "tray")).toBe(true);
    expect(settingsSectionMatches("engine", "Engine", "download")).toBe(true);
  });

  it("requires every token of a multi-word query", () => {
    expect(settingsSectionMatches("ai-models", "AI Models", "api key")).toBe(true);
    expect(settingsSectionMatches("ai-models", "AI Models", "api microphone")).toBe(false);
  });

  it("rejects a query nothing covers", () => {
    expect(settingsSectionMatches("engine", "Engine", "zzzz")).toBe(false);
  });
});
