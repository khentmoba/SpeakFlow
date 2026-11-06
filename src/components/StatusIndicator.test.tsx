import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import type { ModelStage } from "@/hooks/useModelStatus";
import StatusIndicator from "./StatusIndicator";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

function renderStatus(stage: ModelStage) {
  const downloadModels = vi.fn();
  const cancelDownload = vi.fn();
  return {
    ...render(<StatusIndicator
      stage={stage}
      isReady={false}
      isStarting={false}
      isRecording={false}
      isProcessing={false}
      device="cpu"
      gpuName={null}
      downloadProgress={0}
      downloadMessage={null}
      isDownloading={false}
      downloadModels={downloadModels}
      cancelDownload={cancelDownload}
    />),
    downloadModels,
    cancelDownload,
  };
}

// The runtime contract is frozen before ModelStage gains this variant.
const NEED_REPAIR = "need_repair" as ModelStage;

describe("StatusIndicator model integrity repair", () => {
  it("offers an enabled, accessible Repair action that starts the supplied repair download", () => {
    const { downloadModels, cancelDownload } = renderStatus(NEED_REPAIR);
    const repair = screen.getByRole("button", { name: /repair/i });

    expect(repair).toBeEnabled();
    fireEvent.click(repair);

    expect(downloadModels).toHaveBeenCalledOnce();
    expect(cancelDownload).not.toHaveBeenCalled();
  });

  it("explains repair without a progress bar, spinner, or loading/preparing wording", () => {
    const { container } = renderStatus(NEED_REPAIR);

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".animate-spin")).toHaveLength(0);
    expect(screen.queryAllByText(/loading|preparing/i)).toHaveLength(0);
    expect(container.querySelector("[aria-live='polite']"))
      .toHaveTextContent(/repair|corrupt|damaged/i);
  });

  it("shows an error instead of an endless preparing spinner", () => {
    const { container } = renderStatus("error");
    expect(screen.getByRole("status")).toHaveTextContent("Not ready · Needs attention");
    expect(container.querySelector(".animate-spin")).not.toBeInTheDocument();
    expect(screen.getByText(/model unavailable/i)).toBeInTheDocument();
    expect(screen.queryByText("CPU")).not.toBeInTheDocument();
  });

  it("keeps the existing missing-model download action", () => {
    const { downloadModels } = renderStatus("need_download");
    fireEvent.click(screen.getByRole("button", { name: /start download/i }));

    expect(downloadModels).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: /repair/i })).not.toBeInTheDocument();
  });
});

describe("StatusIndicator download controls", () => {
  function renderDownloading(overrides: Record<string, unknown> = {}) {
    const pauseDownload = vi.fn();
    const resumeDownload = vi.fn();
    const view = render(
      <StatusIndicator
        stage="downloading"
        isReady={false}
        isStarting={false}
        isRecording={false}
        isProcessing={false}
        device="cpu"
        gpuName={null}
        downloadProgress={42}
        downloadMessage={null}
        isDownloading
        downloadEstimate={{ bytesPerSecond: 5_000_000, secondsRemaining: 30 }}
        downloadModels={vi.fn()}
        cancelDownload={vi.fn()}
        pauseDownload={pauseDownload}
        resumeDownload={resumeDownload}
        {...overrides}
      />,
    );
    return { ...view, pauseDownload, resumeDownload };
  }

  it("shows the measured speed and time remaining", () => {
    renderDownloading();
    expect(screen.getByText(/5\.00 MB\/s/)).toBeInTheDocument();
    expect(screen.getByText(/30 s left/)).toBeInTheDocument();
  });

  it("pauses a running download", () => {
    const { pauseDownload } = renderDownloading();
    fireEvent.click(screen.getByRole("button", { name: /pause/i }));
    expect(pauseDownload).toHaveBeenCalledOnce();
  });

  it("offers Resume and says the download is paused", () => {
    const { resumeDownload } = renderDownloading({
      stage: "need_download",
      isDownloading: false,
      isDownloadPaused: true,
      downloadEstimate: null,
    });
    expect(screen.getByText(/download paused/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /resume/i }));
    expect(resumeDownload).toHaveBeenCalledOnce();
  });
});
