import { act, fireEvent, render as renderUI, screen, waitFor, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { AppUpdateProvider } from "@/contexts/AppUpdateContext";
const render = (ui: ReactElement) => renderUI(ui, { wrapper: ({ children }) => <AppUpdateProvider automatic={false}>{children}</AppUpdateProvider> });import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { INPUT_DEVICE_STORAGE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY } from "@/lib/constants";
import type { UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  addCustomProvider: vi.fn(),
  addHotWord: vi.fn(),
  checkAppUpdate: vi.fn(),
  completeGrokBuildOauthDeviceCode: vi.fn(),
  completeOpenaiCodexOauthDeviceCode: vi.fn(),
  copyToClipboard: vi.fn(),
  disableAutostart: vi.fn(),
  enableAutostart: vi.fn(),
  exportUserProfile: vi.fn(),
  getAiPolishApiKey: vi.fn(),
  getAlibabaAsrConfig: vi.fn(),
  getAssistantApiKey: vi.fn(),
  getEngine: vi.fn(),
  getGpuIdleSeconds: vi.fn(),
  setGpuIdleSeconds: vi.fn(),
  getLlmReasoningSupport: vi.fn(),
  getModelsDir: vi.fn(),
  getOnlineAsrApiKey: vi.fn(),
  getOnlineAsrEndpoint: vi.fn(),
  getGrokBuildOauthStatus: vi.fn(),
  getOpenaiCodexOauthStatus: vi.fn(),
  getClaudeOauthStatus: vi.fn(),
  getAiPolishConfig: vi.fn(),
  detectMagpie: vi.fn(),
  startClaudeOauth: vi.fn(),
  completeClaudeOauth: vi.fn(),
  syncClaudeOauth: vi.fn(),
  getUserProfile: vi.fn(),
  getWebSearchApiKey: vi.fn(),
  hideMainWindow: vi.fn(),
  importUserProfile: vi.fn(),
  isAutostartEnabled: vi.fn(),
  listAiModels: vi.fn(),
  listAlibabaAsrModels: vi.fn(),
  listInstalledModels: vi.fn(),
  getVoiceProcessing: vi.fn(),
  setSilenceFilter: vi.fn(),
  setRemoveFillers: vi.fn(),
  getModelDirs: vi.fn(),
  setModelDirs: vi.fn(),
  testTranscription: vi.fn(),
  listInputDevices: vi.fn(),
  loginGrokBuildOauth: vi.fn(),
  loginOpenaiCodexOauth: vi.fn(),
  logoutGrokBuildOauth: vi.fn(),
  logoutOpenaiCodexOauth: vi.fn(),
  openAppReleasePage: vi.fn(),
  pasteText: vi.fn(),
  pickFolder: vi.fn(),
  removeCorrection: vi.fn(),
  removeCustomProvider: vi.fn(),
  removeHotWord: vi.fn(),
  setAiPolishConfig: vi.fn(),
  setAiPolishScreenContextEnabled: vi.fn(),
  setScreenContextEnabled: vi.fn(),
  setAlibabaAsrModel: vi.fn(),
  setAssistantApiKey: vi.fn(),
  setAssistantHotkey: vi.fn(),
  setAssistantScreenContextEnabled: vi.fn(),
  setAssistantSystemPrompt: vi.fn(),
  setCorrectionValidationConfig: vi.fn(),
  setCustomPrompt: vi.fn(),
  setEngine: vi.fn(),
  setInputDevice: vi.fn(),
  setInputMethodCommand: vi.fn(),
  setLlmProviderConfig: vi.fn(),
  setModelsDir: vi.fn(),
  setOnlineAsrApiKey: vi.fn(),
  setOnlineAsrEndpoint: vi.fn(),
  setOpenaiFastMode: vi.fn(),
  setRecordingMode: vi.fn(),
  setSelectionAssistantConfig: vi.fn(),
  setSoundEnabled: vi.fn(),
  setTranslationHotkey: vi.fn(),
  setTranslationTarget: vi.fn(),
  setWebSearchApiKey: vi.fn(),
  setWebSearchConfig: vi.fn(),
  startMicrophoneLevelMonitor: vi.fn(),
  startGrokBuildOauthDeviceCode: vi.fn(),
  startOpenaiCodexOauthDeviceCode: vi.fn(),
  stopMicrophoneLevelMonitor: vi.fn(),
  testMicrophone: vi.fn(),
  validateCorrections: vi.fn(),
}));

const appMock = vi.hoisted(() => ({
  getVersion: vi.fn(),
}));

const eventMock = vi.hoisted(() => ({
  listen: vi.fn(),
}));

const eventDisposers = vi.hoisted(() => [] as Array<ReturnType<typeof vi.fn>>);

const recordingContextMock = vi.hoisted(() => ({
  retryModel: vi.fn(),
  setHotkey: vi.fn(),
  stage: "ready" as string,
  modelError: null as string | null,
}));

const storageMock = vi.hoisted(() => ({
  readLocalStorage: vi.fn(),
  writeLocalStorage: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
}));

vi.mock("@/api/tauri", () => tauriMock);
vi.mock("@tauri-apps/api/app", () => appMock);
vi.mock("@tauri-apps/api/event", () => eventMock);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ minimize: vi.fn() }),
}));
vi.mock("@/contexts/RecordingContext", () => ({
  useRecordingContext: () => ({
    hotkeyDiagnostic: null,
    hotkeyDisplay: "F2",
    hotkeyError: null,
    isRecording: false,
    retryModel: recordingContextMock.retryModel,
    setHotkey: recordingContextMock.setHotkey,
    stage: recordingContextMock.stage,
    modelError: recordingContextMock.modelError,
  }),
}));
vi.mock("@/hooks/useTheme", () => ({
  useTheme: () => ({ isDark: false, setTheme: vi.fn(), theme: "light" }),
}));
vi.mock("@/lib/storage", () => storageMock);
vi.mock("sonner", () => ({ toast: toastMock }));

const labels: Record<string, string> = {
  "common.add": "Add",
  "common.cancel": "Cancel",
  "common.clear": "Clear",
  "common.close": "Close",
  "common.copy": "Copy",
  "common.change": "Change",
  "common.refresh": "Refresh",
  "common.test": "Test",
  "settings.addCustomProvider": "Add Custom Provider",
  "settings.alsoSystemDefault": "Also the system default",
  "settings.alibabaApiKeyPlaceholder": "Alibaba API Key",
  "settings.alibabaAsrDesc": "Alibaba cloud speech recognition",
  "settings.alibabaAsrLabel": "Alibaba DashScope",
  "settings.apiFormatLabel": "API Format",
  "settings.apiKey": "API Key",
  "settings.assistantApiKey": "Assistant API Key",
  "settings.assistantModelLabel": "Assistant model name",
  "settings.assistantProvider": "Assistant Provider",
  "settings.assistantSeparateConfig": "Assistant uses separate config",
  "settings.baseUrlLabel": "Base URL",
  "settings.defaultModelLabel": "Provider default model",
  "settings.fetching": "Fetching...",
  "settings.fixedMic": "Use this microphone exclusively",
  "settings.followSystemMic": "Use system default microphone",
  "settings.glmApiKeyPlaceholder": "GLM API Key",
  "settings.glmAsrDesc": "GLM cloud speech recognition",
  "settings.speechProcessing": "Speech processing",
  "settings.silenceFilter": "Skip silence before recognition",
  "settings.silenceFilterDesc": "Trims silence with the voice detector before the model runs.",
  "settings.removeFillers": "Remove filler words",
  "settings.removeFillersDesc": "Drops um, uh and similar sounds from the finished transcript.",
  "settings.modelFolders": "Model folders",
  "settings.modelFoldersDesc": "Extra read-only folders searched for models.",
  "settings.modelFoldersEmpty": "No extra folders.",
  "settings.addModelFolder": "Add folder",
  "settings.removeModelFolder": "Remove {{path}}",
  "settings.testModel": "Test model",
  "settings.testModelRecording": "Recording…",
  "settings.testModelHint": "Records 5 seconds and shows what the model hears.",
  "settings.testModelSilence": "No speech detected in that clip.",
  "settings.engineNeedsDownload": "{{engine}} is not downloaded yet. Downloading starts on the Dictation tab.",
  "settings.engineNeedsRepair": "The {{engine}} model file is damaged. Repair it on the Dictation tab.",
  "settings.engineUnavailable": "{{engine}} cannot run: {{error}}",
  "settings.engineNeedsKey": "Add an API key below to use {{engine}}.",
  "toast.settingSaveFailed": "Could not save that setting.",
  "settings.browseModels": "Browse models",
  "settings.allLanguages": "All languages",
  "settings.filterByLanguage": "Filter by language",
  "settings.searchModels": "Search models",
  "settings.searchModelsPlaceholder": "Search by name or description",
  "settings.modelCount": "{{count}} models",
  "settings.modelRecommended": "Recommended",
  "settings.modelTranslate": "Translate",
  "settings.modelSelected": "Selected",
  "settings.modelUse": "Use",
  "settings.modelDownload": "Download",
  "settings.modelErrorRate": "({{percent}}% error)",
  "settings.modelSpeed": "Speed: {{seconds}} / 30 s",
  "settings.modelLanguageCount_one": "{{count}} language",
  "settings.modelLanguageCount_other": "{{count}} languages",
  "settings.modelNoResults": "No model matches this language and search.",
  "settings.modelBrowserHint": "Benchmarks come from Handy.",
  "settings.otherEngines": "Other engines",
  "settings.engine": "ASR Engine",
  "settings.currentDefault": "System default: {{name}}",
  "settings.micLevelMonitor": "Microphone Level Monitor",
  "settings.micMonitorOff": "Microphone level monitor is off",
  "settings.micNotStarted": "Microphone level monitor not started",
  "settings.micRecordingPaused": "Microphone monitoring is paused while recording",
  "settings.micSpeakToTest": "Speak into the microphone to see level changes",
  "settings.microphone": "Microphone",
  "settings.modelStorageDir": "Model directory",
  "settings.modelNameLabel": "Model name",
  "settings.autoUseDefault": "Automatically use system default input device",
  "settings.canSelect": "Select this microphone",
  "settings.qwen3Asr06Desc": "Fast local speech recognition",
  "settings.r2t2Desc": "Higher accuracy local speech recognition",
  "settings.restoreDefault": "Restore default",
  "settings.savedMicUnavailable": "Saved microphone is unavailable",
  "settings.selectMic": "Select Microphone",
  "settings.systemDefaultDevice": "System default",
  "settings.openAssistantModelList": "Open assistant model list",
  "settings.openModelList": "Open model list",
  "settings.providerBaseUrlLabel": "Provider Base URL",
  "settings.providerNameLabel": "Provider name",
  "settings.searchAssistantModel": "Search assistant model",
  "settings.searchAssistantProviderLabel": "Search assistant provider",
  "settings.searchModelLabel": "Search model",
  "settings.searchProviderLabel": "Search provider",
  "settings.selectProvider": "Select LLM Provider",
  "settings.showApiKey": "Show API Key",
  "settings.useSeparateConfig": "Use Separate Config",
  "toast.micSwitchFailed": "Failed to switch microphone",
  "toast.modelsDirUpdated": "Model directory updated",
  "toast.switchEngineFailed": "Failed to switch engine",
  "toast.switchedToEngine": "Switched to {{label}} engine",
};

function translate(key: string, options?: Record<string, unknown>) {
  const template = labels[key] ?? key;
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => (
    options?.[name] === undefined ? `{{${name}}}` : String(options[name])
  ));
}

vi.mock("@/i18n", () => ({
  default: {
    changeLanguage: vi.fn(),
    language: "en",
    t: translate,
  },
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { init: vi.fn(), type: "3rdParty" },
  useTranslation: () => ({
    i18n: { changeLanguage: vi.fn(), language: "en" },
    t: translate,
  }),
}));

const baseProfile: UserProfile = {
  blocked_hot_words: [],
  correction_patterns: [],
  correction_validation_enabled: false,
  custom_prompt: null,
  hot_words: [],
  last_correction_validation: 0,
  last_updated: 0,
  llm_provider: {
    active: "cerebras",
    custom_providers: [],
  },
  total_transcriptions: 0,
  translation_hotkey: null,
  translation_target: null,
  vocab_frequency: {},
  web_search: {
    enabled: false,
    max_results: 5,
    provider: "model_native",
  },
};

function resetMocks(profile: UserProfile = baseProfile) {
  for (const mock of Object.values(tauriMock)) {
    mock.mockReset();
    mock.mockResolvedValue(undefined);
  }
  tauriMock.getUserProfile.mockResolvedValue(profile);
  tauriMock.getAiPolishApiKey.mockResolvedValue("");
  tauriMock.getAssistantApiKey.mockResolvedValue("");
  tauriMock.getAlibabaAsrConfig.mockResolvedValue({
    model: "qwen3-asr-flash",
    models: ["qwen3-asr-flash"],
    region: "international",
    url: "https://dashscope-intl.aliyuncs.com",
  });
  tauriMock.getEngine.mockResolvedValue("qwen3-asr-0.6b");
  tauriMock.getGpuIdleSeconds.mockResolvedValue(0);
  tauriMock.getLlmReasoningSupport.mockResolvedValue({
    strategy: null,
    summary: "reasoning unavailable",
    supported: false,
  });
  tauriMock.getModelsDir.mockResolvedValue({
    is_custom: false,
    path: "C:\\Users\\khent\\.cache\\speak-flow-models",
  });
  tauriMock.getOnlineAsrApiKey.mockResolvedValue("");
  tauriMock.getOnlineAsrEndpoint.mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  tauriMock.getGrokBuildOauthStatus.mockResolvedValue({ loggedIn: false });
  tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: false });
  tauriMock.getClaudeOauthStatus.mockResolvedValue({ loggedIn: false });
  tauriMock.getAiPolishConfig.mockResolvedValue({ enabled: false, provider: "cerebras" });
  tauriMock.detectMagpie.mockResolvedValue({ available: false, running: false, gatewayUrl: "http://127.0.0.1:3425/v1" });
  tauriMock.isAutostartEnabled.mockResolvedValue(false);
  tauriMock.listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  tauriMock.listAlibabaAsrModels.mockResolvedValue({
    models: ["qwen3-asr-flash"],
    source: "fallback",
  });
  tauriMock.listInstalledModels.mockResolvedValue([]);
  tauriMock.getVoiceProcessing.mockResolvedValue({ silenceFilter: true, removeFillers: true });
  tauriMock.getModelDirs.mockResolvedValue([]);
  tauriMock.listInputDevices.mockResolvedValue({ devices: [], selectedDeviceName: null });
  tauriMock.pickFolder.mockResolvedValue(null);
  tauriMock.setOnlineAsrEndpoint.mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  tauriMock.setAiPolishConfig.mockResolvedValue(undefined);
  tauriMock.setLlmProviderConfig.mockResolvedValue(undefined);
  tauriMock.addCustomProvider.mockResolvedValue("custom-provider");
  appMock.getVersion.mockReset();
  appMock.getVersion.mockResolvedValue("1.5.9");
  eventMock.listen.mockReset();
  eventDisposers.length = 0;
  eventMock.listen.mockImplementation(() => {
    const disposer = vi.fn();
    eventDisposers.push(disposer);
    return Promise.resolve(disposer);
  });
  recordingContextMock.retryModel.mockReset();
  recordingContextMock.stage = "ready";
  recordingContextMock.modelError = null;
  recordingContextMock.setHotkey.mockReset();
  recordingContextMock.setHotkey.mockResolvedValue(undefined);
  storageMock.readLocalStorage.mockReset();
  storageMock.readLocalStorage.mockReturnValue(null);
  storageMock.writeLocalStorage.mockReset();
  toastMock.error.mockReset();
  toastMock.info.mockReset();
  toastMock.success.mockReset();
  Object.defineProperty(window, "IntersectionObserver", {
    configurable: true,
    writable: true,
    value: class {
      disconnect() {}
      observe() {}
      unobserve() {}
    },
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value: vi.fn(),
  });
}

async function renderSettings(profile: UserProfile = baseProfile, active = true) {
  tauriMock.getUserProfile.mockReset();
  tauriMock.getUserProfile.mockResolvedValue(profile);
  const { default: SettingsPage } = await import("@/pages/SettingsPage");
  const rendered = render(<SettingsPage active={active} onNavigate={vi.fn()} />);
  await waitFor(() => expect(tauriMock.getUserProfile).toHaveBeenCalledWith());
  await waitFor(() => expect(tauriMock.getEngine).toHaveBeenCalled());
  return rendered;
}

beforeEach(() => resetMocks());
afterEach(() => vi.clearAllMocks());

describe("SettingsPage ASR and device settings", () => {
  it("selects the final navigation section when manually scrolling to the bottom", async () => {
    const { container } = await renderSettings();
    const content = container.querySelector<HTMLElement>(".settings-content")!;
    Object.defineProperties(content, {
      scrollHeight: { configurable: true, value: 2400 },
      clientHeight: { configurable: true, value: 600 },
    });
    content.scrollTop = 1800;
    fireEvent.scroll(content);
    await waitFor(() => expect(container.querySelector('[data-nav-tab="startup"]')).toHaveAttribute("data-active", "true"));
  });
  it("loads the persisted engine, microphone, and model directory and applies a directory change", async () => {
    const defaultModelsDir = "C:\\speak-flow\\models";
    const customModelsDir = "D:\\speak-flow\\models";
    tauriMock.getEngine.mockResolvedValue("confucius4-r2t2");
    tauriMock.getModelsDir
      .mockResolvedValueOnce({ path: defaultModelsDir, is_custom: false })
      .mockResolvedValueOnce({ path: customModelsDir, is_custom: true });
    tauriMock.listInputDevices.mockResolvedValue({
      devices: [
        { name: "Built-in Microphone", isDefault: true },
        { name: "USB Microphone", isDefault: false },
      ],
      selectedDeviceName: "USB Microphone",
    });
    tauriMock.pickFolder.mockResolvedValue(customModelsDir);
    tauriMock.setModelsDir.mockResolvedValue({ runtimeWarning: null });

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).toHaveTextContent("Confucius4-R2T2 Q8"));
    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).toHaveTextContent("USB Microphone"));
    expect(screen.getByText(defaultModelsDir)).toBeInTheDocument();

    fireEvent.click(within(engineSection!).getByRole("button", { name: "Change" }));

    await waitFor(() => {
      expect(tauriMock.pickFolder).toHaveBeenCalledWith();
      expect(tauriMock.setModelsDir).toHaveBeenCalledWith(customModelsDir, true);
      expect(screen.getByText(customModelsDir)).toBeInTheDocument();
    });
    expect(tauriMock.getModelsDir).toHaveBeenCalledTimes(2);
    expect(toastMock.success).toHaveBeenCalledWith("Model directory updated");
  });

  it("switches to an online engine and refreshes its endpoint and key before retrying the model", async () => {
    tauriMock.getOnlineAsrApiKey
      .mockResolvedValueOnce("local-engine-key")
      .mockResolvedValue("glm-engine-key");
    tauriMock.getOnlineAsrEndpoint
      .mockResolvedValueOnce({ region: "international", url: "https://local.example" })
      .mockResolvedValue({ region: "international", url: "https://glm.example" });
    tauriMock.setEngine.mockResolvedValue("glm-asr");

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).not.toBeDisabled());
    fireEvent.click(engineTrigger);
    const engineList = await screen.findByRole("listbox", { name: "Browse models" });
    fireEvent.click(within(engineList).getByRole("option", { name: /GLM-ASR/ }));

    await waitFor(() => {
      expect(tauriMock.setEngine).toHaveBeenCalledWith("glm-asr");
      expect(engineTrigger).toHaveTextContent("GLM-ASR");
      expect(recordingContextMock.retryModel).toHaveBeenCalledTimes(1);
    });
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(2);
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect(screen.getByPlaceholderText("GLM API Key")).toHaveValue("glm-engine-key");
    expect(screen.getByText("https://glm.example")).toBeInTheDocument();
    expect(toastMock.success).toHaveBeenCalledWith("Switched to GLM-ASR engine");
  });

  it("keeps the current engine and reports a failed engine switch", async () => {
    tauriMock.setEngine.mockRejectedValueOnce(new Error("engine unavailable"));

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).not.toBeDisabled());
    fireEvent.click(engineTrigger);
    const engineList = await screen.findByRole("listbox", { name: "Browse models" });
    fireEvent.click(within(engineList).getByRole("option", { name: /Confucius4-R2T2 Q8/ }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("Failed to switch engine"));
    expect(tauriMock.setEngine).toHaveBeenCalledWith("confucius4-r2t2");
    expect(engineTrigger).toHaveTextContent("Qwen3-ASR 0.6B");
    expect(recordingContextMock.retryModel).not.toHaveBeenCalled();
  });

  it("persists a selected microphone and reloads the device selection", async () => {
    const devices = [
      { name: "Built-in Microphone", isDefault: true },
      { name: "USB Microphone", isDefault: false },
      { name: "Headset Microphone", isDefault: false },
    ];
    const updatedDevices = devices;
    tauriMock.listInputDevices
      .mockResolvedValueOnce({ devices, selectedDeviceName: "USB Microphone" })
      .mockResolvedValue({ devices: updatedDevices, selectedDeviceName: "Headset Microphone" });

    await renderSettings();

    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).not.toBeDisabled());
    fireEvent.click(microphoneTrigger);
    const deviceList = screen.getByRole("listbox");
    fireEvent.click(within(deviceList).getByRole("option", { name: /^Headset Microphone/ }));

    await waitFor(() => {
      expect(tauriMock.setInputDevice).toHaveBeenCalledWith("Headset Microphone");
      expect(tauriMock.listInputDevices).toHaveBeenCalledTimes(2);
      expect(microphoneTrigger).toHaveTextContent("Headset Microphone");
    });
    expect(storageMock.writeLocalStorage).toHaveBeenCalledWith(
      INPUT_DEVICE_STORAGE_KEY,
      "Headset Microphone",
    );
  });

  it("reports a failed microphone change without replacing the current selection", async () => {
    tauriMock.listInputDevices.mockResolvedValue({
      devices: [
        { name: "Built-in Microphone", isDefault: true },
        { name: "USB Microphone", isDefault: false },
        { name: "Headset Microphone", isDefault: false },
      ],
      selectedDeviceName: "USB Microphone",
    });
    tauriMock.setInputDevice.mockRejectedValueOnce(new Error("device unavailable"));

    await renderSettings();

    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).not.toBeDisabled());
    fireEvent.click(microphoneTrigger);
    fireEvent.click(within(screen.getByRole("listbox")).getByRole("option", { name: /^Headset Microphone/ }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("Failed to switch microphone"));
    expect(tauriMock.setInputDevice).toHaveBeenCalledWith("Headset Microphone");
    expect(tauriMock.listInputDevices).toHaveBeenCalledTimes(1);
    expect(microphoneTrigger).toHaveTextContent("USB Microphone");
    expect(storageMock.writeLocalStorage).not.toHaveBeenCalledWith(
      INPUT_DEVICE_STORAGE_KEY,
      "Headset Microphone",
    );
  });

  it("starts and stops the level monitor with the toggle and on unmount", async () => {
    const rendered = await renderSettings();
    await waitFor(() => expect(tauriMock.stopMicrophoneLevelMonitor).toHaveBeenCalled());

    const monitorToggle = screen.getByRole("switch", { name: "Microphone Level Monitor" });
    await act(async () => {
      fireEvent.click(monitorToggle);
      await Promise.resolve();
    });
    await waitFor(() => expect(tauriMock.startMicrophoneLevelMonitor).toHaveBeenCalledTimes(1));
    expect(storageMock.writeLocalStorage).toHaveBeenCalledWith(
      MIC_LEVEL_MONITOR_ENABLED_KEY,
      "true",
    );

    await act(async () => {
      fireEvent.click(monitorToggle);
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(monitorToggle).toHaveAttribute("aria-checked", "false");
      expect(tauriMock.startMicrophoneLevelMonitor).toHaveBeenCalledTimes(1);
    });

    await waitFor(() => expect(eventDisposers.length).toBeGreaterThanOrEqual(4));
    const disposers = [...eventDisposers];
    const stopCallsBeforeUnmount = tauriMock.stopMicrophoneLevelMonitor.mock.calls.length;
    rendered.unmount();
    await waitFor(() => {
      expect(tauriMock.stopMicrophoneLevelMonitor.mock.calls.length).toBeGreaterThan(stopCallsBeforeUnmount);
      for (const disposer of disposers) {
        expect(disposer).toHaveBeenCalled();
      }
    });
  });

  it("does not start the level monitor while the settings page is inactive", async () => {
    storageMock.readLocalStorage.mockImplementation((key: string) => (
      key === MIC_LEVEL_MONITOR_ENABLED_KEY ? "true" : null
    ));
    const rendered = await renderSettings(baseProfile, false);

    await waitFor(() => expect(tauriMock.stopMicrophoneLevelMonitor).toHaveBeenCalled());
    await waitFor(() => expect(eventDisposers.length).toBeGreaterThanOrEqual(2));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(tauriMock.startMicrophoneLevelMonitor).not.toHaveBeenCalled();

    const disposers = [...eventDisposers];
    const stopCallsBeforeUnmount = tauriMock.stopMicrophoneLevelMonitor.mock.calls.length;
    rendered.unmount();
    await waitFor(() => {
      expect(tauriMock.stopMicrophoneLevelMonitor.mock.calls.length).toBeGreaterThan(stopCallsBeforeUnmount);
      for (const disposer of disposers) {
        expect(disposer).toHaveBeenCalled();
      }
    });
  });
});

describe("SettingsPage speech processing", () => {
  it("loads and saves both processing toggles", async () => {
    tauriMock.getVoiceProcessing.mockResolvedValue({ silenceFilter: true, removeFillers: false });
    tauriMock.setSilenceFilter.mockResolvedValue(false);
    tauriMock.setRemoveFillers.mockResolvedValue(true);

    await renderSettings();

    const silence = await screen.findByRole("switch", { name: "Skip silence before recognition" });
    const fillers = screen.getByRole("switch", { name: "Remove filler words" });
    await waitFor(() => expect(fillers).toHaveAttribute("aria-checked", "false"));

    fireEvent.click(silence);
    await waitFor(() => expect(tauriMock.setSilenceFilter).toHaveBeenCalledWith(false));
    expect(silence).toHaveAttribute("aria-checked", "false");

    fireEvent.click(fillers);
    await waitFor(() => expect(tauriMock.setRemoveFillers).toHaveBeenCalledWith(true));
    expect(fillers).toHaveAttribute("aria-checked", "true");
  });

  it("restores a toggle and reports when saving fails", async () => {
    tauriMock.setSilenceFilter.mockRejectedValueOnce(new Error("disk full"));

    await renderSettings();

    const silence = await screen.findByRole("switch", { name: "Skip silence before recognition" });
    fireEvent.click(silence);
    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("Could not save that setting."));
    expect(silence).toHaveAttribute("aria-checked", "true");
  });

  it("adds and removes a read-only model folder", async () => {
    const folder = String.raw`D:\hex\models`;
    tauriMock.getModelDirs.mockResolvedValue([]);
    tauriMock.pickFolder.mockResolvedValue(folder);
    tauriMock.setModelDirs.mockResolvedValue([folder]);

    await renderSettings();

    fireEvent.click(await screen.findByRole("button", { name: "Add folder" }));
    await waitFor(() => expect(tauriMock.setModelDirs).toHaveBeenCalledWith([folder]));
    expect(await screen.findByText(folder)).toBeInTheDocument();

    tauriMock.setModelDirs.mockResolvedValue([]);
    fireEvent.click(screen.getByRole("button", { name: `Remove ${folder}` }));
    await waitFor(() => expect(tauriMock.setModelDirs).toHaveBeenLastCalledWith([]));
    expect(screen.getByText("No extra folders.")).toBeInTheDocument();
    // The selected model may now be found in (or lost from) the folders.
    expect(recordingContextMock.retryModel).toHaveBeenCalled();
  });

  it("shows the transcript from a model test and explains a missing model", async () => {
    tauriMock.testTranscription.mockResolvedValue({ text: "hello there", language: "en", duration: 5 });
    recordingContextMock.stage = "need_download";

    await renderSettings();

    fireEvent.click(await screen.findByRole("button", { name: "Test model" }));
    await waitFor(() => expect(tauriMock.testTranscription).toHaveBeenCalledWith(5));
    expect(await screen.findByText("hello there")).toBeInTheDocument();
    expect(screen.getByText(/is not downloaded yet/)).toBeInTheDocument();
  });
});
