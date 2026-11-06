import { useEffect, useRef, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";

/** Bars drawn across the signal panel. */
export const WAVEFORM_BAR_COUNT = 78;
/** Rightmost bars that carry the live (amber) treatment. */
export const WAVEFORM_LIVE_TAIL = 18;

/**
 * Rolls live microphone levels into a scrolling waveform.
 *
 * The native side emits nine RMS bars roughly eighteen times a second. Writing
 * those through React state would re-render the whole dictation page at that
 * rate, so the buffer is painted straight onto the bar elements inside a single
 * animation frame instead.
 */
export function useWaveformBars(
  containerRef: RefObject<HTMLElement | null>,
  isRecording: boolean,
  barCount: number = WAVEFORM_BAR_COUNT,
): void {
  const bufferRef = useRef<number[]>(Array.from({ length: barCount }, () => 0));
  const frameRef = useRef<number | null>(null);
  const latestSession = useRef(0);

  useEffect(() => {
    const paint = () => {
      const container = containerRef.current;
      if (!container) return;
      const bars = container.children;
      for (let index = 0; index < bars.length && index < barCount; index += 1) {
        const element = bars[index] as HTMLElement;
        const value = bufferRef.current[index] ?? 0;
        // A resting bar stays visible as a hairline so the panel never looks broken.
        element.style.transform = `scaleY(${Math.max(0.03, value).toFixed(3)})`;
        element.style.opacity = value > 0.04 ? "1" : "0.5";
      }
    };

    const schedulePaint = () => {
      if (frameRef.current !== null) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = null;
        paint();
      });
    };

    if (!isRecording) {
      bufferRef.current = Array.from({ length: barCount }, () => 0);
      paint();
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<{ sessionId: number; bars: number[] }>("waveform", ({ payload }) => {
      if (disposed) return;
      if (!Number.isSafeInteger(payload.sessionId) || payload.sessionId <= 0) return;
      if (payload.sessionId < latestSession.current) return;
      latestSession.current = payload.sessionId;

      const incoming = payload.bars;
      if (!Array.isArray(incoming) || incoming.length === 0) return;

      const buffer = bufferRef.current;
      const shift = Math.min(incoming.length, barCount);
      buffer.copyWithin(0, shift);
      for (let index = 0; index < shift; index += 1) {
        const sourceIndex = shift === 1
          ? incoming.length - 1
          : Math.round((index / (shift - 1)) * (incoming.length - 1));
        const value = incoming[sourceIndex];
        buffer[barCount - shift + index] = Number.isFinite(value)
          ? Math.max(0, Math.min(1, value))
          : 0;
      }
      schedulePaint();
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    };
  }, [barCount, containerRef, isRecording]);
}
