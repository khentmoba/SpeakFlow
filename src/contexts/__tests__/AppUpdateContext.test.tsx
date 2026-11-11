import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

const api = vi.hoisted(() => ({
  checkAppUpdate: vi.fn(),
  installAppUpdate: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({
  toast: { info: vi.fn(), success: vi.fn(), error: vi.fn() },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { AppUpdateProvider, useAppUpdates } from "../AppUpdateContext";

function wrapper({ children }: { children: ReactNode }) {
  return <AppUpdateProvider>{children}</AppUpdateProvider>;
}

describe("automatic update checks", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    api.checkAppUpdate.mockResolvedValue({
      available: false,
      latestVersion: null,
      notes: null,
      releaseUrl: null,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.resetAllMocks();
  });

  it("waits for the startup delay before the first automatic check", async () => {
    renderHook(() => useAppUpdates(), { wrapper });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(4_000);
    });
    expect(api.checkAppUpdate).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(api.checkAppUpdate).toHaveBeenCalledTimes(1);
  });
});
