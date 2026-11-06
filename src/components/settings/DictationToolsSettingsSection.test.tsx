import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DictationToolsSettings } from "@/types";

const api = vi.hoisted(() => ({
  getDictationTools: vi.fn(),
  setDictationTools: vi.fn(),
}));

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import DictationToolsSettingsSection from "@/components/settings/DictationToolsSettingsSection";

const stored: DictationToolsSettings = {
  snippets: [{ trigger: "my email", expansion: "khent@example.com" }],
  toneMode: "casual",
  tidyTranscript: true,
  voiceCommands: false,
  whisperMode: false,
  localOnly: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  api.getDictationTools.mockResolvedValue(stored);
  api.setDictationTools.mockImplementation((next: DictationToolsSettings) =>
    Promise.resolve(next),
  );
});

describe("DictationToolsSettingsSection", () => {
  it("renders the stored settings", async () => {
    render(<DictationToolsSettingsSection />);

    await waitFor(() => expect(screen.getByLabelText("settings.toneMode")).toHaveValue("casual"));
    expect(screen.getByLabelText("settings.snippetTrigger")).toHaveValue("my email");
    expect(screen.getByLabelText("settings.voiceCommands")).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });

  it("saves the flipped value when a toggle is clicked", async () => {
    const user = userEvent.setup();
    render(<DictationToolsSettingsSection />);
    await waitFor(() => expect(api.getDictationTools).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByLabelText("settings.voiceCommands")).not.toBeDisabled(),
    );

    await user.click(screen.getByLabelText("settings.voiceCommands"));

    await waitFor(() =>
      expect(api.setDictationTools).toHaveBeenCalledWith(
        expect.objectContaining({ voiceCommands: true }),
      ),
    );
    await waitFor(() =>
      expect(screen.getByLabelText("settings.voiceCommands")).toHaveAttribute(
        "aria-checked",
        "true",
      ),
    );
  });

  it("saves a snippet only once both fields are filled in", async () => {
    const user = userEvent.setup();
    render(<DictationToolsSettingsSection />);
    await waitFor(() => expect(api.getDictationTools).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByLabelText("settings.snippetTrigger")).toHaveValue("my email"),
    );

    await user.click(screen.getByRole("button", { name: /settings.snippetAdd/ }));
    const triggers = screen.getAllByLabelText("settings.snippetTrigger");
    const expansions = screen.getAllByLabelText("settings.snippetExpansion");
    expect(triggers).toHaveLength(2);

    await user.type(triggers[1], "my address");
    await user.tab();
    // The row is incomplete, so nothing new is saved yet.
    expect(api.setDictationTools).not.toHaveBeenCalled();

    await user.type(expansions[1], "1 Example Street");
    await user.tab();

    await waitFor(() =>
      expect(api.setDictationTools).toHaveBeenLastCalledWith(
        expect.objectContaining({
          snippets: [
            { trigger: "my email", expansion: "khent@example.com" },
            { trigger: "my address", expansion: "1 Example Street" },
          ],
        }),
      ),
    );
  });

  it("removes a snippet row", async () => {
    const user = userEvent.setup();
    render(<DictationToolsSettingsSection />);
    await waitFor(() => expect(api.getDictationTools).toHaveBeenCalled());
    await waitFor(() =>
      expect(screen.getByLabelText("settings.snippetTrigger")).toHaveValue("my email"),
    );

    await user.click(screen.getByRole("button", { name: "settings.snippetRemove" }));

    await waitFor(() =>
      expect(api.setDictationTools).toHaveBeenLastCalledWith(
        expect.objectContaining({ snippets: [] }),
      ),
    );
  });
});
