/**
 * The behavior, scripting, custom-model, search, and meeting wrappers are thin
 * `invoke` shims, but their command names and argument casing are the contract
 * with Rust. A rename or a snake_case slip would fail silently at runtime, so
 * each wrapper is asserted against the exact `invoke` call it should make.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));

vi.mock("@tauri-apps/api/core", () => invokeMock);

vi.mock("@tauri-apps/plugin-autostart", () => ({
  disable: vi.fn(),
  enable: vi.fn(),
  isEnabled: vi.fn(),
}));

beforeEach(() => {
  invokeMock.invoke.mockReset();
  invokeMock.invoke.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

async function api() {
  return await import("@/api/tauri");
}

function lastCall() {
  const calls = invokeMock.invoke.mock.calls;
  expect(calls.length).toBeGreaterThan(0);
  const call = calls[calls.length - 1];
  return { command: call[0], args: call[1] };
}

describe("behavior settings commands", () => {
  it("sends audio behavior as a single camelCase payload", async () => {
    const { setAudioBehavior } = await api();
    const audioBehavior = {
      duckingMode: "duck" as const,
      duckVolume: 0.25,
      speakersOnly: true,
      avoidBluetoothMicrophone: true,
      releaseMicWhileIdle: false,
      preRollEnabled: true,
      adaptiveReleaseTail: true,
    };

    await setAudioBehavior(audioBehavior);

    expect(lastCall()).toEqual({ command: "set_audio_behavior", args: { audioBehavior } });
  });

  it("sends output, shortcuts, and labs payloads under their own keys", async () => {
    const { setOutputBehavior, setShortcutsBehavior, setLabsConfig } = await api();
    const outputBehavior = {
      pasteWithShift: true,
      autoTerminalCtrlShiftV: true,
      keepOnClipboard: false,
      copyOnly: false,
    };
    const shortcutsBehavior = {
      secondDictationHotkey: "Ctrl+Shift+D",
      terminalPasteHotkey: null,
      typedTextHotkey: null,
      meetingHotkey: null,
      shareDictationShortcut: true,
    };
    const labs = { failureNotices: false } as Parameters<typeof setLabsConfig>[0];

    await setOutputBehavior(outputBehavior);
    expect(lastCall()).toEqual({ command: "set_output_behavior", args: { outputBehavior } });

    await setShortcutsBehavior(shortcutsBehavior);
    expect(lastCall()).toEqual({ command: "set_shortcuts_behavior", args: { shortcutsBehavior } });

    await setLabsConfig(labs);
    expect(lastCall()).toEqual({ command: "set_labs_config", args: { labs } });
  });

  it("sends keyboard switching and global transformations", async () => {
    const { setKeyboardModelSwitching, setGlobalTransformations } = await api();
    const rules = [{ layoutTag: "zh", engine: "qwen3-asr-1.7b" }];

    await setKeyboardModelSwitching(true, rules);
    expect(lastCall()).toEqual({
      command: "set_keyboard_model_switching",
      args: { enabled: true, rules },
    });

    await setGlobalTransformations(["spoken_symbols"]);
    expect(lastCall()).toEqual({
      command: "set_global_transformations",
      args: { transformations: ["spoken_symbols"] },
    });
  });

  it("reads the active keyboard locale with no arguments", async () => {
    const { getActiveKeyboardLocale } = await api();
    invokeMock.invoke.mockResolvedValueOnce("zh-Hans-CN");

    await expect(getActiveKeyboardLocale()).resolves.toBe("zh-Hans-CN");
    expect(invokeMock.invoke).toHaveBeenCalledWith("get_active_keyboard_locale");
  });
});

describe("scripting commands", () => {
  it("maps the config source and transform test payloads", async () => {
    const { saveScriptConfig, testScriptTransformations } = await api();

    await saveScriptConfig("export default {}");
    expect(lastCall()).toEqual({ command: "save_script_config", args: { source: "export default {}" } });

    await testScriptTransformations({
      source: "export default {}",
      transformIds: ["camelCase"],
      text: "hello world",
      appProcess: "Code.exe",
      windowTitle: "main.ts",
      domain: null,
    });
    expect(lastCall()).toEqual({
      command: "test_script_transformations",
      args: {
        source: "export default {}",
        transformIds: ["camelCase"],
        text: "hello world",
        appProcess: "Code.exe",
        windowTitle: "main.ts",
        domain: null,
      },
    });
  });

  it("exposes the no-argument scripting reads", async () => {
    const { getScriptingStatus, scaffoldScriptConfig, readScriptConfig } = await api();

    await getScriptingStatus();
    expect(lastCall().command).toBe("get_scripting_status");

    await scaffoldScriptConfig();
    expect(lastCall().command).toBe("scaffold_script_config");

    await readScriptConfig();
    expect(lastCall().command).toBe("read_script_config");
  });
});

describe("output fix commands", () => {
  it("submits a fix with the exact camelCase fields", async () => {
    const { submitOutputFix } = await api();

    await submitOutputFix({
      original: "teh cat",
      fixed: "the cat",
      rawOriginal: "teh kat",
      note: "homophone",
      sessionId: 42,
      processName: "Code.exe",
      windowTitle: "main.ts",
    });

    expect(lastCall()).toEqual({
      command: "submit_output_fix",
      args: {
        original: "teh cat",
        fixed: "the cat",
        rawOriginal: "teh kat",
        note: "homophone",
        sessionId: 42,
        processName: "Code.exe",
        windowTitle: "main.ts",
      },
    });
  });

  it("exposes the no-argument output commands", async () => {
    const { getLastDictationRecord, listOutputFixes, pasteLastAsTerminal, registerExtendedHotkeys } =
      await api();

    await getLastDictationRecord();
    expect(lastCall().command).toBe("get_last_dictation_record");

    await listOutputFixes();
    expect(lastCall().command).toBe("list_output_fixes");

    await pasteLastAsTerminal();
    expect(lastCall().command).toBe("paste_last_as_terminal");

    await registerExtendedHotkeys();
    expect(lastCall().command).toBe("register_extended_hotkeys");
  });
});

describe("custom model commands", () => {
  it("inspects, adds, and removes GGUF models", async () => {
    const { inspectGgufModel, addCustomModel, removeCustomModel } = await api();

    await inspectGgufModel("C:/models/a.gguf");
    expect(lastCall()).toEqual({ command: "inspect_gguf_model", args: { path: "C:/models/a.gguf" } });

    await addCustomModel({
      path: "C:/models/a.gguf",
      name: "A",
      source: "local_gguf",
      repoId: null,
    });
    expect(lastCall()).toEqual({
      command: "add_custom_model",
      args: { path: "C:/models/a.gguf", name: "A", source: "local_gguf", repoId: null },
    });

    await removeCustomModel("custom:a");
    expect(lastCall()).toEqual({ command: "remove_custom_model", args: { id: "custom:a" } });
  });

  it("exposes the no-argument model reads", async () => {
    const { listCustomModels, scanHfCacheModels, pickGgufFile } = await api();

    await listCustomModels();
    expect(lastCall().command).toBe("list_custom_models");

    await scanHfCacheModels();
    expect(lastCall().command).toBe("scan_hf_cache_models");

    await pickGgufFile();
    expect(lastCall().command).toBe("pick_gguf_file");
  });
});

describe("search and meeting commands", () => {
  it("passes the transcript search filter through", async () => {
    const { searchTranscripts } = await api();

    await searchTranscripts("budget", "meeting", 10);

    expect(lastCall()).toEqual({
      command: "search_transcripts",
      args: { query: "budget", kind: "meeting", limit: 10 },
    });
  });

  it("maps the meeting lifecycle commands", async () => {
    const { startMeetingRecording, getMeeting, deleteMeeting, exportMeeting, reprocessMeeting, setMeetingsConfig } =
      await api();

    await startMeetingRecording("Standup", "Teams.exe");
    expect(lastCall()).toEqual({
      command: "start_meeting_recording",
      args: { title: "Standup", appName: "Teams.exe" },
    });

    await getMeeting("m1");
    expect(lastCall()).toEqual({ command: "get_meeting", args: { id: "m1" } });

    await deleteMeeting("m1");
    expect(lastCall()).toEqual({ command: "delete_meeting", args: { id: "m1" } });

    await exportMeeting("m1", "srt");
    expect(lastCall()).toEqual({
      command: "export_meeting",
      args: { id: "m1", format: "srt", saveToDisk: false },
    });

    await reprocessMeeting("m1");
    expect(lastCall()).toEqual({ command: "reprocess_meeting", args: { id: "m1" } });

    const meetings = { keepAudioDays: 7 } as Parameters<typeof setMeetingsConfig>[0];
    await setMeetingsConfig(meetings);
    expect(lastCall()).toEqual({ command: "set_meetings_config", args: { meetings } });
  });

  it("exposes the no-argument meeting reads", async () => {
    const { getMeetingStatus, detectMeetingCall, stopMeetingRecording, listMeetings } = await api();

    await getMeetingStatus();
    expect(lastCall().command).toBe("get_meeting_status");

    await detectMeetingCall();
    expect(lastCall().command).toBe("detect_meeting_call");

    await stopMeetingRecording();
    expect(lastCall().command).toBe("stop_meeting_recording");

    await listMeetings();
    expect(lastCall().command).toBe("list_meetings");
  });
});
