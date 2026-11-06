import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SupportSettingsSection from "./SupportSettingsSection";

const copyToClipboard = vi.fn();
const getDiagnostics = vi.fn();
const openLogsFolder = vi.fn();

vi.mock("@/api/tauri", () => ({
  copyToClipboard: (...args: unknown[]) => copyToClipboard(...args),
  getDiagnostics: () => getDiagnostics(),
  openLogsFolder: () => openLogsFolder(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, string>) =>
      values?.path ? `${key} ${values.path}` : key,
  }),
}));

describe("Support settings", () => {
  it("copies a redacted diagnostics report", async () => {
    getDiagnostics.mockResolvedValue("SpeakFlow 1.1.8\nengine: qwen3-asr-1.7b");
    copyToClipboard.mockResolvedValue("copied");

    render(<SupportSettingsSection />);
    fireEvent.click(screen.getByRole("button", { name: "settings.copyDiagnostics" }));

    await waitFor(() => expect(copyToClipboard).toHaveBeenCalledWith(
      "SpeakFlow 1.1.8\nengine: qwen3-asr-1.7b",
    ));
    expect(await screen.findByText("settings.diagnosticsCopied")).toBeInTheDocument();
  });

  it("reports a failed diagnostics copy without hiding the log folder action", async () => {
    getDiagnostics.mockRejectedValue(new Error("no log"));
    render(<SupportSettingsSection />);
    fireEvent.click(screen.getByRole("button", { name: "settings.copyDiagnostics" }));

    expect(await screen.findByText("settings.diagnosticsFailed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "settings.openLogsFolder" })).toBeEnabled();
  });

  it("opens the log folder and shows where it went", async () => {
    openLogsFolder.mockResolvedValue("C:\\Users\\me\\AppData\\Local\\SpeakFlow\\logs");
    render(<SupportSettingsSection />);
    fireEvent.click(screen.getByRole("button", { name: "settings.openLogsFolder" }));

    await waitFor(() => expect(openLogsFolder).toHaveBeenCalled());
    expect(await screen.findByText(
      "settings.logsOpened C:\\Users\\me\\AppData\\Local\\SpeakFlow\\logs",
    )).toBeInTheDocument();
  });
});
