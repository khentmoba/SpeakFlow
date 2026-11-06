import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import MainPage from "../MainPage";

const api = vi.hoisted(() => ({
  getEngine: vi.fn(), setEngine: vi.fn(),
  getOnlineAsrApiKey: vi.fn(), getOnlineAsrEndpoint: vi.fn(), getAlibabaAsrConfig: vi.fn(),
  listAlibabaAsrModels: vi.fn(), setAlibabaAsrModel: vi.fn(), listInstalledModels: vi.fn(),
  setOnlineAsrApiKey: vi.fn(), setOnlineAsrEndpoint: vi.fn(),
}));
const recording = vi.hoisted(() => ({
  isStarting: false, isRecording: false, isProcessing: false, isReady: true,
  stage: "ready", device: "cuda", gpuName: "NVIDIA GeForce RTX 4070",
  history: [], transcriptionResult: null, recordingMode: "dictation",
  downloadProgress: 0, downloadMessage: null, isDownloading: false,
  retryModel: vi.fn(), startRecording: vi.fn(), stopRecording: vi.fn(),
}));
const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("@/api/tauri", () => api);
vi.mock("@/contexts/RecordingContext", () => ({ useRecordingContext: () => recording }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ minimize: vi.fn() }) }));
vi.mock("sonner", () => ({ toast }));

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  Object.assign(recording, { isStarting: false, isRecording: false, isProcessing: false, isDownloading: false, isReady: true, stage: "ready" });
  api.getEngine.mockResolvedValue("qwen3-asr-0.6b");
  api.setEngine.mockResolvedValue("glm-asr");
  api.getOnlineAsrApiKey.mockResolvedValue("");
  api.getOnlineAsrEndpoint.mockResolvedValue({ region: "international", url: "" });
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "qwen3-asr-flash", models: ["qwen3-asr-flash", "qwen3-asr-flash-realtime"] });
  api.setAlibabaAsrModel.mockResolvedValue(undefined);
  api.listInstalledModels.mockResolvedValue([]);
});

async function setup() {
  const view = render(<MainPage onNavigate={vi.fn()} />);
  const selector = screen.getByRole("combobox", { name: "Dictation model" });
  await waitFor(() => expect(selector).toBeEnabled());
  return { ...view, selector };
}

describe("Dictation model selector", () => {
  it("loads the saved engine and displays actual readiness and hardware", async () => {
    api.getEngine.mockResolvedValue("confucius4-r2t2");
    const { selector } = await setup();
    expect(selector).toHaveValue("confucius4-r2t2");
    expect(screen.getByRole("status")).toHaveTextContent("Active · Ready to dictate");
    expect(screen.getByText("NVIDIA GeForce RTX 4070")).toBeInTheDocument();
  });

  it("persists an engine switch, blocks recording while pending, then rechecks readiness", async () => {
    let finish!: (engine: string) => void;
    api.setEngine.mockReturnValue(new Promise<string>((resolve) => { finish = resolve; }));
    const { selector } = await setup();
    fireEvent.change(selector, { target: { value: "glm-asr" } });
    await waitFor(() => expect(api.setEngine).toHaveBeenCalledWith("glm-asr"));
    expect(selector).toBeDisabled();
    expect(screen.getByRole("status")).not.toHaveTextContent("Ready to dictate");
    expect(screen.getByRole("button", { name: /start recording/i })).toBeDisabled();
    await act(async () => finish("glm-asr"));
    await waitFor(() => expect(selector).toHaveValue("glm-asr"));
    expect(recording.retryModel).toHaveBeenCalledOnce();
  });

  it("keeps the previous selection and reports a failed switch", async () => {
    api.setEngine.mockRejectedValue(new Error("Failed to save"));
    const { selector } = await setup();
    fireEvent.change(selector, { target: { value: "glm-asr" } });
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(selector).toHaveValue("qwen3-asr-0.6b");
    expect(recording.retryModel).not.toHaveBeenCalled();
  });

  it.each(["isStarting", "isRecording", "isProcessing", "isDownloading"] as const)("prevents model changes during %s", async (flag) => {
    const { selector, rerender } = await setup();
    recording[flag] = true;
    rerender(<MainPage onNavigate={vi.fn()} />);
    expect(selector).toBeDisabled();
  });

  it("offers the configured Alibaba model catalog and saves the selection", async () => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    await setup();
    const model = screen.getByRole("combobox", { name: "Cloud model" });
    fireEvent.change(model, { target: { value: "qwen3-asr-flash-realtime" } });
    await waitFor(() => expect(api.setAlibabaAsrModel).toHaveBeenCalledWith("qwen3-asr-flash-realtime"));
    await waitFor(() => expect(model).toHaveValue("qwen3-asr-flash-realtime"));
  });
});
