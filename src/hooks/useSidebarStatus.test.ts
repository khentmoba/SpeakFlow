import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import en from "@/i18n/en";
import { resolveSidebarStatus, useSidebarStatus } from "./useSidebarStatus";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.2.0"),
}));

const mockRecordingState = vi.fn(() => ({
  isRecording: false,
  isStarting: false,
  isProcessing: false,
  isReady: true,
  stage: "ready",
  hotkeyDisplay: "F2",
}));

vi.mock("@/contexts/RecordingContext", () => ({
  useRecordingContext: () => mockRecordingState(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("resolveSidebarStatus", () => {
  it("maps recording, connecting, processing, ready, error, and idle states", () => {
    expect(resolveSidebarStatus({ isRecording: true, isStarting: false, isProcessing: false, isReady: true, stage: "ready" })).toEqual({
      state: "recording",
      labelKey: "main.statusListening",
    });
    expect(resolveSidebarStatus({ isRecording: false, isStarting: true, isProcessing: false, isReady: false, stage: "starting" })).toEqual({
      state: "recording",
      labelKey: "main.statusConnecting",
    });
    expect(resolveSidebarStatus({ isRecording: false, isStarting: false, isProcessing: true, isReady: false, stage: "processing" })).toEqual({
      state: "processing",
      labelKey: "main.statusProcessing",
    });
    expect(resolveSidebarStatus({ isRecording: false, isStarting: false, isProcessing: false, isReady: true, stage: "ready" })).toEqual({
      state: "ready",
      labelKey: "main.statusReady",
    });
    expect(resolveSidebarStatus({ isRecording: false, isStarting: false, isProcessing: false, isReady: false, stage: "error" })).toEqual({
      state: "error",
      labelKey: "main.statusError",
    });
    expect(resolveSidebarStatus({ isRecording: false, isStarting: false, isProcessing: false, isReady: false, stage: "loading" })).toEqual({
      state: "idle",
      labelKey: "main.statusPreparing",
    });
  });
});

describe("useSidebarStatus", () => {
  it("resolves live status, hotkey, version, and check-update action", async () => {
    let hookResult: { current: ReturnType<typeof useSidebarStatus> } | undefined;
    await act(async () => {
      const rendered = renderHook(() => useSidebarStatus());
      hookResult = rendered.result;
    });

    expect(hookResult?.current).toEqual({
      version: "1.2.0",
      state: "ready",
      label: "Ready when you are",
      hotkeyDisplay: "F2",
      updateLabel: "Check for Updates",
      statusBarLabel: "Application status",
    });
  });

  it("formats new version message when an update is available", async () => {
    let hookResult: { current: ReturnType<typeof useSidebarStatus> } | undefined;
    await act(async () => {
      const rendered = renderHook(() => useSidebarStatus(true, "1.3.0"));
      hookResult = rendered.result;
    });

    expect(hookResult?.current.updateLabel).toBe("New version v1.3.0 available.");
    expect(en.settings.checkUpdate).toBe("Check for Updates");
    expect(en.settings.newVersionAvailable).toContain("{{version}}");
  });
});
