import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AppUpdatePanel from "./AppUpdatePanel";

const openAppReleasePage = vi.fn().mockResolvedValue("opened");
const toast = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
const updateState = {
  updateChecking: false,
  updateInstalling: false,
  updateProgress: null as null | { phase: string; downloaded: number; total: number | null },
  updateStatusText: "",
  latestAvailableVersion: null as string | null,
  latestRelease: null as null | { notes: string | null; url: string | null },
  handleCheckForUpdates: vi.fn(),
  handleInstallUpdate: vi.fn(),
};

vi.mock("@/contexts/AppUpdateContext", () => ({
  useAppUpdates: () => updateState,
}));

vi.mock("@/api/tauri", () => ({
  openAppReleasePage: (...args: unknown[]) => openAppReleasePage(...args),
}));

vi.mock("sonner", () => ({ toast }));

beforeEach(() => {
  Object.assign(updateState, {
    updateChecking: false,
    updateInstalling: false,
    updateProgress: null,
    updateStatusText: "",
    latestAvailableVersion: null,
    latestRelease: null,
    handleCheckForUpdates: vi.fn(),
    handleInstallUpdate: vi.fn(),
  });
  openAppReleasePage.mockReset().mockResolvedValue("opened");
  toast.error.mockReset();
});

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("AppUpdatePanel", () => {
  it("shows the release notes and opens the release page", () => {
    updateState.latestAvailableVersion = "1.2.0";
    updateState.latestRelease = {
      notes: "Faster local models\nBetter hotkey recovery",
      url: "https://github.com/khentmoba/SpeakFlow/releases/tag/v1.2.0",
    };
    render(<AppUpdatePanel appVersion="1.1.8" />);

    expect(screen.getByText(/Faster local models/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /updates.viewRelease/ }));
    expect(openAppReleasePage).toHaveBeenCalledWith(
      "https://github.com/khentmoba/SpeakFlow/releases/tag/v1.2.0",
    );
  });

  it("reports a release page that cannot be opened", async () => {
    updateState.latestAvailableVersion = "1.2.0";
    updateState.latestRelease = { notes: "Notes", url: "https://example.com/release" };
    openAppReleasePage.mockRejectedValue(new Error("blocked"));
    render(<AppUpdatePanel appVersion="1.1.8" />);

    fireEvent.click(screen.getByRole("button", { name: /updates.viewRelease/ }));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("updates.releaseOpenFailed"));
  });

  it("hides the notes block when the release body is empty", () => {
    updateState.latestAvailableVersion = "1.2.0";
    updateState.latestRelease = { notes: "   ", url: null };
    render(<AppUpdatePanel appVersion="1.1.8" />);
    expect(screen.queryByText("updates.releaseNotes")).toBeNull();
  });

  it("shows no notes when the app is up to date", () => {
    updateState.latestAvailableVersion = null;
    updateState.latestRelease = null;
    render(<AppUpdatePanel appVersion="1.1.8" />);
    expect(screen.queryByText("updates.releaseNotes")).toBeNull();
  });
});
