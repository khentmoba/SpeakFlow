import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const events = await vi.hoisted(async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController();
});
vi.mock("@tauri-apps/api/event", () => events.module);

import { useWaveformBars, WAVEFORM_BAR_COUNT } from "../useWaveformBars";

afterEach(() => {
  events.reset();
  vi.restoreAllMocks();
});

function mountBars(count = WAVEFORM_BAR_COUNT) {
  const host = document.createElement("div");
  for (let index = 0; index < count; index += 1) host.appendChild(document.createElement("i"));
  document.body.appendChild(host);
  return { host, ref: { current: host } as React.RefObject<HTMLElement | null> };
}

const levels = (host: HTMLElement) => [...host.children].map((bar) => (bar as HTMLElement).style.transform);

async function flushFrame() {
  await act(async () => {
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)));
  });
}

function setReducedMotion(matches: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockReturnValue({
      matches,
      media: "(prefers-reduced-motion: reduce)",
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  });
}

it("appends only the new part of each overlapping window", async () => {
  setReducedMotion(false);
  const { host, ref } = mountBars();
  renderHook(() => useWaveformBars(ref, true));
  await act(async () => { await Promise.resolve(); });

  // Nine bars cover 120 ms and arrive every 55 ms, so only ~46% of them are new.
  act(() => events.emit("waveform", { sessionId: 1, bars: [1, 1, 1, 1, 1, 1, 1, 1, 1] }));
  await flushFrame();

  // 9 bars x 55/120 ms of new audio = 4 fresh bars, and they land at the tail.
  const painted = levels(host);
  expect(painted.filter((value) => value === "scaleY(1.000)")).toHaveLength(4);
  expect(painted.slice(-4)).toEqual(["scaleY(1.000)", "scaleY(1.000)", "scaleY(1.000)", "scaleY(1.000)"]);
  expect(painted.slice(0, -4).every((value) => value === "scaleY(0.030)")).toBe(true);
});

it("ignores a stale session and stops painting after unmount", async () => {
  setReducedMotion(false);
  const { host, ref } = mountBars();
  const view = renderHook(() => useWaveformBars(ref, true));
  await act(async () => { await Promise.resolve(); });

  act(() => events.emit("waveform", { sessionId: 4, bars: [1, 1, 1, 1, 1, 1, 1, 1, 1] }));
  await flushFrame();
  const afterLive = levels(host);
  expect(afterLive.some((value) => value === "scaleY(1.000)")).toBe(true);

  act(() => events.emit("waveform", { sessionId: 2, bars: [0, 0, 0, 0, 0, 0, 0, 0, 0] }));
  await flushFrame();
  expect(levels(host)).toEqual(afterLive);

  view.unmount();
  act(() => events.emit("waveform", { sessionId: 9, bars: [0, 0, 0, 0, 0, 0, 0, 0, 0] }));
  await flushFrame();
  expect(levels(host)).toEqual(afterLive);
});

it("flattens to a resting hairline when recording stops", async () => {
  setReducedMotion(false);
  const { host, ref } = mountBars();
  const view = renderHook(({ recording }) => useWaveformBars(ref, recording), {
    initialProps: { recording: true },
  });
  await act(async () => { await Promise.resolve(); });
  act(() => events.emit("waveform", { sessionId: 1, bars: [1, 1, 1, 1, 1, 1, 1, 1, 1] }));
  await flushFrame();
  expect(levels(host).some((value) => value === "scaleY(1.000)")).toBe(true);

  view.rerender({ recording: false });
  expect(levels(host).every((value) => value === "scaleY(0.030)")).toBe(true);
});

it("holds a steady level instead of scrolling under reduced motion", async () => {
  setReducedMotion(true);
  const { host, ref } = mountBars();
  renderHook(() => useWaveformBars(ref, true));
  await act(async () => { await Promise.resolve(); });

  act(() => events.emit("waveform", { sessionId: 1, bars: [0.2, 0.4, 0.6, 0.8, 1, 0.8, 0.6, 0.4, 0.2] }));
  await flushFrame();

  const painted = levels(host);
  expect(new Set(painted).size).toBe(1);
  expect(painted[0]).toBe("scaleY(1.000)");
});
