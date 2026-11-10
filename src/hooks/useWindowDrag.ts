import type React from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function handleWindowDrag(e: React.MouseEvent) {
  if (e.button !== 0) return;
  const target = e.target as HTMLElement;
  if (
    target.closest("button") ||
    target.closest("input") ||
    target.closest("select") ||
    target.closest("textarea") ||
    target.closest("a") ||
    target.closest("[data-no-drag]")
  ) {
    return;
  }
  e.preventDefault();
  try {
    void getCurrentWindow().startDragging().catch(() => undefined);
  } catch {
    // non-Tauri preview fallback
  }
}
