import { useEffect, useRef, useState } from "react";
import {
  estimateDownload,
  type DownloadEstimate,
  type DownloadSample,
} from "@/lib/downloadEstimate";

const MAX_SAMPLES = 120;

/**
 * Samples download progress while it is active and returns a smoothed speed
 * and ETA. Samples reset whenever the download stops, so a resumed download
 * starts from a fresh baseline instead of mixing two rates.
 */
export function useDownloadEstimate(
  progress: number,
  totalBytes: number | null,
  active: boolean,
): DownloadEstimate | null {
  const samples = useRef<DownloadSample[]>([]);
  const [estimate, setEstimate] = useState<DownloadEstimate | null>(null);

  useEffect(() => {
    if (!active) {
      samples.current = [];
      setEstimate(null);
      return;
    }
    samples.current.push({ at: Date.now(), progress });
    if (samples.current.length > MAX_SAMPLES) samples.current.shift();
    // Compute after appending so the shown speed matches the latest event.
    setEstimate(estimateDownload(samples.current, totalBytes));
  }, [progress, active, totalBytes]);

  return active ? estimate : null;
}
