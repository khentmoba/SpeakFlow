import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createTauriEventController } from "@/test/tauriEventMock";

const events = vi.hoisted(() => ({ controller: null as null | ReturnType<typeof createTauriEventController> }));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController: create } = await import("@/test/tauriEventMock");
  events.controller = create();
  return events.controller.module;
});

import { useTauriEvent } from "../useTauriEvent";

const controller = () => events.controller!;

describe("useTauriEvent", () => {
  beforeEach(() => controller().reset());

  it("delivers payloads to the latest handler without resubscribing", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ handler }) => useTauriEvent<number>("tick", handler), {
      initialProps: { handler: first },
    });
    await act(async () => {});

    rerender({ handler: second });
    act(() => controller().emit("tick", 7));

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith(7);
    expect(controller().listen).toHaveBeenCalledOnce();
  });

  it("stops delivering after unmount", async () => {
    const handler = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent("tick", handler));
    await act(async () => {});

    unmount();
    controller().emit("tick", 1);

    expect(handler).not.toHaveBeenCalled();
  });

  it("releases a subscription that resolves after unmount", async () => {
    let resolve!: (stop: () => void) => void;
    controller().listen.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    const stop = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent("tick", vi.fn()));

    unmount();
    await act(async () => { resolve(stop); });

    expect(stop).toHaveBeenCalledOnce();
  });

  it("swallows a failed subscription", async () => {
    controller().listen.mockRejectedValueOnce(new Error("no bridge"));
    renderHook(() => useTauriEvent("tick", vi.fn()));
    await act(async () => {});
  });
});
