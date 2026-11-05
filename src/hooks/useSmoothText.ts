import { useEffect, useRef, useState } from "react";
import { prefersReducedMotion } from "@/lib/motion";

interface SmoothTextOptions {
  /** Bypass animation while keeping the latest text ready for re-enabling. */
  enabled?: boolean;
  /** Graphemes advanced per 16.67ms baseline frame. */
  charsPerFrame?: number;
  /** Snap instantly when more than this many graphemes behind. */
  snapThreshold?: number;
  /** Maximum catch-up multiplier when far behind. */
  maxCatchup?: number;
}

const INV_16_6667 = 1.0 / 16.6667;
const INV_80 = 1.0 / 80.0;

const DEFAULTS: Required<SmoothTextOptions> = {
  enabled: true,
  charsPerFrame: 2.5,
  snapThreshold: 240,
  maxCatchup: 5,
};

interface GraphemeSegmenter {
  segment(text: string): Iterable<{ segment: string }>;
}

let cachedGraphemeSegmenter: GraphemeSegmenter | null | undefined;

function getGraphemeSegmenter(): GraphemeSegmenter | null {
  if (cachedGraphemeSegmenter !== undefined) return cachedGraphemeSegmenter;

  try {
    const Segmenter = (Intl as typeof Intl & {
      Segmenter?: new (
        locales?: string,
        options?: { granularity: "grapheme" },
      ) => GraphemeSegmenter;
    }).Segmenter;
    cachedGraphemeSegmenter = Segmenter
      ? new Segmenter(undefined, { granularity: "grapheme" })
      : null;
  } catch {
    cachedGraphemeSegmenter = null;
  }

  return cachedGraphemeSegmenter;
}

const MAX_GRAPHEME_CACHE = 2048;
const graphemeCache = new Map<string, string[]>();
const offsetCache = new WeakMap<string[], Int32Array>();

/** Grapheme-aware split: keeps emoji + ZWJ sequences + CJK whole. */
export function segmentGraphemes(text: string): string[] {
  if (!text) return [];
  const hit = graphemeCache.get(text);
  if (hit !== undefined) return hit;

  const segmenter = getGraphemeSegmenter();
  let result: string[];
  if (segmenter) {
    result = [];
    for (const { segment } of segmenter.segment(text)) {
      result.push(segment);
    }
  } else {
    result = Array.from(text); // handles BMP surrogate pairs but not ZWJ joiners
  }

  if (graphemeCache.size >= MAX_GRAPHEME_CACHE) {
    const firstKey = graphemeCache.keys().next().value;
    if (firstKey !== undefined) graphemeCache.delete(firstKey);
  }
  graphemeCache.set(text, result);
  getOrComputeOffsets(result);
  return result;
}

function getOrComputeOffsets(graphemes: string[]): Int32Array {
  const cached = offsetCache.get(graphemes);
  if (cached !== undefined) return cached;
  const count = graphemes.length;
  const offsets = new Int32Array(count);
  let sum = 0;
  for (let i = 0; i < count; i++) {
    sum += graphemes[i].length;
    offsets[i] = sum;
  }
  offsetCache.set(graphemes, offsets);
  return offsets;
}

/**
 * Smooth character-by-character reveal of streaming text.
 * - Drains incoming source at ~60fps, grapheme-aware so emoji and CJK never split.
 * - Snaps immediately on reset, divergence, very-large jumps, or reduced-motion.
 * - Catches up faster as the backlog grows, so latency stays bounded.
 */
export function useSmoothText(source: string, options: SmoothTextOptions = {}): string {
  const enabled = options.enabled ?? DEFAULTS.enabled;
  const charsPerFrame = options.charsPerFrame ?? DEFAULTS.charsPerFrame;
  const snapThreshold = options.snapThreshold ?? DEFAULTS.snapThreshold;
  const maxCatchup = options.maxCatchup ?? DEFAULTS.maxCatchup;

  const [display, setDisplay] = useState(source);
  const displayRef = useRef(source);
  const sourceRef = useRef(source);
  const graphemesRef = useRef<string[]>(segmentGraphemes(source));
  const offsetsRef = useRef<Int32Array>(getOrComputeOffsets(graphemesRef.current));
  const drawnRef = useRef<number>(graphemesRef.current.length);
  const rafRef = useRef<number | null>(null);
  const lastTimeRef = useRef<number | null>(null);
  const fracRef = useRef(0);

  displayRef.current = display;
  sourceRef.current = source;

  useEffect(() => {
    const cur = displayRef.current;

    // Bypass animation or snap on a cleared/diverged source. Keep the state
    // synchronized so re-enabling never replays already-visible native text.
    if (!enabled || !source.startsWith(cur) || source.length < cur.length) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      lastTimeRef.current = null;
      fracRef.current = 0;
      graphemesRef.current = segmentGraphemes(source);
      offsetsRef.current = getOrComputeOffsets(graphemesRef.current);
      drawnRef.current = graphemesRef.current.length;
      displayRef.current = source;
      setDisplay(source);
      return;
    }

    // Already caught up.
    if (source === cur) {
      graphemesRef.current = segmentGraphemes(source);
      offsetsRef.current = getOrComputeOffsets(graphemesRef.current);
      drawnRef.current = graphemesRef.current.length;
      return;
    }

    // Refresh grapheme view with the latest source (prefix is stable).
    const nextGraphemes = segmentGraphemes(source);
    graphemesRef.current = nextGraphemes;
    offsetsRef.current = getOrComputeOffsets(nextGraphemes);
    // Prefix stability assumption holds, but clamp just in case Segmenter
    // collapses previously-drawn graphemes (e.g., partial ZWJ sequence).
    if (drawnRef.current > nextGraphemes.length) {
      drawnRef.current = nextGraphemes.length;
    }

    const behind = nextGraphemes.length - drawnRef.current;
    if (behind <= 0) {
      // Same (or fewer) graphemes than we've drawn, but the *content* of an
      // already-drawn grapheme may have changed — e.g. a ZWJ joiner or
      // combining mark just arrived and merged into the last grapheme. Snap
      // display to the new drawn-prefix so we never keep showing stale text.
      const offsets = offsetsRef.current;
      const expectedDisplay = drawnRef.current >= nextGraphemes.length
        ? source
        : drawnRef.current > 0 && offsets.length >= drawnRef.current
          ? source.slice(0, offsets[drawnRef.current - 1])
          : nextGraphemes.slice(0, drawnRef.current).join("");
      if (expectedDisplay !== cur) {
        displayRef.current = expectedDisplay;
        setDisplay(expectedDisplay);
      }
      return;
    }

    // Far behind or reduced motion → snap.
    if (behind > snapThreshold || prefersReducedMotion()) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      lastTimeRef.current = null;
      fracRef.current = 0;
      drawnRef.current = nextGraphemes.length;
      displayRef.current = source;
      setDisplay(source);
      return;
    }

    if (rafRef.current !== null) return; // loop already running, will pick up new source via refs

    const tick = (time: number) => {
      const dt = lastTimeRef.current === null
        ? 16.6667
        : Math.min(64, time - lastTimeRef.current); // cap to tame tab-return spikes
      lastTimeRef.current = time;

      const graphemes = graphemesRef.current;
      const total = graphemes.length;
      let drawn = drawnRef.current;

      if (drawn >= total) {
        rafRef.current = null;
        lastTimeRef.current = null;
        fracRef.current = 0;
        return;
      }

      const pending = total - drawn;
      const catchup = Math.min(1 + pending * INV_80, maxCatchup);
      fracRef.current += charsPerFrame * dt * INV_16_6667 * catchup;

      const take = Math.floor(fracRef.current);
      if (take > 0) {
        fracRef.current -= take;
        drawn = Math.min(total, drawn + take);
        drawnRef.current = drawn;
        const currentSource = sourceRef.current;
        const offsets = offsetsRef.current;
        const next = drawn >= total
          ? currentSource
          : drawn > 0 && offsets.length >= drawn
            ? currentSource.slice(0, offsets[drawn - 1])
            : graphemes.slice(0, drawn).join("");
        if (displayRef.current !== next) {
          displayRef.current = next;
          setDisplay(next);
        }
        if (drawn >= total) {
          rafRef.current = null;
          lastTimeRef.current = null;
          fracRef.current = 0;
          return;
        }
      }

      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
  }, [source, enabled, charsPerFrame, snapThreshold, maxCatchup]);

  useEffect(() => {
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  return enabled ? display : source;
}
