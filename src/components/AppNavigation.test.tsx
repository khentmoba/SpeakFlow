import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AppNavigation from "./AppNavigation";

vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: vi.fn() }));

describe("workspace navigation", () => {
  it("identifies the active view and opens each existing workspace", () => {
    const onNavigate = vi.fn();
    const { rerender } = render(<AppNavigation view="main" onNavigate={onNavigate} />);
    expect(screen.getByRole("navigation", { name: "Main navigation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dictation" })).toHaveAttribute("aria-current", "page");
    for (const [label, view] of [["Dictation", "main"], ["History", "history"], ["Settings", "settings"], ["Updates", "updates"]]) {
      fireEvent.click(screen.getByRole("button", { name: label }));
      expect(onNavigate).toHaveBeenLastCalledWith(view);
    }
    rerender(<AppNavigation view="settings" onNavigate={onNavigate} />);
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "Dictation" })).not.toHaveAttribute("aria-current");
  });

  it("names an available update without the installing suffix", () => {
    render(<AppNavigation view="main" onNavigate={vi.fn()} updateAvailable />);
    expect(screen.getByRole("button", { name: "Updates, New release available" })).toBeInTheDocument();
  });

  it("marks an available update and identifies the update destination", () => {
    render(<AppNavigation view="updates" onNavigate={vi.fn()} updateAvailable updateInstalling />);
    expect(screen.getByRole("button", { name: "Updates, Update in progress" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("img", { name: "Update in progress" })).toBeInTheDocument();
  });
});
