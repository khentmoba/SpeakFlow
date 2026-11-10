import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "@/i18n";
import SidebarFooter from "./SidebarFooter";
import en from "@/i18n/en";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("1.2.0"),
}));

vi.mock("@/contexts/RecordingContext", () => ({
  useRecordingContext: () => ({
    isRecording: false,
    isStarting: false,
    isProcessing: false,
    isReady: true,
    stage: "ready",
    hotkeyDisplay: "F2",
  }),
}));

describe("SidebarFooter", () => {
  it("renders live status and resolves the app version dynamically", async () => {
    await act(async () => {
      render(<SidebarFooter onNavigate={vi.fn()} />);
    });

    expect(screen.getByLabelText("Application status")).toBeInTheDocument();
    expect(screen.getByText("Ready when you are")).toBeInTheDocument();
    expect(screen.getByText("F2")).toBeInTheDocument();
    expect(screen.getByText("v1.2.0")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check for Updates" })).toBeInTheDocument();
  });

  it("renders new version message when an update is available", async () => {
    await act(async () => {
      render(<SidebarFooter onNavigate={vi.fn()} updateAvailable latestVersion="1.3.0" />);
    });

    expect(screen.getByRole("button", { name: "New version v1.3.0 available." })).toBeInTheDocument();
  });

  it("uses valid en i18n strings for update actions", () => {
    expect(en.settings.checkUpdate).toBe("Check for Updates");
    expect(en.settings.newVersionAvailable).toContain("{{version}}");
  });
});
