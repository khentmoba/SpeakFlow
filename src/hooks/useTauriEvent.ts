import { useEffect, useEffectEvent } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Subscribe to a Tauri event for the lifetime of the component.
 *
 * The handler always sees the latest props and state, so callers may pass an
 * inline closure without resubscribing on every render. A subscription that
 * resolves after unmount is released immediately, and no event is delivered
 * after cleanup.
 */
export function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  const onEvent = useEffectEvent(handler);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    listen<T>(event, (incoming) => {
      if (!disposed) onEvent(incoming.payload);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {
        // A missing event bridge leaves the rest of the UI usable.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [event]);
}
