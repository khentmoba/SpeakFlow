import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getVoiceProcessing: vi.fn(),
  setSilenceFilter: vi.fn(),
  setRemoveFillers: vi.fn(),
  setAsrLanguage: vi.fn(),
  setTranslateToEnglish: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("@/api/tauri", () => ({
  getVoiceProcessing: mocks.getVoiceProcessing,
  setSilenceFilter: mocks.setSilenceFilter,
  setRemoveFillers: mocks.setRemoveFillers,
  setAsrLanguage: mocks.setAsrLanguage,
  setTranslateToEnglish: mocks.setTranslateToEnglish,
}));
vi.mock("sonner", () => ({ toast: { error: mocks.toastError } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import { useVoiceProcessingSettings } from "../useVoiceProcessingSettings";

describe("useVoiceProcessingSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getVoiceProcessing.mockResolvedValue({
      silenceFilter: false,
      removeFillers: true,
      language: "en",
      translateToEnglish: true,
    });
  });

  it("loads the saved settings", async () => {
    const { result } = renderHook(() => useVoiceProcessingSettings());
    await waitFor(() => expect(result.current.asrLanguage).toBe("en"));
    expect(result.current.silenceFilter).toBe(false);
    expect(result.current.removeFillers).toBe(true);
    expect(result.current.translateToEnglish).toBe(true);
  });

  it("rolls a rejected toggle back and reports it", async () => {
    mocks.setSilenceFilter.mockRejectedValue(new Error("denied"));
    const { result } = renderHook(() => useVoiceProcessingSettings());
    await waitFor(() => expect(result.current.silenceFilter).toBe(false));

    await act(() => result.current.handleSilenceFilterToggle(true));

    expect(result.current.silenceFilter).toBe(false);
    expect(mocks.toastError).toHaveBeenCalledWith("toast.settingSaveFailed");
  });

  it("keeps the language the backend saved, or restores the previous one", async () => {
    const { result } = renderHook(() => useVoiceProcessingSettings());
    await waitFor(() => expect(result.current.asrLanguage).toBe("en"));

    mocks.setAsrLanguage.mockResolvedValueOnce("zh");
    await act(() => result.current.handleLanguageChange("zh-CN"));
    expect(result.current.asrLanguage).toBe("zh");
    expect(result.current.languageSaving).toBe(false);

    mocks.setAsrLanguage.mockRejectedValueOnce(new Error("denied"));
    await act(() => result.current.handleLanguageChange("ja"));
    expect(result.current.asrLanguage).toBe("zh");
  });
});
