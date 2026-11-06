import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { expectNoA11yViolations } from "@/test/a11y";
import SupportSettingsSection from "@/components/settings/SupportSettingsSection";
import SettingsNav from "@/components/settings/SettingsNav";
import "@/i18n";

vi.mock("@/api/tauri", () => ({
  copyToClipboard: vi.fn(),
  getDiagnostics: vi.fn(),
  openLogsFolder: vi.fn(),
}));

describe("settings accessibility", () => {
  it("Support has no structural violations", async () => {
    const { container } = render(<SupportSettingsSection />);
    await expectNoA11yViolations(container);
  });

  it("the settings rail has no structural violations", async () => {
    const { container } = render(
      <SettingsNav
        sections={[
          { id: "appearance", labelKey: "settings.appearance" },
          { id: "support", labelKey: "settings.support" },
        ]}
        activeId="appearance"
        query=""
        navRef={{ current: null }}
        onQueryChange={vi.fn()}
        onSelect={vi.fn()}
      />,
    );
    await expectNoA11yViolations(container);
  });

  it("the search field keeps an accessible name", () => {
    render(
      <SettingsNav
        sections={[{ id: "support", labelKey: "settings.support" }]}
        activeId="support"
        query=""
        navRef={{ current: null }}
        onQueryChange={vi.fn()}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByRole("searchbox", { name: "Search settings" })).toBeInTheDocument();
  });
});
