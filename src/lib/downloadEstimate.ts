export interface DownloadSample {
  /** Milliseconds since the epoch. */
  at: number;
  /** Overall progress, 0-100. */
  progress: number;
}

export interface DownloadEstimate {
  bytesPerSecond: number;
  secondsRemaining: number;
}

/** Smooth over the last 15 seconds so one burst does not swing the estimate. */
const SAMPLE_WINDOW_MS = 15_000;
const MIN_ELAPSED_SECONDS = 1;

/**
 * Speed and time remaining from progress samples. Returns null until there are
 * two usable samples, so the UI never shows a made-up number on the first tick.
 */
export function estimateDownload(
  samples: readonly DownloadSample[],
  totalBytes: number | null,
): DownloadEstimate | null {
  if (!totalBytes || totalBytes <= 0 || samples.length < 2) return null;
  const last = samples[samples.length - 1];
  const windowStart = last.at - SAMPLE_WINDOW_MS;
  const first = samples.find((sample) => sample.at >= windowStart) ?? samples[0];
  const elapsedSeconds = (last.at - first.at) / 1000;
  const progressDelta = last.progress - first.progress;
  if (elapsedSeconds < MIN_ELAPSED_SECONDS || progressDelta <= 0) return null;
  const bytesPerSecond = ((progressDelta / 100) * totalBytes) / elapsedSeconds;
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return null;
  const remainingBytes = (Math.max(0, 100 - last.progress) / 100) * totalBytes;
  return { bytesPerSecond, secondsRemaining: remainingBytes / bytesPerSecond };
}

/** "5.00 MB/s", "120 MB/s" — one unit, no rounding to zero for slow links. */
export function formatDownloadSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return "0 MB/s";
  const megabytes = bytesPerSecond / 1_000_000;
  if (megabytes >= 100) return `${Math.round(megabytes)} MB/s`;
  if (megabytes >= 10) return `${megabytes.toFixed(1)} MB/s`;
  return `${megabytes.toFixed(2)} MB/s`;
}

/** "45 s", "2 min 10 s", "1 h 5 min" — compact enough for the status line. */
export function formatEta(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  if (total < 60) return `${total} s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) {
    const rest = total % 60;
    return rest === 0 ? `${minutes} min` : `${minutes} min ${rest} s`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}
