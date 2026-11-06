import { useEffect, useRef, type RefObject } from "react";
import { listen } from "@tauri-apps/api/event";
import { prefersReducedMotion } from "@/lib/motion";

/** Bars drawn across the signal panel. */
export const WAVEFORM_BAR_COUNT = 78;
/** Rightmost bars that carry the live (amber) treatment. */
export const WAVEFORM_LIVE_TAIL = 18;

/**
 * Fraction of each native window that is genuinely new. The emitter samples a
 * 120 ms window every 55 ms, so consecutive payloads overlap by about 54%.
 * Appending the whole payload would draw every syllable twice.
 */
const NEW_SAMPLE_RATIO = 55 / 120;

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
    const paint = (levels: number[]) => {
      const container = containerRef.current;
      if (!container) return;
      const bars = container.children;
      for (let index = 0; index < bars.length && index < barCount; index += 1) {
        const element = bars[index] as HTMLElement;
        const value = levels[index] ?? 0;
        // A resting bar stays visible as a hairline so the panel never looks broken.
        element.style.transform = `scaleY(${Math.max(0.03, value).toFixed(3)})`;
        element.style.opacity = value > 0.04 ? "1" : "0.5";
      }
    };

    const schedulePaint = (levels: number[]) => {
      if (frameRef.current !== null) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = null;
        paint(levels);
      });
    };

    if (!isRecording) {
      bufferRef.current = Array.from({ length: barCount }, () => 0);
      paint(bufferRef.current);
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

      const levels = incoming.map((value) => (
        Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0
      ));

      // Reduced motion: show the current level without scrolling the history.
      if (prefersReducedMotion()) {
        const peak = Math.max(...levels);
        bufferRef.current = Array.from({ length: barCount }, () => peak);
        schedulePaint(bufferRef.current);
        return;
      }

      const fresh = Math.max(1, Math.round(levels.length * NEW_SAMPLE_RATIO));
      const buffer = bufferRef.current;
      buffer.copyWithin(0, fresh);
      buffer.splice(barCount - fresh, fresh, ...levels.slice(levels.length - fresh));
      schedulePaint(buffer);
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
