import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import SetupWizard from "@/components/SetupWizard";
import "@/i18n";

const api = vi.hoisted(() => ({
  listInputDevices: vi.fn(),
  setInputDevice: vi.fn(),
  startMicrophoneLevelMonitor: vi.fn(),
  stopMicrophoneLevelMonitor: vi.fn(),
}));

vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => undefined),
}));

beforeEach(() => {
  vi.clearAllMocks();
  api.listInputDevices.mockResolvedValue({
    devices: [
      { name: "Built-in Microphone", isDefault: true },
      { name: "USB Microphone", isDefault: false },
    ],
    selectedDeviceName: "Built-in Microphone",
  });
  api.setInputDevice.mockResolvedValue(undefined);
  api.startMicrophoneLevelMonitor.mockResolvedValue("started");
  api.stopMicrophoneLevelMonitor.mockResolvedValue(undefined);
});

function setup(overrides: Partial<Parameters<typeof SetupWizard>[0]> = {}) {
  const onSelectEngine = vi.fn().mockResolvedValue(true);
  const onDownload = vi.fn();
  const onRetryModel = vi.fn();
  const onCancelDownload = vi.fn();
  const onOpenSettings = vi.fn();
  const onDismiss = vi.fn();
  render(
    <SetupWizard
      engine="qwen3-asr-0.6b"
      installed={new Set()}
      isToggleMode={false}
      stage="ready"
      isReady={false}
      isDownloading={false}
      downloadProgress={0}
      downloadMessage={null}
      onSelectEngine={onSelectEngine}
      onDownload={onDownload}
      onRetryModel={onRetryModel}
      onCancelDownload={onCancelDownload}
      hotkeyDisplay="F2"
      onOpenSettings={onOpenSettings}
      onDismiss={onDismiss}
      {...overrides}
    />,
  );
  return { onSelectEngine, onDownload, onRetryModel, onCancelDownload, onOpenSettings, onDismiss };
}

describe("SetupWizard", () => {
  it("covers microphone, recognition, hotkey and the first sentence", async () => {
    setup();
    expect(screen.getByText("Set up SpeakFlow")).toBeInTheDocument();
    expect(screen.getByText("Microphone")).toBeInTheDocument();
    expect(screen.getByText("Recognition")).toBeInTheDocument();
    expect(screen.getByText("Hotkey")).toBeInTheDocument();
    expect(screen.getByText("Try it")).toBeInTheDocument();
    expect(screen.getByText(/Parakeet Unified EN 0\.6B/)).toBeInTheDocument();
    await waitFor(() => expect(api.listInputDevices).toHaveBeenCalled());
  });

  it("selects the recommended English model and starts its download", async () => {
    const { onSelectEngine, onDownload } = setup();
    fireEvent.click(screen.getByRole("button", { name: /Download and use/ }));
    expect(onSelectEngine).toHaveBeenCalledWith("parakeet-unified-en-0.6b");
    await waitFor(() => expect(onDownload).toHaveBeenCalledOnce());
  });

  it("downloads the recommended model when it is already selected but missing", async () => {
    const { onDownload } = setup({
      engine: "parakeet-unified-en-0.6b",
      installed: new Set(),
    });
    fireEvent.click(screen.getByRole("button", { name: /Download and use/ }));
    await waitFor(() => expect(onDownload).toHaveBeenCalledOnce());
  });

  it("offers a retry when the selected model failed to start", () => {
    const { onRetryModel } = setup({
      engine: "parakeet-unified-en-0.6b",
      installed: new Set(["parakeet-unified-en-0.6b"]),
      stage: "error",
    });
    const retry = screen.getByRole("button", { name: "Retry" });
    expect(retry).toBeEnabled();
    fireEvent.click(retry);
    expect(onRetryModel).toHaveBeenCalledOnce();
  });

  it("uses an installed recommended model without downloading", async () => {
    const { onSelectEngine, onDownload } = setup({
      installed: new Set(["parakeet-unified-en-0.6b"]),
    });
    fireEvent.click(screen.getByRole("button", { name: /Use this model/ }));
    expect(onSelectEngine).toHaveBeenCalledWith("parakeet-unified-en-0.6b");
    await waitFor(() => expect(onSelectEngine).toHaveBeenCalled());
    expect(onDownload).not.toHaveBeenCalled();
  });

  it("switches microphone from the guide", async () => {
    setup();
    const select = await screen.findByRole("combobox", { name: "Microphone" });
    fireEvent.change(select, { target: { value: "USB Microphone" } });
    await waitFor(() => expect(api.setInputDevice).toHaveBeenCalledWith("USB Microphone"));
    expect(localStorage.getItem("speak-flow-input-device")).toBe("USB Microphone");
  });

  it("shows download progress and can cancel", () => {
    const { onCancelDownload } = setup({
      isDownloading: true,
      downloadProgress: 42,
      engine: "parakeet-unified-en-0.6b",
    });
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "42");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancelDownload).toHaveBeenCalledOnce();
  });

  it("can be skipped or sent to Settings", () => {
    const { onDismiss, onOpenSettings } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Skip and use the default" }));
    expect(onDismiss).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(onOpenSettings).toHaveBeenCalledOnce();
  });
});
