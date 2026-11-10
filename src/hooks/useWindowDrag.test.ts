import type React from "react";
import { describe, expect, it, vi } from "vitest";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { handleWindowDrag } from "./useWindowDrag";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn(),
}));

describe("handleWindowDrag", () => {
  it("ignores non-primary mouse clicks", () => {
    const startDragging = vi.fn();
    vi.mocked(getCurrentWindow).mockReturnValue({ startDragging } as unknown as ReturnType<typeof getCurrentWindow>);

    const event = { button: 1, target: document.createElement("div"), preventDefault: vi.fn() } as unknown as React.MouseEvent;
    handleWindowDrag(event);
    expect(startDragging).not.toHaveBeenCalled();
  });

  it("ignores clicks on interactive buttons", () => {
    const startDragging = vi.fn();
    vi.mocked(getCurrentWindow).mockReturnValue({ startDragging } as unknown as ReturnType<typeof getCurrentWindow>);

    const button = document.createElement("button");
    const event = { button: 0, target: button, preventDefault: vi.fn() } as unknown as React.MouseEvent;
    handleWindowDrag(event);
    expect(startDragging).not.toHaveBeenCalled();
  });

  it("calls startDragging on drag surfaces", () => {
    const startDragging = vi.fn().mockResolvedValue(undefined);
    vi.mocked(getCurrentWindow).mockReturnValue({ startDragging } as unknown as ReturnType<typeof getCurrentWindow>);

    const div = document.createElement("div");
    const event = { button: 0, target: div, preventDefault: vi.fn() } as unknown as React.MouseEvent;
    handleWindowDrag(event);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(startDragging).toHaveBeenCalled();
  });
});
