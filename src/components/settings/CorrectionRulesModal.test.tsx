import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import CorrectionRulesModal from "./CorrectionRulesModal";

const api = vi.hoisted(() => ({
  removeCorrection: vi.fn(),
  setCorrectionValidationConfig: vi.fn(),
  setExternalCorrectionLearning: vi.fn(),
  addCorrectionRule: vi.fn(),
  validateCorrections: vi.fn(),
}));
const toast = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function setup() {
  const props: ComponentProps<typeof CorrectionRulesModal> = {
    profile: null,
    allProviderOptions: [],
    validationEnabled: false,
    setValidationEnabled: vi.fn(),
    validationUseSeparateModel: false,
    setValidationUseSeparateModel: vi.fn(),
    validationProvider: null,
    setValidationProvider: vi.fn(),
    validationModel: "",
    setValidationModel: vi.fn(),
    validationRunning: false,
    setValidationRunning: vi.fn(),
    validationResult: null,
    setValidationResult: vi.fn(),
    returnFocusRef: { current: null },
    onClose: vi.fn(),
    onRefreshProfile: vi.fn(),
  };
  render(<CorrectionRulesModal {...props} />);
  return { props, toggle: screen.getByRole("switch", { name: "settings.externalCorrectionLearning" }) };
}

beforeEach(() => vi.resetAllMocks());

describe("external correction learning", () => {
  it("is opt-in and persists both enabling and disabling", async () => {
    api.setExternalCorrectionLearning.mockResolvedValue(undefined);
    const { props, toggle } = setup();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(api.setExternalCorrectionLearning).toHaveBeenLastCalledWith(true);
    expect(props.onRefreshProfile).toHaveBeenCalledOnce();
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    expect(api.setExternalCorrectionLearning).toHaveBeenLastCalledWith(false);
  });

  it("disables the switch while saving and keeps the previous value on failure", async () => {
    let reject!: (error: Error) => void;
    api.setExternalCorrectionLearning.mockReturnValue(new Promise<void>((_, fail) => { reject = fail; }));
    const { props, toggle } = setup();
    fireEvent.click(toggle);
    expect(toggle).toBeDisabled();
    reject(new Error("failed"));
    await waitFor(() => expect(toggle).not.toBeDisabled());
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(props.onRefreshProfile).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith("settings.externalCorrectionLearningFailed");
  });

  it("adds a manual correction rule and refreshes the profile", async () => {
    api.addCorrectionRule.mockResolvedValue(undefined);
    const { props } = setup();
    fireEvent.change(screen.getByLabelText("settings.correctionOriginalPlaceholder"), {
      target: { value: "Speekflow" },
    });
    fireEvent.change(screen.getByLabelText("settings.correctionCorrectedPlaceholder"), {
      target: { value: "SpeakFlow" },
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.correctionAddRuleLabel" }));
    await waitFor(() => expect(api.addCorrectionRule).toHaveBeenCalledWith("Speekflow", "SpeakFlow"));
    expect(props.onRefreshProfile).toHaveBeenCalledOnce();
  });
});
