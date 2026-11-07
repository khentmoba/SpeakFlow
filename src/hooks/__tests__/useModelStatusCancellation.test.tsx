import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type DownloadStatusEvent = {
  payload: {
    status: string;
    message?: string;
    progress?: number;
    error?: string;
  };
};

type DownloadStatusHandler = (event: DownloadStatusEvent) => void;

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  check: vi.fn(),
  start: vi.fn(),
  restart: vi.fn(),
  download: vi.fn(),
  cancel: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/api/tauri", () => ({
  checkFunASRStatus: mocks.check,
  startFunASR: mocks.start,
  restartFunASR: mocks.restart,
  downloadModels: mocks.download,
  cancelModelDownload: mocks.cancel,
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));

import { useModelStatus } from "../useModelStatus";
import { ONBOARDING_DISMISSED_KEY } from "@/lib/constants";

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function runInitialAutoDownload() {
  await flushMicrotasks();
  await act(async () => {
    vi.advanceTimersByTime(0);
    await Promise.resolve();
    await Promise.resolve();
  });
  await flushMicrotasks();
}

async function advanceAutoRetry() {
  await act(async () => {
    vi.advanceTimersByTime(3000);
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useModelStatus automatic download cancellation", () => {
  beforeEach(() => {
    localStorage.setItem(ONBOARDING_DISMISSED_KEY, "true");
    vi.useFakeTimers();
    vi.resetAllMocks();
    mocks.listen.mockImplementation(() => Promise.resolve(vi.fn()));
    mocks.check.mockResolvedValue({
      running: false,
      ready: false,
      models_present: false,
      device: "cpu",
      gpu_name: null,
    });
    mocks.start.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
    mocks.download.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("cancels an initial automatic download before its zero-delay trigger runs", async () => {
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();

    await act(async () => {
      await hook.result.current.cancelDownload();
    });
    await act(async () => {
      vi.advanceTimersByTime(0);
      await Promise.resolve();
    });

    expect(mocks.cancel).toHaveBeenCalledOnce();
    expect(mocks.download).not.toHaveBeenCalled();
    hook.unmount();
  });

  it("does not schedule an automatic retry when cancellation precedes a pending rejection", async () => {
    const pendingDownload = deferred<void>();
    mocks.download.mockReturnValueOnce(pendingDownload.promise);

    const hook = renderHook(() => useModelStatus());
    await runInitialAutoDownload();
    expect(mocks.download).toHaveBeenCalledOnce();

    await act(async () => {
      await hook.result.current.cancelDownload();
      pendingDownload.reject(new Error("download failed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    await advanceAutoRetry();

    expect(mocks.download).toHaveBeenCalledOnce();
    hook.unmount();
  });

  it("clears an automatic retry on cancellation and unmount", async () => {
    mocks.download.mockRejectedValueOnce(new Error("download failed"));
    const cancelled = renderHook(() => useModelStatus());
    await runInitialAutoDownload();
    expect(mocks.download).toHaveBeenCalledOnce();

    await act(async () => {
      await cancelled.result.current.cancelDownload();
    });
    await advanceAutoRetry();
    expect(mocks.download).toHaveBeenCalledOnce();
    cancelled.unmount();

    mocks.download.mockReset();
    mocks.download.mockRejectedValueOnce(new Error("download failed"));
    const unmounted = renderHook(() => useModelStatus());
    await runInitialAutoDownload();
    expect(mocks.download).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBeGreaterThan(0);

    unmounted.unmount();
    expect(vi.getTimerCount()).toBe(0);
    await advanceAutoRetry();
    expect(mocks.download).toHaveBeenCalledOnce();
  });

  it("ignores a disposed download-status callback after StrictMode effect replay", async () => {
    const downloadHandlers: DownloadStatusHandler[] = [];
    mocks.check.mockResolvedValue({
      running: true,
      ready: true,
      device: "cpu",
      gpu_name: null,
    });
    mocks.listen.mockImplementation(
      (event: string, handler: DownloadStatusHandler) => {
        if (event === "model-download-status") downloadHandlers.push(handler);
        return Promise.resolve(vi.fn());
      },
    );

    const hook = renderHook(() => useModelStatus(), { wrapper: StrictMode });
    await flushMicrotasks();
    expect(downloadHandlers).toHaveLength(2);

    await act(async () => {
      downloadHandlers[0]({ payload: { status: "completed" } });
      await Promise.resolve();
    });

    expect(mocks.restart).not.toHaveBeenCalled();
    hook.unmount();
  });
});
