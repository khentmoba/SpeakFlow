import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { getRecordingSnapshot } from "@/api/tauri";
import "@/i18n";
import "../styles/theme.css";
import "../styles/pill.css";

type PillPhase = "idle" | "starting" | "recording" | "processing";

interface RecordingStatePayload {
  isStarting?: boolean;
  isRecording?: boolean;
  isProcessing?: boolean;
}

const BAR_COUNT = 7;
const MIN_BAR = 0.08;

function phaseFrom(payload: RecordingStatePayload): PillPhase {
  if (payload.isStarting) return "starting";
  if (payload.isRecording) return "recording";
  if (payload.isProcessing) return "processing";
  return "idle";
}

function resampleBars(source: number[]): number[] {
  if (!Array.isArray(source) || source.length === 0) {
    return Array.from({ length: BAR_COUNT }, () => MIN_BAR);
  }
  return Array.from({ length: BAR_COUNT }, (_, index) => {
    const value = source[Math.round((index * (source.length - 1)) / (BAR_COUNT - 1))];
    return Number.isFinite(value) ? Math.max(MIN_BAR, Math.min(1, value)) : MIN_BAR;
  });
}

/**
 * The floating recording pill. It is the only on-screen feedback while the
 * user dictates into another app, so it follows the same recording-state event
 * that drives the tray mark and reads the waveform stream directly.
 */
export default function RecordingPill() {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<PillPhase>("idle");
  const [bars, setBars] = useState<number[]>(() =>
    Array.from({ length: BAR_COUNT }, () => MIN_BAR));

  useEffect(() => {
    let disposed = false;
    const disposers: Array<() => void> = [];

    void getRecordingSnapshot()
      .then((snapshot) => {
        if (disposed || !snapshot) return;
        setPhase(phaseFrom({
          isStarting: snapshot.phase === "starting",
          isRecording: snapshot.phase === "recording",
          isProcessing: snapshot.phase === "processing",
        }));
      })
      .catch(() => undefined);

    void listen<RecordingStatePayload>("recording-state", ({ payload }) => {
      if (!disposed) setPhase(phaseFrom(payload ?? {}));
    })
      .then((unlisten) => { if (disposed) unlisten(); else disposers.push(unlisten); })
      .catch(() => undefined);

    void listen<{ bars: number[] }>("waveform", ({ payload }) => {
      if (disposed) return;
      setBars(resampleBars(payload?.bars ?? []));
    })
      .then((unlisten) => { if (disposed) unlisten(); else disposers.push(unlisten); })
      .catch(() => undefined);

    return () => {
      disposed = true;
      for (const dispose of disposers) dispose();
    };
  }, []);

  if (phase === "idle") return null;

  return (
    <div className="pill" data-phase={phase} role="status" aria-live="polite">
      <span className="pill-signal" aria-hidden="true" />
      <span className="pill-label">{t(`pill.${phase}`)}</span>
      {phase === "recording" && (
        <span className="pill-bars" aria-hidden="true">
          {bars.map((level, index) => (
            <i key={index} style={{ transform: `scaleY(${level})` }} />
          ))}
        </span>
      )}
    </div>
  );
}
