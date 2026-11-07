import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import SettingsNav from "./SettingsNav";
import "@/i18n";

const sections = [
  { id: "appearance", labelKey: "settings.appearance" },
  { id: "engine", labelKey: "settings.engine" },
  { id: "microphone", labelKey: "settings.microphone" },
  { id: "startup", labelKey: "settings.startup" },
  { id: "support", labelKey: "settings.support" },
] as const;

function setup(query = "") {
  const onQueryChange = vi.fn();
  const onSelect = vi.fn();
  const navRef = { current: null };
  const view = render(
    <SettingsNav
      sections={sections}
      activeId="appearance"
      query={query}
      navRef={navRef}
      onQueryChange={onQueryChange}
      onSelect={onSelect}
    />,
  );
  return { onQueryChange, onSelect, view };
}

describe("SettingsNav", () => {
  it("lists every section and the search box", () => {
    setup();
    const nav = screen.getByRole("navigation", { name: "Settings" });
    expect(nav).toHaveTextContent("Appearance");
    expect(nav).toHaveTextContent("Support");
    expect(screen.getByRole("searchbox", { name: "Search settings" })).toBeInTheDocument();
  });

  it("filters sections by everyday keywords", () => {
    setup("logs");
    expect(screen.queryByRole("button", { name: /Appearance/ })).toBeNull();
    expect(screen.getByRole("button", { name: /Support/ })).toBeInTheDocument();
  });

  it("reports an empty result instead of an empty rail", () => {
    setup("zzzz");
    expect(screen.getByText("No setting matches that search.")).toBeInTheDocument();
  });

  it("reports typing and section clicks", () => {
    const { onQueryChange, onSelect } = setup();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search settings" }), {
      target: { value: "mic" },
    });
    expect(onQueryChange).toHaveBeenCalledWith("mic");
    fireEvent.click(screen.getByRole("button", { name: /Microphone/ }));
    expect(onSelect).toHaveBeenCalledWith("microphone");
  });
});
