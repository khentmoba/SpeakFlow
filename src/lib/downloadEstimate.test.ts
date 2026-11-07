import { describe, expect, it } from "vitest";
import { estimateDownload, formatEta } from "@/lib/downloadEstimate";

const MB = 1024 * 1024;

describe("estimateDownload", () => {
  it("needs two samples and a known total", () => {
    expect(estimateDownload([], 100 * MB)).toBeNull();
    expect(estimateDownload([{ at: 0, progress: 10 }], 100 * MB)).toBeNull();
    expect(estimateDownload([
      { at: 0, progress: 10 },
      { at: 2000, progress: 20 },
    ], null)).toBeNull();
  });

  it("reports speed and remaining time from the progress delta", () => {
    // 20% of 100 MB in 4 s = 5 MB/s; 80 MB left = 16 s.
    const estimate = estimateDownload([
      { at: 0, progress: 0 },
      { at: 4000, progress: 20 },
    ], 100 * MB);
    expect(estimate).not.toBeNull();
    expect(estimate!.bytesPerSecond).toBeCloseTo(5 * MB, 0);
    expect(estimate!.secondsRemaining).toBeCloseTo(16, 0);
  });

  it("ignores samples with no measurable progress", () => {
    expect(estimateDownload([
      { at: 0, progress: 40 },
      { at: 5000, progress: 40 },
    ], 100 * MB)).toBeNull();
  });

  it("smooths over the sampling window instead of one burst", () => {
    const estimate = estimateDownload([
      { at: 0, progress: 0 },
      { at: 10_000, progress: 10 },
      { at: 11_000, progress: 40 },
    ], 100 * MB);
    // The 15 s window keeps the 10 s baseline, so the burst cannot claim 30 MB/s.
    expect(estimate!.bytesPerSecond).toBeLessThan(10 * MB);
  });
});

describe("formatEta", () => {
  it.each([
    [0, "0 s"],
    [45, "45 s"],
    [60, "1 min"],
    [130, "2 min 10 s"],
    [3600, "1 h"],
    [3900, "1 h 5 min"],
  ])("formats %s seconds as %s", (seconds, expected) => {
    expect(formatEta(seconds)).toBe(expected);
  });
});
