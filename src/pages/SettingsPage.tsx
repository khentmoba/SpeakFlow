import { lazy, Suspense, useState, useEffect, useCallback, useRef, useMemo } from "react";
import { Mic, Keyboard, ClipboardPaste, AudioLines, BookOpen, Plus, X, ChevronsUpDown, FolderOpen, RotateCcw, HardDrive, AlertTriangle } from "lucide-react";
import { toast } from "sonner";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";
import { useSystemSettings } from "@/hooks/useSystemSettings";
import { useAsrEngineSettings } from "@/hooks/useAsrEngineSettings";
import { useModelDirectorySettings } from "@/hooks/useModelDirectorySettings";
import { useMicrophoneSettings } from "@/hooks/useMicrophoneSettings";
import {
  setInputMethodCommand,
  getUserProfile,
  addHotWord,
  removeHotWord,
  setSoundEnabled,
  setRecordingMode,
  detectMagpie,
  listInstalledModels,
  getVoiceProcessing,
  setSilenceFilter,
  setRemoveFillers,
  getModelDirs,
  setModelDirs,
  pickFolder,
  testTranscription,
} from "@/api/tauri";
import type { MagpieStatus } from "@/api/tauri";
import type { UserProfile } from "@/types";
import { useRecordingContext } from "@/contexts/RecordingContext";
import SecretInput from "@/components/SecretInput";
import Kbd from "@/components/Kbd";
import AppearanceSettingsSection from "@/components/settings/AppearanceSettingsSection";
import AiModelsSettingsSection from "@/components/settings/AiModelsSettingsSection";
import SystemSettingsSections from "@/components/settings/SystemSettingsSections";
import AppProfileRulesSettingsSection from "@/components/settings/AppProfileRulesSettingsSection";
import HistorySettingsSection from "@/components/settings/HistorySettingsSection";
import DictationToolsSettingsSection from "@/components/settings/DictationToolsSettingsSection";
import GpuIdleUnloadControl from "@/components/settings/GpuIdleUnloadControl";
import { SettingsReveal } from "@/components/settings/SettingsReveal";
import R2T2SettingsSection from "@/components/settings/R2T2SettingsSection";
import { ModelBrowser } from "@/components/settings/ModelBrowser";
import { prefersReducedMotion } from "@/lib/motion";
import { INPUT_METHOD_KEY, DEFAULT_HOTKEY, SOUND_ENABLED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import { formatAsrEngineDescription, getAsrEngineCapability } from "@/lib/asrEngineCapabilities";
import {
  llmProviderOptions,
  magpieProviderOption,
} from "@/lib/llmModelOptions";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { useTranslation } from "react-i18next";

import { engineDisplayDescription, engineDisplayLabel } from "@/lib/asrEngineOptions";
import { isOnlineEngineKey, asrCapabilityTagKeys, inputOptions, recordingModeOptions, sourceLabels, sourceColors, findRecordingModeOption } from "@/pages/settings/settingsOptions";

const CorrectionRulesModal = lazy(() => import("@/components/settings/CorrectionRulesModal"));

export default function SettingsPage({
  active,
  animClass = "",
}: {
  onNavigate: (v: "main" | "settings" | "history") => void;
  active: boolean;
  animClass?: string;
}) {
  const pageContentClass = `page-content ${animClass || ""}`.trim();

  const { t } = useTranslation();
  const {
    isRecording,
    retryModel,
    hotkeyDisplay,
    setHotkey,
    hotkeyError,
    hotkeyDiagnostic,
    stage: modelStage,
    modelError,
  } = useRecordingContext();
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [profileLoadError, setProfileLoadError] = useState(false);
  const [profileLoadPending, setProfileLoadPending] = useState(false);
  const settingsReady = profile !== null && !profileLoadError;

  // --- Settings nav sections ---
  const navSections = useMemo(() => [
    { id: "appearance", labelKey: "settings.appearance" },
    { id: "engine", labelKey: "settings.engine" },
    { id: "ai-models", labelKey: "settings.aiModels" },
    { id: "hotkey", labelKey: "settings.hotkeySection" },
    { id: "microphone", labelKey: "settings.microphone" },
    { id: "input", labelKey: "settings.inputMethod" },
    { id: "dictation-tools", labelKey: "settings.dictationTools" },
    { id: "app-profiles", labelKey: "settings.appProfiles" },
    { id: "history-settings", labelKey: "settings.historySettings" },
    { id: "vocabulary", labelKey: "settings.vocabulary" },
    { id: "startup", labelKey: "settings.startup" },
  ] as const, []);
  const [activeNavSection, setActiveNavSection] = useState("appearance");
  const settingsContentRef = useRef<HTMLDivElement | null>(null);
  const navScrollRef = useRef<HTMLDivElement | null>(null);
  const isNavClickScrolling = useRef(false);
  const navScrollTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Measure the scroll position instead of relying on a narrow intersection
  // band that the final sections cannot reach. Keep click intent until scrollend.
  useEffect(() => {
    const container = settingsContentRef.current;
    if (!container) return;
    const sectionEls = navSections
      .map(({ id }) => container.querySelector(`[data-nav-id="${id}"]`))
      .filter(Boolean) as Element[];
    if (sectionEls.length === 0) return;

    let frame = 0;
    const updateSection = () => {
      frame = 0;
      if (isNavClickScrolling.current) return;
      const atBottom = container.scrollHeight > container.clientHeight
        && container.scrollTop + container.clientHeight >= container.scrollHeight - 2;
      const top = container.getBoundingClientRect().top + container.clientHeight * 0.1;
      const current = atBottom ? sectionEls[sectionEls.length - 1] : sectionEls.reduce((selected, section) => (
        section.getBoundingClientRect().top <= top ? section : selected
      ), sectionEls[0]);
      const id = (current as HTMLElement).dataset.navId;
      if (id) setActiveNavSection(id);
    };
    const finishScroll = () => {
      clearTimeout(navScrollTimer.current);
      isNavClickScrolling.current = false;
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(updateSection);
      clearTimeout(navScrollTimer.current);
      // Fallback for WebView versions without scrollend: wait for silence,
      // restarting on every scroll frame instead of guessing journey duration.
      navScrollTimer.current = setTimeout(finishScroll, 180);
    };
    const interruptScroll = () => { finishScroll(); };
    updateSection();
    container.addEventListener("scroll", onScroll, { passive: true });
    container.addEventListener("scrollend", finishScroll);
    container.addEventListener("wheel", interruptScroll, { passive: true });
    container.addEventListener("touchstart", interruptScroll, { passive: true });
    container.addEventListener("keydown", interruptScroll);
    return () => {
      cancelAnimationFrame(frame);
      finishScroll();
      container.removeEventListener("scroll", onScroll);
      container.removeEventListener("scrollend", finishScroll);
      container.removeEventListener("wheel", interruptScroll);
      container.removeEventListener("touchstart", interruptScroll);
      container.removeEventListener("keydown", interruptScroll);
    };
  }, [navSections, settingsReady]);

  const handleNavClick = useCallback((id: string) => {
    const container = settingsContentRef.current;
    if (!container) return;
    const target = container.querySelector(`[data-nav-id="${id}"]`) as HTMLElement | null;
    if (!target) return;
    setActiveNavSection(id);
    isNavClickScrolling.current = true;
    const containerRect = container.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const scrollMarginTop = Number.parseFloat(
      window.getComputedStyle(target).scrollMarginTop,
    ) || 0;
    const top = Math.min(Math.max(0, container.scrollHeight - container.clientHeight), Math.max(
      0,
      container.scrollTop + targetRect.top - containerRect.top - scrollMarginTop,
    ));
    if (Math.abs(container.scrollTop - top) < 1 || prefersReducedMotion()) {
      container.scrollTo({ top, behavior: "instant" });
      isNavClickScrolling.current = false;
      return;
    }
    container.scrollTo({ top, behavior: "smooth" });
  }, []);


  // Auto-scroll the nav bar to keep active tab visible + measure indicator position
  useEffect(() => {
    const navEl = navScrollRef.current;
    if (!navEl || !active) return;
    const updateIndicator = () => {
      const activeBtn = navEl.querySelector(`[data-nav-tab="${activeNavSection}"]`) as HTMLElement | null;
      if (!activeBtn) return;
      const behavior = prefersReducedMotion() ? "instant" : "smooth";
      if (window.getComputedStyle(navEl).flexDirection === "column") {
        navEl.scrollTo({ top: activeBtn.offsetTop - navEl.clientHeight / 2 + activeBtn.clientHeight / 2, behavior });
      } else {
        navEl.scrollTo({ left: activeBtn.offsetLeft - navEl.offsetWidth / 2 + activeBtn.offsetWidth / 2, behavior });
      }
    };
    updateIndicator();
    window.addEventListener("resize", updateIndicator);
    return () => window.removeEventListener("resize", updateIndicator);
  }, [activeNavSection, active, t, settingsReady]);

  // --- Picker group (mutually exclusive dropdowns) ---
  type PickerId = "recordingMode" | "microphone" | "alibabaModel";
  const picker = useExclusivePicker<PickerId>();

  // --- Hotkey capture ---
  const mainHotkeyCapture = useHotkeyCapture({
    save: async (shortcut) => { await setHotkey(shortcut); },
    label: t("settings.hotkeyLabel"),
  });

  // --- Core state ---
  const [recordingMode, setRecordingModeState] = useState<"hold" | "toggle">(() => {
    return readLocalStorage(RECORDING_MODE_KEY) === "toggle" ? "toggle" : "hold";
  });
  const [inputMethod, setInputMethod] = useState<"sendInput" | "clipboard">(() => {
    return readLocalStorage(INPUT_METHOD_KEY) === "clipboard" ? "clipboard" : "sendInput";
  });
  const [soundEnabled, setSoundEnabledState] = useState(() => readLocalStorage(SOUND_ENABLED_KEY) !== "false");
  const [newHotWord, setNewHotWord] = useState("");
  const [browserOpen, setBrowserOpen] = useState(false);
  const [installedModels, setInstalledModels] = useState<ReadonlySet<string>>(() => new Set());
  const [silenceFilter, setSilenceFilterState] = useState(true);
  const [removeFillers, setRemoveFillersState] = useState(true);
  const [modelDirs, setModelDirsState] = useState<string[]>([]);
  const [testRunning, setTestRunning] = useState(false);
  const [testText, setTestText] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  // --- Correction validation ---
  const [validationEnabled, setValidationEnabled] = useState(false);
  const [validationUseSeparateModel, setValidationUseSeparateModel] = useState(false);
  const [validationProvider, setValidationProvider] = useState<string | null>(null);
  const [validationModel, setValidationModel] = useState("");
  const [validationRunning, setValidationRunning] = useState(false);
  const [validationResult, setValidationResult] = useState<string | null>(null);
  const [correctionModalOpen, setCorrectionModalOpen] = useState(false);
  const correctionManageButtonRef = useRef<HTMLButtonElement>(null);

  // 加载用户画像
  const refreshProfile = useCallback(async () => {
    setProfileLoadPending(true);
    try {
      const p = await getUserProfile();
      setProfile(p);
      setProfileLoadError(false);
      setValidationEnabled(Boolean(p.correction_validation_enabled));
      setValidationUseSeparateModel(Boolean(p.llm_provider.validation_use_separate_model));
      setValidationProvider(p.llm_provider.validation_provider ?? null);
      setValidationModel(p.llm_provider.validation_model ?? "");
      return p;
    } catch (error) {
      console.error("Failed to load user profile:", error);
      setProfileLoadError(true);
      setCorrectionModalOpen(false);
      return undefined;
    } finally {
      setProfileLoadPending(false);
    }
  }, []);

  useEffect(() => { void refreshProfile(); }, [refreshProfile]);

  const {
    appVersion,
    autostart,
    autostartLoading,
    autostartMinimized,
    autostartMinimizedLoading,
    handleAutostartToggle,
    handleAutostartMinimizedToggle,
    handleCopyExportPath,
    handleExportConfig,
    exporting,
    handleImportConfig,
    handleTestPaste,
    lastExportPath,
  } = useSystemSettings({ inputMethod, profile, refreshProfile });

  const engineLabel = useCallback(
    (engineValue: string) => engineDisplayLabel(engineValue, t),
    [t],
  );

  const {
    alibabaAsrModel,
    alibabaAsrModels,
    alibabaAsrModelsLoading,
    alibabaAsrModelsSource,
    alibabaHasKey,
    engine,
    engineLoading,
    handleAlibabaAsrModelSelect,
    handleEngineSwitch,
    handleOnlineAsrApiKeyChange,
    handleOnlineAsrRegionChange,
    onlineAsrApiKey,
    onlineAsrRegion,
    onlineAsrRegionLoading,
    onlineAsrUrl,
    refreshAlibabaModels,
  } = useAsrEngineSettings({ engineLabel, retryModel });

  const {
    handleChooseModelsDir,
    handleRestoreDefaultModelsDir,
    modelsDir,
    modelsDirCustom,
    modelsDirMigrating,
    modelsMigrateMsg,
  } = useModelDirectorySettings({ retryModel });

  // Which catalog models are already on disk. Refreshed when the engine changes,
  // because a finished download or a models-directory switch changes the answer.
  useEffect(() => {
    let disposed = false;
    listInstalledModels()
      .then((keys) => { if (!disposed) setInstalledModels(new Set(keys)); })
      .catch(() => { if (!disposed) setInstalledModels(new Set()); });
    return () => { disposed = true; };
  }, [engine, modelsDir]);

  useEffect(() => {
    let disposed = false;
    getVoiceProcessing()
      .then((settings) => {
        if (disposed || !settings) return;
        setSilenceFilterState(settings.silenceFilter !== false);
        setRemoveFillersState(settings.removeFillers !== false);
      })
      .catch(() => {});
    getModelDirs()
      .then((dirs) => { if (!disposed && Array.isArray(dirs)) setModelDirsState(dirs); })
      .catch(() => {});
    return () => { disposed = true; };
  }, []);

  const handleSilenceFilterToggle = useCallback(async (enabled: boolean) => {
    setSilenceFilterState(enabled);
    try {
      await setSilenceFilter(enabled);
    } catch {
      setSilenceFilterState(!enabled);
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [t]);

  const handleRemoveFillersToggle = useCallback(async (enabled: boolean) => {
    setRemoveFillersState(enabled);
    try {
      await setRemoveFillers(enabled);
    } catch {
      setRemoveFillersState(!enabled);
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [t]);

  const handleAddModelFolder = useCallback(async () => {
    try {
      const folder = await pickFolder();
      if (!folder) return;
      const next = await setModelDirs([...modelDirs, folder]);
      setModelDirsState(next);
      const keys = await listInstalledModels();
      setInstalledModels(new Set(keys));
      // The selected model may now be found in the new folder.
      retryModel();
    } catch {
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [modelDirs, retryModel, t]);

  const handleTestModel = useCallback(async () => {
    if (testRunning) return;
    setTestRunning(true);
    setTestText(null);
    setTestError(null);
    try {
      const result = await testTranscription(5);
      setTestText(result.text.trim());
    } catch (error) {
      setTestError(error instanceof Error ? error.message : String(error));
    } finally {
      setTestRunning(false);
    }
  }, [testRunning]);

  const handleRemoveModelFolder = useCallback(async (folder: string) => {
    try {
      const next = await setModelDirs(modelDirs.filter((entry) => entry !== folder));
      setModelDirsState(next);
      const keys = await listInstalledModels();
      setInstalledModels(new Set(keys));
      retryModel();
    } catch {
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [modelDirs, retryModel, t]);

  const {
    deviceListLoading,
    handleInputDeviceChange,
    handleMicLevelMonitorToggle,
    handleTestMicrophone,
    inputDevices,
    micLevel,
    micLevelMonitorEnabled,
    micMonitorReady,
    refreshInputDevices,
    selectedInputDeviceName,
  } = useMicrophoneSettings({ active, closePicker: picker.close, isRecording });

  // (hotkey capture effects are now in useHotkeyCapture hook)

  const handleResetHotkey = async () => {
    if (mainHotkeyCapture.saving) return;
    mainHotkeyCapture.cancelCapture();
    try {
      await setHotkey(DEFAULT_HOTKEY);
      toast.success(t("toast.hotkeyReset"));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.hotkeyResetFailed"));
    }
  };

  const handleAddHotWord = useCallback(() => {
    const word = newHotWord.trim();
    if (!word) return;

    addHotWord(word, 3).then(() => {
      setNewHotWord("");
      refreshProfile();
      toast.success(t("toast.hotWordAdded", { word }));
    }).catch(() => toast.error(t("toast.hotWordAddFailed")));
  }, [newHotWord, refreshProfile]);

  const hotkeyStatusError = hotkeyError || hotkeyDiagnostic?.lastError || null;
  const selectedDeviceMissing = Boolean(selectedInputDeviceName)
    && !inputDevices.some((device) => device.name === selectedInputDeviceName);

  const [magpieStatus, setMagpieStatus] = useState<MagpieStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    detectMagpie()
      .then((status) => {
        if (!cancelled) setMagpieStatus(status);
      })
      .catch(() => {
        // No Magpie on this machine, or the probe failed: leave it out.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const allProviderOptions = useMemo(() => {
    const presets = llmProviderOptions.map((opt) => ({
      key: opt.key, label: opt.label, desc: t(opt.descKey), baseUrl: opt.baseUrl, isCustom: false,
    }));
    // Magpie is only a choice when it is installed here, or already answering.
    // Keep it listed while it is the saved provider so the card can explain
    // that the gateway is down instead of leaving the picker blank.
    if (magpieStatus?.available || profile?.llm_provider.active === magpieProviderOption.key) {
      presets.push({
        key: magpieProviderOption.key,
        label: magpieProviderOption.label,
        desc: t(magpieProviderOption.descKey),
        baseUrl: magpieProviderOption.baseUrl,
        isCustom: false,
      });
    }
    return presets.concat((profile?.llm_provider.custom_providers ?? []).map((provider) => ({
      key: provider.id, label: provider.name, desc: provider.api_format, baseUrl: provider.base_url, isCustom: true,
    })));
  }, [profile, t, magpieStatus]);

  const selectedRecordingModeOption = useMemo(
    () => findRecordingModeOption(recordingMode),
    [recordingMode],
  );
  const selectedInputDeviceOption = useMemo(() => {
    if (!selectedInputDeviceName) {
      const systemDefaultDevice = inputDevices.find((device) => device.isDefault);
      return {
        label: t("settings.followSystemMic"),
        desc: systemDefaultDevice ? t("settings.currentDefault", { name: systemDefaultDevice.name }) : t("settings.autoUseDefault"),
      };
    }

    const activeDevice = inputDevices.find((device) => device.name === selectedInputDeviceName);
    if (activeDevice) {
      return {
        label: activeDevice.name,
        desc: activeDevice.isDefault ? t("settings.alsoSystemDefault") : t("settings.fixedMic"),
      };
    }

    return {
      label: selectedInputDeviceName,
      desc: t("settings.deviceUnavailable"),
    };
  }, [inputDevices, selectedInputDeviceName, t]);

  const handleRecordingModeChange = useCallback((mode: "hold" | "toggle") => {
    setRecordingModeState(mode);
    picker.close();
    writeLocalStorage(RECORDING_MODE_KEY, mode);
    setRecordingMode(mode === "toggle").catch(() => {});
  }, []);

  return (
    <div className="page-root settings-page">

      <div className={`${pageContentClass} settings-layout`}>
        {!settingsReady ? (
          <div className="settings-content settings-load-state" role={profileLoadError ? "alert" : "status"}>
            {profileLoadError ? (
              <>
                <AlertTriangle size={20} aria-hidden="true" />
                <p>{t("settings.profileLoadFailed")}</p>
                <button className="btn-ghost" type="button" disabled={profileLoadPending} onClick={() => void refreshProfile()}>
                  {t("common.retry")}
                </button>
              </>
            ) : <p>{t("common.loading")}</p>}
          </div>
        ) : (
          <>
        {/* Settings nav */}
        <nav className="settings-nav" aria-label={t("settings.title")} ref={navScrollRef}>
          <div className="settings-nav-head">
            <span className="settings-nav-title">{t("settings.title")}</span>
            <span className="settings-nav-count">{String(navSections.length).padStart(2, "0")}</span>
          </div>
          {navSections.map(({ id, labelKey }, index) => (
            <button
              key={id}
              type="button"
              className="settings-nav-tab"
              aria-current={activeNavSection === id ? "location" : undefined}
              data-active={activeNavSection === id}
              data-nav-tab={id}
              onClick={() => handleNavClick(id)}
            >
              <span className="settings-nav-index" aria-hidden="true">{String(index + 1).padStart(2, "0")}</span>
              <span className="settings-nav-label">{t(labelKey)}</span>
            </button>
          ))}
        </nav>

        {/* Content */}
        <div className="settings-content" ref={settingsContentRef}>
          <div className="settings-sections">

          <AppearanceSettingsSection />

          {/* Engine */}
          <section
            className="settings-card"
            data-nav-id="engine"
            style={{
              position: "relative",
              zIndex: picker.isOpen("alibabaModel") ? 8 : "auto",
            }}
          >
            <div className="settings-section-header">
              <AudioLines size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.engine")}</h2>
            </div>
            {(() => {
              const currentLabel = engineDisplayLabel(engine, t);
              const currentDescription = engineDisplayDescription(engine, t);
              const currentCapabilityTags = asrCapabilityTagKeys(engine);
              return (
                <div className="settings-column" style={{ gap: 8 }}>
                  <button
                    type="button"
                    className="picker-trigger"
                    data-open={browserOpen}
                    aria-expanded={browserOpen}
                    aria-controls="asr-model-browser"
                    aria-label={t("settings.engine")}
                    disabled={engineLoading || onlineAsrRegionLoading}
                    onClick={() => setBrowserOpen((open) => !open)}
                  >
                    <span style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
                      <AudioLines size={18} strokeWidth={1.5} style={{ flexShrink: 0, color: "var(--color-accent)" }} />
                      <span className="picker-trigger-copy">
                        <strong>{currentLabel}</strong>
                        <span>
                          {formatAsrEngineDescription(engine, currentDescription)}
                          {currentCapabilityTags.length > 0 && (
                            <span className="asr-capability-tags">
                              {currentCapabilityTags.map((tagKey) => (
                                <span key={tagKey} className="asr-capability-tag">{t(tagKey)}</span>
                              ))}
                            </span>
                          )}
                        </span>
                      </span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" style={{ flexShrink: 0 }} />
                  </button>
                  <SettingsReveal open={browserOpen} gap={8}>
                    <div id="asr-model-browser">
                      <ModelBrowser
                        engine={engine}
                        onSelect={(key) => { void handleEngineSwitch(key); }}
                        disabled={engineLoading || onlineAsrRegionLoading}
                        installed={installedModels}
                      />
                    </div>
                  </SettingsReveal>
                  {!isOnlineEngineKey(engine) && (
                    <div className="settings-column" style={{ gap: 6 }}>
                      <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
                        <button
                          type="button"
                          className="theme-btn theme-btn-xs"
                          disabled={testRunning || engineLoading || isRecording || modelStage === "downloading"}
                          onClick={() => { void handleTestModel(); }}
                        >
                          <Mic size={11} />
                          {testRunning ? t("settings.testModelRecording") : t("settings.testModel")}
                        </button>
                        <span className="settings-hint">{t("settings.testModelHint")}</span>
                      </div>
                      {testText !== null && (
                        <p className="settings-test-result" aria-live="polite">
                          {testText || t("settings.testModelSilence")}
                        </p>
                      )}
                      {testError && <p className="settings-error">{testError}</p>}
                    </div>
                  )}
                  {modelStage === "need_download" && (
                    <p className="settings-hint">{t("settings.engineNeedsDownload", { engine: currentLabel })}</p>
                  )}
                  {modelStage === "need_repair" && (
                    <p className="settings-hint">{t("settings.engineNeedsRepair", { engine: currentLabel })}</p>
                  )}
                  {modelStage === "error" && modelError && (
                    <p className="settings-error">{t("settings.engineUnavailable", { engine: currentLabel, error: modelError })}</p>
                  )}
                  {isOnlineEngineKey(engine) && !onlineAsrApiKey && (
                    <p className="settings-hint">{t("settings.engineNeedsKey", { engine: currentLabel })}</p>
                  )}
                </div>
              );
            })()}
            <GpuIdleUnloadControl />
            <SettingsReveal open={engine === "confucius4-r2t2" && !!profile} gap={10}>
              {profile && <R2T2SettingsSection profile={profile} onSaved={() => { void refreshProfile(); }} />}
            </SettingsReveal>
            {(engine === "glm-asr" || engine === "alibaba-asr") && (
              <div className="settings-inline-panel" style={{ marginTop: 8 }}>
                <div className="settings-column" style={{ gap: 4 }}>
                  <span className="settings-option-desc">{t("settings.apiEndpoint")}</span>
                  <div className="settings-row" style={{ gap: 6 }}>
                    {([
                      { region: "international", labelKey: "settings.international" },
                      { region: "domestic", labelKey: "settings.domestic" },
                    ] as const).map(({ region, labelKey }) => (
                      <button
                        key={region}
                        type="button"
                        disabled={engineLoading || onlineAsrRegionLoading}
                        className={`theme-btn${onlineAsrRegion === region ? " active" : ""}`}
                        onClick={() => { void handleOnlineAsrRegionChange(region); }}
                        style={{ flex: 1 }}
                      >
                        {t(labelKey)}
                      </button>
                    ))}
                  </div>
                  {onlineAsrUrl && (
                    <span className="settings-option-desc" style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
                      {onlineAsrUrl}
                    </span>
                  )}
                </div>
                {engine === "alibaba-asr" && alibabaAsrModels.length > 0 && (
                  <div className="settings-column" style={{ gap: 4 }}>
                    <div className="settings-row" style={{ gap: 6, alignItems: "center" }}>
                      <span className="settings-option-desc" style={{ flex: 1 }}>
                        {t("settings.alibabaModelLabel")}
                        <span style={{ marginLeft: 6, fontSize: 11, color: "var(--color-text-tertiary)" }}>
                          {alibabaAsrModelsLoading
                            ? t("settings.alibabaModelsLoading")
                            : alibabaAsrModelsSource === "live"
                              ? t("settings.alibabaModelsLive", { count: alibabaAsrModels.length })
                              : t("settings.alibabaModelsFallback")}
                        </span>
                      </span>
                      <button
                        type="button"
                        className="theme-btn theme-btn-xs"
                        disabled={alibabaAsrModelsLoading || !alibabaHasKey}
                        onClick={() => { void refreshAlibabaModels(); }}
                        aria-label={t("settings.alibabaModelsRefresh")}
                      >
                        <RotateCcw size={11} />
                        {t("settings.alibabaModelsRefresh")}
                      </button>
                    </div>
                    <div ref={picker.setRef("alibabaModel")} style={{ position: "relative" }}>
                      <button
                        type="button"
                        className="picker-trigger"
                        data-open={picker.isOpen("alibabaModel")}
                        aria-haspopup="listbox"
                          aria-expanded={picker.isExpanded("alibabaModel")}
                        aria-label={t("settings.alibabaModelLabel")}
                        onClick={() => picker.toggle("alibabaModel")}
                      >
                        <span style={{ fontFamily: "var(--font-mono, monospace)", fontSize: 12, color: "var(--color-text-primary)" }}>
                          {alibabaAsrModel}
                        </span>
                        <ChevronsUpDown size={13} />
                      </button>
                      {picker.isOpen("alibabaModel") && (
                        <div className={picker.popoverClass("alibabaModel")}>
                          <div className="picker-list" role="listbox">
                            {alibabaAsrModels.map((m) => (
                              <button
                                key={m}
                                type="button"
                                className="picker-option"
                                data-active={alibabaAsrModel === m}
                                onClick={() => {
                                  picker.close();
                                  void handleAlibabaAsrModelSelect(m);
                                }}
                              >
                                <strong style={{ fontSize: 12, fontFamily: "var(--font-mono, monospace)" }}>{m}</strong>
                              </button>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                )}
                <fieldset disabled={engineLoading || onlineAsrRegionLoading}
                  className="settings-column" style={{ gap: 4, border: 0, padding: 0, margin: 0, minWidth: 0 }}>
                  <span className="settings-option-desc">{t("settings.apiKey")}</span>
                  <SecretInput
                    value={onlineAsrApiKey}
                    placeholder={engine === "alibaba-asr"
                      ? t("settings.alibabaApiKeyPlaceholder")
                      : t("settings.glmApiKeyPlaceholder")}
                    ariaLabelShow={t("settings.showApiKey")}
                    ariaLabelHide={t("settings.hideApiKey")}
                    onChange={handleOnlineAsrApiKeyChange}
                  />
                </fieldset>
              </div>
            )}
            {/* Model Directory */}
            {!isOnlineEngineKey(engine) && (
              <div className="settings-column" style={{ gap: 6, marginTop: 8 }}>
                <div className="settings-row" style={{ gap: 6, alignItems: "center" }}>
                  <HardDrive size={13} className="icon-tertiary" style={{ flexShrink: 0 }} />
                  <span className="settings-option-desc" style={{ flex: 1 }}>{t("settings.modelStorageDir")}</span>
                  {modelsDirCustom && (
                    <button
                      className="theme-btn theme-btn-xs"
                      disabled={modelsDirMigrating}
                      onClick={() => { void handleRestoreDefaultModelsDir(); }}
                    >
                      <RotateCcw size={11} />
                      {t("settings.restoreDefault")}
                    </button>
                  )}
                  <button
                    className="theme-btn theme-btn-xs"
                    disabled={modelsDirMigrating}
                    onClick={() => { void handleChooseModelsDir(); }}
                  >
                    <FolderOpen size={11} />
                    {modelsDirMigrating ? (modelsMigrateMsg || t("settings.migrating")) : t("common.change")}
                  </button>
                </div>
                <span
                  className="settings-option-desc"
                  style={{ fontSize: 11, color: "var(--color-text-tertiary)", wordBreak: "break-all", userSelect: "text" }}
                >
                  {modelsDir || t("common.loading")}
                </span>
              </div>
            )}
            {/* Model folders: read-only extra locations */}
            {!isOnlineEngineKey(engine) && (
              <div className="settings-column" style={{ gap: 6, marginTop: 8 }}>
                <div className="settings-row" style={{ gap: 6, alignItems: "center" }}>
                  <FolderOpen size={13} className="icon-tertiary" style={{ flexShrink: 0 }} />
                  <span className="settings-option-desc" style={{ flex: 1 }}>{t("settings.modelFolders")}</span>
                  <button
                    className="theme-btn theme-btn-xs"
                    onClick={() => { void handleAddModelFolder(); }}
                  >
                    <Plus size={11} />
                    {t("settings.addModelFolder")}
                  </button>
                </div>
                <span className="settings-hint">{t("settings.modelFoldersDesc")}</span>
                {modelDirs.length === 0 ? (
                  <span className="settings-option-desc" style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
                    {t("settings.modelFoldersEmpty")}
                  </span>
                ) : (
                  <ul className="model-folders">
                    {modelDirs.map((folder) => (
                      <li key={folder} className="model-folder-row">
                        <span className="model-folder-path">{folder}</span>
                        <button
                          type="button"
                          className="theme-btn theme-btn-xs"
                          aria-label={t("settings.removeModelFolder", { path: folder })}
                          onClick={() => { void handleRemoveModelFolder(folder); }}
                        >
                          <X size={11} />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            {/* Speech processing */}
            <div className="settings-column" style={{ gap: 8, marginTop: 8 }}>
              <span className="settings-option-desc">{t("settings.speechProcessing")}</span>
              <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
                <div className="settings-column" style={{ flex: 1, gap: 2 }}>
                  <span className="settings-option-label">{t("settings.silenceFilter")}</span>
                  <span className="settings-hint">{t("settings.silenceFilterDesc")}</span>
                </div>
                <button
                  role="switch"
                  aria-checked={silenceFilter}
                  aria-label={t("settings.silenceFilter")}
                  onClick={() => { void handleSilenceFilterToggle(!silenceFilter); }}
                  className="toggle-switch"
                >
                  <div className="toggle-knob" />
                </button>
              </div>
              {engine === "confucius4-r2t2" && (
                <p className="settings-hint">{t("settings.silenceFilterR2T2")}</p>
              )}
              <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
                <div className="settings-column" style={{ flex: 1, gap: 2 }}>
                  <span className="settings-option-label">{t("settings.removeFillers")}</span>
                  <span className="settings-hint">{t("settings.removeFillersDesc")}</span>
                </div>
                <button
                  role="switch"
                  aria-checked={removeFillers}
                  aria-label={t("settings.removeFillers")}
                  onClick={() => { void handleRemoveFillersToggle(!removeFillers); }}
                  className="toggle-switch"
                >
                  <div className="toggle-knob" />
                </button>
              </div>
              {(isOnlineEngineKey(engine) || engine === "confucius4-r2t2") && (
                <p className="settings-hint">{t("settings.removeFillersScope")}</p>
              )}
            </div>
          </section>

          <AiModelsSettingsSection
            profile={profile}
            allProviderOptions={allProviderOptions}
            onRefreshProfile={refreshProfile}
            magpieStatus={magpieStatus}
          />

          {/* Hotkey */}
          <section
            className="settings-card"
            data-nav-id="hotkey"
            style={{
              position: "relative",
              zIndex: picker.isOpen("recordingMode") ? 9 : "auto",
            }}
          >
            <div className="settings-section-header">
              <Keyboard size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.hotkeySection")}</h2>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => mainHotkeyCapture.startCapture()}
                  disabled={mainHotkeyCapture.saving}
                  data-capturing={mainHotkeyCapture.capturing}
                  style={{
                    cursor: mainHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: mainHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {mainHotkeyCapture.capturing
                    ? t("settings.pressCombo")
                    : hotkeyDisplay
                      ? <Kbd combo={hotkeyDisplay} />
                      : null}
                </button>
                <button
                  className="btn-ghost"
                  onClick={handleResetHotkey}
                  disabled={mainHotkeyCapture.saving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: mainHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: mainHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {t("settings.resetF2")}
                </button>
              </div>
              <p className="settings-hint">
                {t("settings.hotkeyHint")}
              </p>
              <div className="settings-column" style={{ gap: 6, marginTop: 8 }}>
                <span className="settings-option-desc">{t("settings.recordingMode")}</span>
                <div
                  ref={picker.setRef("recordingMode")}
                  style={{
                    position: "relative",
                    zIndex: picker.isOpen("recordingMode") ? 2 : 1,
                  }}
                >
                  <button
                    type="button"
                    className="picker-trigger"
                    data-open={picker.isOpen("recordingMode")}
                    aria-haspopup="listbox"
                    aria-expanded={picker.isExpanded("recordingMode")}
                    aria-label={t("settings.recordingModeLabel")}
                    onClick={() => {
                      picker.toggle("recordingMode");
                    }}
                  >
                    <span className="picker-trigger-copy">
                      <strong>{t(selectedRecordingModeOption.labelKey)}</strong>
                      <span>{t(selectedRecordingModeOption.descKey)}</span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" />
                  </button>
                  {picker.isOpen("recordingMode") && (
                    <div className={picker.popoverClass("recordingMode")}>
                      <div className="picker-list" role="listbox">
                        {recordingModeOptions.map((option) => (
                          <button
                            key={`recording-mode-${option.key}`}
                            type="button"
                            className="picker-option"
                            data-active={recordingMode === option.key}
                            onClick={() => handleRecordingModeChange(option.key)}
                          >
                            <span className="picker-option-copy">
                              <strong>{t(option.labelKey)}</strong>
                              <span>{t(option.descKey)}</span>
                            </span>
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
              {hotkeyDiagnostic?.systemConflict && (
                <p className="settings-error" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <AlertTriangle size={12} style={{ flexShrink: 0 }} /> {hotkeyDiagnostic.systemConflict}
                </p>
              )}
              {hotkeyDiagnostic?.warning && <p className="settings-hint">{hotkeyDiagnostic.warning}</p>}
              {hotkeyStatusError && <p className="settings-error">{hotkeyStatusError}</p>}
            </div>
          </section>

          {/* Microphone */}
          <section
            className="settings-card"
            data-nav-id="microphone"
            style={{
              position: "relative",
              zIndex: picker.isOpen("microphone") ? 8 : "auto",
            }}
          >
            <div className="settings-section-header">
              <Mic size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.microphone")}</h2>
              <div className="settings-row" style={{ marginLeft: "auto", gap: 8, flex: "0 0 auto" }}>
                <span className="settings-option-desc" style={{ whiteSpace: "nowrap" }}>{t("settings.levelMonitor")}</span>
                <button
                  role="switch"
                  aria-checked={micLevelMonitorEnabled}
                  aria-label={t("settings.micLevelMonitor")}
                  onClick={() => handleMicLevelMonitorToggle(!micLevelMonitorEnabled)}
                  className="toggle-switch"
                >
                  <div className="toggle-knob" />
                </button>
              </div>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button className="test-btn" onClick={() => { void handleTestMicrophone(); }}>{t("common.test")}</button>
                <div ref={picker.setRef("microphone")} style={{ position: "relative", flex: 1, minWidth: 0 }}>
                  <button
                    type="button"
                    className="picker-trigger microphone-select"
                    data-open={picker.isOpen("microphone")}
                    aria-haspopup="listbox"
                    aria-expanded={picker.isExpanded("microphone")}
                    aria-label={t("settings.selectMic")}
                    disabled={deviceListLoading}
                    onClick={() => {
                      if (deviceListLoading) return;
                      picker.toggle("microphone");
                    }}
                    style={{
                      opacity: deviceListLoading ? 0.7 : 1,
                      cursor: deviceListLoading ? "wait" : "pointer",
                    }}
                  >
                    <span className="picker-trigger-copy">
                      <strong>{selectedInputDeviceOption.label}</strong>
                      <span>{selectedInputDeviceOption.desc}</span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" />
                  </button>
                  {picker.isOpen("microphone") && (
                    <div className={picker.popoverClass("microphone")}>
                      <div className="picker-list" role="listbox">
                        <button
                          type="button"
                          className="picker-option"
                          data-active={!selectedInputDeviceName}
                          onClick={() => { void handleInputDeviceChange(""); }}
                        >
                          <span className="picker-option-copy">
                            <strong>{t("settings.followSystemMic")}</strong>
                            <span>
                              {inputDevices.find((device) => device.isDefault)?.name ?? t("settings.autoUseDefault")}
                            </span>
                          </span>
                        </button>
                        {inputDevices.map((device) => (
                          <button
                            key={device.name}
                            type="button"
                            className="picker-option"
                            data-active={selectedInputDeviceName === device.name}
                            onClick={() => { void handleInputDeviceChange(device.name); }}
                          >
                            <span className="picker-option-copy">
                              <strong>{device.name}</strong>
                              <span>{device.isDefault ? t("settings.systemDefaultDevice") : t("settings.canSelect")}</span>
                            </span>
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
                <button
                  className="btn-ghost btn-ghost-sm"
                  disabled={deviceListLoading}
                  onClick={() => { void refreshInputDevices(); }}
                  style={{ opacity: deviceListLoading ? 0.7 : 1 }}
                >
                  {t("common.refresh")}
                </button>
              </div>
              <div className="mic-level-shell" aria-label={t("settings.micLevelPreview")}>
                <div className="mic-level-fill" style={{ "--mic-level": `${Math.round(micLevel * 100)}%` } as React.CSSProperties} />
              </div>
              <div className="settings-row" style={{ gap: 10 }}>
                <span className="settings-hint">
                  {!micLevelMonitorEnabled
                    ? t("settings.micMonitorOff")
                    : isRecording
                    ? t("settings.micRecordingPaused")
                    : micMonitorReady
                      ? t("settings.micSpeakToTest")
                      : t("settings.micNotStarted")}
                </span>
                <span className="settings-option-desc">{Math.round(micLevel * 100)}%</span>
              </div>
              {selectedDeviceMissing && (
                <p className="settings-error">
                  {t("settings.savedMicUnavailable")}
                </p>
              )}
            </div>
          </section>

          {/* Input Method */}
          <section className="settings-card" data-nav-id="input">
            <div className="settings-section-header">
              <ClipboardPaste size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.inputMethod")}</h2>
            </div>
            <div className="input-method-list">
              {inputOptions.map(({ key, icon: Icon, labelKey, descKey }) => (
                <button
                  key={key}
                  className="input-method-item"
                  aria-pressed={inputMethod === key}
                  onClick={() => {
                    setInputMethod(key);
                    writeLocalStorage(INPUT_METHOD_KEY, key);
                    setInputMethodCommand(key).catch(() => {});
                  }}
                >
                  <Icon size={18} strokeWidth={1.5} style={{ color: inputMethod === key ? "var(--color-accent)" : "var(--color-text-tertiary)", flexShrink: 0 }} />
                  <div className="input-method-item-body">
                    <span className="settings-option-label">{t(labelKey)}</span>
                    <span className="settings-option-desc">{t(descKey)}</span>
                  </div>
                </button>
              ))}
            </div>
            <div className="settings-row" style={{ marginTop: 6 }}>
              <span className="permission-label">{t("settings.recordingSound")}</span>
              <button
                role="switch"
                aria-checked={soundEnabled}
                aria-label={t("settings.recordingSound")}
                onClick={() => {
                  const next = !soundEnabled;
                  setSoundEnabledState(next);
                  writeLocalStorage(SOUND_ENABLED_KEY, String(next));
                  setSoundEnabled(next).catch(() => {});
                }}
                className="toggle-switch"
              >
                <div className="toggle-knob" />
              </button>
            </div>
          </section>

          <DictationToolsSettingsSection />

          <AppProfileRulesSettingsSection
            profile={profile}
            onSaved={() => { void refreshProfile(); }}
          />
          <HistorySettingsSection
            profile={profile}
            onSaved={() => { void refreshProfile(); }}
          />

          {/* Smart Vocabulary */}
          <section className="settings-card" data-nav-id="vocabulary">
            <div className="settings-section-header">
              <BookOpen size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.vocabulary")}</h2>
              {profile && (
                <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-text-tertiary)" }}>
                  {t("settings.hotWordsCount", { count: profile.hot_words.length, transcriptions: profile.total_transcriptions })}
                </span>
              )}
            </div>
            <div className="settings-column" style={{ gap: 8 }}>
              {getAsrEngineCapability(engine)?.vocabulary === false && (
                <p className="settings-hint">
                  {t("settings.vocabularyAfterRecognitionHint", {
                    engine: engineDisplayLabel(engine, t),
                  })}
                </p>
              )}
              {/* Add hot word */}
              <div style={{ display: "flex", gap: 6 }}>
                <input
                  type="text"
                  placeholder={t("settings.addHotWordPlaceholder")}
                  aria-label={t("settings.addHotWordLabel")}
                  value={newHotWord}
                  onChange={(e) => setNewHotWord(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newHotWord.trim()) {
                      handleAddHotWord();
                    }
                  }}
                  style={{
                    flex: 1, padding: "7px 10px", borderRadius: 8,
                    border: "1px solid var(--color-border)",
                    background: "var(--color-bg-secondary)",
                    color: "var(--color-text-primary)", fontSize: 13, outline: "none",
                  }}
                />
                <button
                  type="button"
                  className="test-btn"
                  aria-label={t("settings.addHotWordLabel")}
                  onClick={() => {
                    handleAddHotWord();
                  }}
                  style={{ padding: "7px 12px" }}
                >
                  <Plus size={14} />
                </button>
              </div>

              {/* Hot word list */}
              {profile && profile.hot_words.length > 0 && (
                <div style={{
                  display: "flex", flexWrap: "wrap", gap: 4,
                  maxHeight: 120, overflow: "auto",
                  padding: "4px 0",
                }}>
                  {[...profile.hot_words]
                    .sort((a, b) => b.weight - a.weight || b.use_count - a.use_count)
                    .map((hw) => (
                    <span
                      key={hw.text}
                      style={{
                        display: "inline-flex", alignItems: "center", gap: 4,
                        padding: "3px 8px", borderRadius: 12,
                        background: "var(--color-bg-secondary)",
                        border: `1px solid ${sourceColors[hw.source] ?? "var(--color-border)"}`,
                        fontSize: 12, color: "var(--color-text-secondary)",
                      }}
                    >
                      <span style={{
                        width: 6, height: 6, borderRadius: "50%",
                        background: sourceColors[hw.source] ?? "var(--color-border)",
                        flexShrink: 0,
                      }} />
                      {hw.text}
                      <button
                        type="button"
                        className="hot-word-remove"
                        aria-label={t("settings.removeHotWordLabel", { word: hw.text })}
                        onClick={() => {
                          removeHotWord(hw.text)
                            .then(() => refreshProfile())
                            .catch(() => toast.error(t("settings.hotWordRemoveFailed")));
                        }}
                      >
                        <X size={10} />
                      </button>
                    </span>
                  ))}
                </div>
              )}

              {/* Correction rules management */}
              <div style={{ marginTop: 12, paddingTop: 12, borderTop: "1px solid var(--color-border-subtle)" }}>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <label style={{ fontSize: 13, color: "var(--color-text-primary)", flex: 1 }}>
                    {t("settings.correctionRules")}
                  </label>
                  <span style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
                    {profile ? t("settings.correctionRulesCount", { count: profile.correction_patterns.length }) : ""}
                  </span>
                  <button
                    ref={correctionManageButtonRef}
                    className="btn-ghost"
                    onClick={() => setCorrectionModalOpen(true)}
                    style={{ padding: "4px 10px", fontSize: 12 }}
                  >
                    {t("settings.correctionManage")}
                  </button>
                </div>
              </div>

              {/* Legend + actions */}
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                {Object.entries(sourceLabels).map(([key, label]) => (
                  <span key={key} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 11, color: "var(--color-text-tertiary)" }}>
                    <span style={{ width: 6, height: 6, borderRadius: "50%", background: sourceColors[key] }} />
                    {t(label)}
                  </span>
                ))}
                <span style={{ flex: 1 }} />
              </div>
            </div>
          </section>

          <SystemSettingsSections
            lastExportPath={lastExportPath}
            exporting={exporting}
            autostart={autostart}
            autostartLoading={autostartLoading}
            autostartMinimized={autostartMinimized}
            autostartMinimizedLoading={autostartMinimizedLoading}
            appVersion={appVersion}
            onExport={() => { void handleExportConfig(); }}
            onCopyExportPath={() => { void handleCopyExportPath(); }}
            onImport={handleImportConfig}
            onTestPaste={() => { void handleTestPaste(); }}
            onToggleAutostart={() => { void handleAutostartToggle(); }}
            onToggleAutostartMinimized={() => { void handleAutostartMinimizedToggle(); }}
          />
        </div>
      </div>

        {/* Footer */}
        <div className="settings-footer">
          <p className="settings-footer-text">
            {t("settings.footer")} <span className="settings-footer-version">v{appVersion}</span>
            <span style={{ margin: "0 6px" }}>·</span>
            {t("settings.footerSubtitle")}
          </p>
        </div>
          </>
        )}
      </div>

      {profile && !profileLoadError && correctionModalOpen && (
        <Suspense fallback={null}>
          <CorrectionRulesModal
            profile={profile}
            allProviderOptions={allProviderOptions}
            validationEnabled={validationEnabled}
            setValidationEnabled={setValidationEnabled}
            validationUseSeparateModel={validationUseSeparateModel}
            setValidationUseSeparateModel={setValidationUseSeparateModel}
            validationProvider={validationProvider}
            setValidationProvider={setValidationProvider}
            validationModel={validationModel}
            setValidationModel={setValidationModel}
            validationRunning={validationRunning}
            setValidationRunning={setValidationRunning}
            validationResult={validationResult}
            setValidationResult={setValidationResult}
            returnFocusRef={correctionManageButtonRef}
            onClose={() => setCorrectionModalOpen(false)}
            onRefreshProfile={refreshProfile}
          />
        </Suspense>
      )}
    </div>
  );
}
