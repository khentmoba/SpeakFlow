import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  test: {
    maxWorkers: 4,
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    testTimeout: 10000,
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "text"],
      // Ratchet: the baseline after removing UI tests (1.2.0) is 74.23
      // statements / 65.02 branches / 66.49 functions / 76.35 lines. Keep a
      // small margin so ordinary refactors do not fail CI, while a real drop
      // still does. CI runs on Linux, so statements keep ~1.5 points of
      // headroom for platform drift.
      thresholds: {
        statements: 72.5,
        branches: 64,
        functions: 65,
        lines: 75,
      },
    },
  },
});
