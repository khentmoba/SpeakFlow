import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface Capability {
  identifier: string;
  windows: string[];
  permissions: string[];
}

interface TauriConfig {
  app: {
    windows: Array<{ label: string }>;
    security: {
      capabilities: string[];
    };
  };
}

function walkSourceFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of readdirSync(resolve(dir), { withFileTypes: true })) {
    const rel = join(dir, entry.name).replace(/\\/g, "/");
    if (entry.isDirectory()) {
      results.push(...walkSourceFiles(rel));
    } else if (/\.(ts|tsx)$/.test(entry.name) && !/\/__tests__\/|\.(test|spec)\.(ts|tsx)$/.test(rel)) {
      results.push(rel);
    }
  }
  return results;
}

function readJson<T>(relativePath: string): T {
  return JSON.parse(readFileSync(resolve(relativePath), "utf8")) as T;
}

describe("Tauri window capability boundaries", () => {
  it("keeps the subtitle window event-only", () => {
    const subtitle = readJson<Capability>(
      "src-tauri/capabilities/subtitle.json",
    );

    expect(subtitle.identifier).toBe("subtitle");
    expect(subtitle.windows).toEqual(["subtitle"]);
    expect(subtitle.permissions).toEqual(["core:event:default"]);
  });

  it("grants the main window every autostart operation used by settings", () => {
    const main = readJson<Capability>("src-tauri/capabilities/main.json");

    expect(main.identifier).toBe("main");
    expect(main.windows).toEqual(["main"]);
    expect(main.permissions).toEqual(
      expect.arrayContaining([
        "autostart:allow-enable",
        "autostart:allow-disable",
        "autostart:allow-is-enabled",
      ]),
    );
  });

  it("grants every getCurrentWindow() operation called across src/ in its window capability file", () => {
    const capabilitiesByWindow = new Map<string, string[]>();
    for (const file of readdirSync(resolve("src-tauri/capabilities"))) {
      if (!file.endsWith(".json")) continue;
      const cap = readJson<Capability>(`src-tauri/capabilities/${file}`);
      for (const win of cap.windows) {
        capabilitiesByWindow.set(win, cap.permissions);
      }
    }
    const permissionFor: Record<string, string> = {
      hide: "core:window:allow-hide",
      minimize: "core:window:allow-minimize",
      toggleMaximize: "core:window:allow-toggle-maximize",
      startDragging: "core:window:allow-start-dragging",
      startResizeDragging: "core:window:allow-start-resize-dragging",
    };
    const entryFiles: Record<string, string> = {
      "src/pages/SelectionOverlay.tsx": "selection-toolbar",
      "src/pages/SubtitleOverlay.tsx": "subtitle",
      "src/pages/RecordingPill.tsx": "recording-pill",
    };
    for (const entryPath of Object.keys(entryFiles)) {
      expect(existsSync(resolve(entryPath)), `missing window entry file ${entryPath}`).toBe(true);
    }
    const windowForFile = (rel: string): string => {
      if (rel in entryFiles) return entryFiles[rel];
      if (rel.startsWith("src/features/selection-assistant/")) return "selection-toolbar";
      if (rel.startsWith("src/pages/subtitle/")) return "subtitle";
      return "main";
    };

    let checkedCalls = 0;
    for (const file of walkSourceFiles("src")) {
      const source = readFileSync(resolve(file), "utf8");
      const matches = [...source.matchAll(/getCurrentWindow\(\)\s*\.(\w+)/g)].map((m) => m[1]);
      for (const method of matches) {
        if (method === "label") continue;
        checkedCalls += 1;
        const permission = permissionFor[method];
        const targetWindow = windowForFile(file);
        const granted = capabilitiesByWindow.get(targetWindow) ?? [];
        expect(permission, `${file}: unmapped getCurrentWindow().${method}`).toBeDefined();
        expect(granted, `${file} (${targetWindow}) missing ${permission}`).toContain(permission);
      }
    }
    expect(checkedCalls).toBeGreaterThan(0);
  });

  it("keeps the selection toolbar isolated while allowing its explicit window-hide fallback", () => {
    const selection = readJson<Capability>(
      "src-tauri/capabilities/selection.json",
    );

    expect(selection.identifier).toBe("selection");
    expect(selection.windows).toEqual(["selection-toolbar"]);
    expect(selection.permissions).toEqual(
      expect.arrayContaining([
        "core:event:default",
        "core:window:default",
        "core:window:allow-hide",
      ]),
    );
    expect(selection.permissions).not.toEqual(
      expect.arrayContaining([
        "clipboard-manager:default",
        "keyring:default",
      ]),
    );
  });

  it("loads every window capability and covers every non-main window in ipc_policy.rs", () => {
    const config = readJson<TauriConfig>("src-tauri/tauri.conf.json");
    const capabilityFiles = readdirSync(resolve("src-tauri/capabilities")).filter((f) => f.endsWith(".json"));
    const allWindows = new Set<string>(config.app.windows.map((w) => w.label));
    const capabilityIds: string[] = [];

    for (const file of capabilityFiles) {
      const cap = readJson<Capability>(`src-tauri/capabilities/${file}`);
      capabilityIds.push(cap.identifier);
      for (const win of cap.windows) allWindows.add(win);
    }

    expect([...config.app.security.capabilities].sort()).toEqual(capabilityIds.sort());

    const ipcPolicy = readFileSync(resolve("src-tauri/src/ipc_policy.rs"), "utf8");
    const testLoopMatch = ipcPolicy.match(/for label in \[([^\]]+)\]/);
    expect(testLoopMatch).not.toBeNull();
    const testedLabels = [...testLoopMatch![1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);

    for (const win of allWindows) {
      expect(ipcPolicy, `ipc_policy.rs missing match arm for window "${win}"`).toContain(`"${win}" =>`);
      if (win !== "main") {
        expect(testedLabels, `ipc_policy.rs test matrix missing non-main window "${win}"`).toContain(win);
      }
    }
  });
});
