import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function readJson(path: string) {
  return JSON.parse(readFileSync(resolve(path), "utf8"));
}

describe("recording pill contract", () => {
  it("defines a click-through, non-activating overlay window", () => {
    const config = readJson("src-tauri/tauri.conf.json");
    const pill = config.app.windows.find(
      (window: { label: string }) => window.label === "recording-pill",
    );
    expect(pill).toMatchObject({
      alwaysOnTop: true,
      skipTaskbar: true,
      focus: false,
      focusable: false,
      decorations: false,
      transparent: true,
      shadow: false,
      visible: false,
      url: "/?window=pill",
    });
    expect(config.app.security.capabilities).toContain("pill");
  });

  it("gives the pill only event access", () => {
    const capabilities = readJson("src-tauri/capabilities/pill.json");
    expect(capabilities.windows).toEqual(["recording-pill"]);
    expect(capabilities.permissions).toEqual(["core:event:default"]);
  });

  it("shows the pill from the same recording-state event as the tray mark", () => {
    const source = readFileSync(resolve("src-tauri/src/lib.rs"), "utf8");
    expect(source).toMatch(
      /listen\("recording-state"[\s\S]*update_recording_pill\(&app, active\)/,
    );
  });

  it("lets the pill read the current snapshot and nothing else", () => {
    const source = readFileSync(resolve("src-tauri/src/ipc_policy.rs"), "utf8");
    expect(source).toMatch(
      /"recording-pill"\s*=>\s*matches!\(command,\s*"get_recording_snapshot"\)/,
    );
  });
});
