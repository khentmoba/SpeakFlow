import { beforeEach, describe, expect, it } from "vitest";
import { LABS_FAILURE_NOTICES_KEY, LABS_OUTPUT_FIXES_KEY } from "@/lib/constants";
import { mirrorLabsToStorage } from "@/lib/labsMirror";

describe("mirrorLabsToStorage", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("writes both flags the overlay and History page read", () => {
    mirrorLabsToStorage({ failureNotices: false, outputFixes: true });

    expect(localStorage.getItem(LABS_FAILURE_NOTICES_KEY)).toBe("false");
    expect(localStorage.getItem(LABS_OUTPUT_FIXES_KEY)).toBe("true");
  });

  it("overwrites a stale mirror when the saved profile changes", () => {
    localStorage.setItem(LABS_OUTPUT_FIXES_KEY, "false");

    mirrorLabsToStorage({ failureNotices: true, outputFixes: true });

    expect(localStorage.getItem(LABS_OUTPUT_FIXES_KEY)).toBe("true");
  });
});
