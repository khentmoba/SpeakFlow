import { describe, expect, it } from "vitest";
import { smoothMicLevel } from "@/hooks/useMicrophoneSettings";

describe("microphone level smoothing", () => {
  it("rises faster than it falls", () => {
    const risen = smoothMicLevel(0, 1);
    const fallen = smoothMicLevel(1, 0);
    expect(risen).toBeGreaterThan(0.5);
    expect(fallen).toBeGreaterThan(0.5);
    // A jump up covers more ground in one step than a drop of the same size.
    expect(risen).toBeGreaterThan(1 - fallen);
  });

  it("settles on a steady level without overshooting", () => {
    let level = 0;
    for (let step = 0; step < 40; step += 1) level = smoothMicLevel(level, 0.4);
    expect(level).toBeCloseTo(0.4, 1);
    expect(level).toBeLessThanOrEqual(0.4);
  });

  it("moves gradually instead of jumping to the reading", () => {
    const level = smoothMicLevel(0.1, 0.9);
    expect(level).toBeGreaterThan(0.1);
    expect(level).toBeLessThan(0.9);
  });
});
