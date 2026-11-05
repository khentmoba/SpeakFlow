import type { AssistantConversationTurn, RecordingOutcomeKind, RecordingSnapshot } from "@/api/tauri";
import { segmentGraphemes } from "@/hooks/useSmoothText";

export interface RecordingState {
  sessionId?: number;
  revision?: number;
  phase?: RecordingSnapshot["phase"];
  isStarting?: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  mode?: "dictation" | "assistant";
}

export interface TranscriptionResult {
  sessionId?: number;
  text: string;
  interim?: boolean;
  stableText?: string;
  tentativeText?: string;
  polished?: boolean;
  mode?: "dictation" | "assistant";
}

export interface InterimSegments {
  stableText: string;
  tentativeText: string;
}

export interface RecordingOutcome {
  sessionId?: number;
  revision?: number;
  outcome: RecordingOutcomeKind;
  mode?: "dictation" | "assistant";
  detail?: string;
}

export interface AssistantSource {
  title: string;
  url: string;
  publishedDate?: string | null;
}

export interface AssistantStreamEvent {
  sessionId?: number;
  chunk?: string;
  status?: string;
  request?: string;
  message?: string;
  source?: AssistantSource;
  sources?: AssistantSource[];
  googleSearchEntryPoint?: string | null;
  query?: string;
  searchProvider?: "model_native" | "exa" | "tavily" | "google" | "bing";
  elapsedMs?: number;
  searchElapsedMs?: number | null;
  webSearchEnabled?: boolean;
}

export interface ConversationMessage extends AssistantConversationTurn {
  id: number;
  sources?: AssistantSource[];
  googleSearchEntryPoint?: string;
}

export type Phase = "idle" | "starting" | "recording" | "processing" | "searching" | "polishing" | "result" | "outcome";
export const RESULT_FADE_DELAY_MS = 2000;
// Fade animation is 160ms; keep a buffer before clearing the stale state and
// backend hides the subtitle window at 2500ms, so we clear stale state in time
// to prevent a one-frame flash of previous text on the next recording.
export const RESULT_CLEANUP_DELAY_MS = RESULT_FADE_DELAY_MS + 400;
export const OUTCOME_FADE_DELAY_MS = 1500;
export const OUTCOME_CLEANUP_DELAY_MS = OUTCOME_FADE_DELAY_MS + 400;
export const QUICK_CANCEL_CLEANUP_DELAY_MS = 400;
export const WAVEFORM_BAR_COUNT = 9;
export const EMPTY_WAVEFORM_BARS = Array.from({ length: WAVEFORM_BAR_COUNT }, () => 0);

export function isValidSessionId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

export function isValidRevision(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function isRecordingOutcomeKind(value: unknown): value is RecordingOutcomeKind {
  return value === "too_short"
    || value === "no_speech"
    || value === "asr_error"
    || value === "processing_error"
    || value === "start_error";
}

export function normalizeWaveformBars(bars: number[]): number[] {
  return Array.from({ length: WAVEFORM_BAR_COUNT }, (_, index) => {
    const value = bars[index];
    return Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 0;
  });
}

export function validInterimSegments(payload: TranscriptionResult): InterimSegments | null {
  const { stableText, tentativeText, text } = payload;
  if (typeof stableText !== "string" || typeof tentativeText !== "string") return null;
  if (`${stableText}${tentativeText}` !== text) return null;

  // Rust guarantees a UTF-8 scalar boundary. Tighten it to a grapheme
  // boundary here so combining marks and ZWJ emoji never straddle two spans.
  let stableBoundary = 0;
  for (const grapheme of segmentGraphemes(text)) {
    const nextBoundary = stableBoundary + grapheme.length;
    if (nextBoundary > stableText.length) break;
    stableBoundary = nextBoundary;
  }
  return {
    stableText: text.slice(0, stableBoundary),
    tentativeText: text.slice(stableBoundary),
  };
}

export function mergeAssistantSources(
  current: AssistantSource[],
  incoming: AssistantSource[],
): AssistantSource[] {
  const merged = [...current];
  const seen = new Set(current.map((source) => source.url));
  for (const source of incoming) {
    if (!source?.url || seen.has(source.url)) continue;
    seen.add(source.url);
    merged.push(source);
  }
  return merged.slice(0, 10);
}
