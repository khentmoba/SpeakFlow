import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import RecordingPill from "@/pages/RecordingPill";
import "@/i18n";

type Handler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Handler>();
const getRecordingSnapshot = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: Handler) => {
    listeners.set(name, handler);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

vi.mock("@/api/tauri", () => ({
  getRecordingSnapshot: () => getRecordingSnapshot(),
}));

function emit(name: string, payload: unknown) {
  const handler = listeners.get(name);
  if (!handler) throw new Error(`no listener for ${name}`);
  act(() => handler({ payload }));
}

beforeEach(() => {
  listeners.clear();
  getRecordingSnapshot.mockReset();
  getRecordingSnapshot.mockResolvedValue(null);
});

describe("RecordingPill", () => {
  it("starts ready and follows recording-state events", async () => {
    render(<RecordingPill />);
    expect(screen.getByRole("status")).toHaveTextContent("Ready");

    emit("recording-state", { isStarting: true });
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");

    emit("recording-state", { isRecording: true });
    expect(screen.getByRole("status")).toHaveTextContent("Listening");

    emit("recording-state", { isProcessing: true });
    expect(screen.getByRole("status")).toHaveTextContent("Transcribing");

    emit("recording-state", {});
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
  });

  it("renders waveform bars while recording", () => {
    const { container } = render(<RecordingPill />);
    emit("recording-state", { isRecording: true });
    emit("waveform", { bars: [0, 0.5, 1] });
    expect(container.querySelectorAll(".pill-bars i")).toHaveLength(7);
  });

  it("uses the current snapshot when the pill opens mid-recording", async () => {
    getRecordingSnapshot.mockResolvedValue({ phase: "recording" });
    render(<RecordingPill />);
    expect(await screen.findByText("Listening")).toBeInTheDocument();
  });
});
