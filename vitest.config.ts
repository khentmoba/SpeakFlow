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
    setupFiles: ["./src/test/setup.ts"],
    globals: true,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    testTimeout: 10000,
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "text"],
      // Ratchet: the v1.1.8 baseline is 77.65 statements / 71.54 branches /
      // 70.94 functions / 80.06 lines. Keep a small margin so ordinary
      // refactors do not fail CI, while a real drop still does. CI runs on
      // Linux, so statements keep ~1.5 points of headroom for platform drift.
      thresholds: {
        statements: 76,
        branches: 71,
        functions: 70,
        lines: 79,
      },
    },
  },
});
