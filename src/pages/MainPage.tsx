import { useState, useEffect, useLayoutEffect, useRef, useCallback } from "react";
import { ArrowUpRight, AudioLines, FileText, Keyboard, Minus, Settings, X } from "lucide-react";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { copyToClipboard, hideMainWindow, listInstalledModels, submitUserCorrection } from "@/api/tauri";
import TitleBar from "@/components/TitleBar";
import RecordingButton from "@/components/RecordingButton";
import StatusIndicator from "@/components/StatusIndicator";
import TranscriptionResult from "@/components/TranscriptionResult";
import TranscriptionHistory from "@/components/TranscriptionHistory";
import { useAsrEngineSettings } from "@/hooks/useAsrEngineSettings";
import { engineOptions, engineOptionLabel } from "@/lib/asrEngineOptions";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useTranslation } from "react-i18next";
import { ONBOARDING_DISMISSED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { prefersReducedMotion } from "@/lib/motion";

export default function MainPage({ onNavigate, animClass = "" }: {
  onNavigate: (v: "main" | "settings" | "history") => void;
  animClass?: string;
}) {
  const { t } = useTranslation();
  const {
    isStarting, isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, originalAsrText, editBaselineText, setEditBaselineText, setTranscriptionResult, polishAuditWarning,
    durationSec, charCount, detectedLanguage, editGrabStatus, timing, history, recordingMode, stage: modelStage, isReady: modelReady,
    device, gpuName, downloadProgress, downloadMessage,
    isDownloading, modelError, hotkeyDisplay,
    downloadModels: triggerDownload, cancelDownload, retryModel,
  } = useRecordingContext();

  const engineLabel = useCallback((value: string) => {
    const option = engineOptions.find((item) => item.key === value);
    return option ? engineOptionLabel(option, t) : value;
  }, [t]);
  const {
    engine, engineLoading, handleEngineSwitch,
    alibabaAsrModel, alibabaAsrModelSaving, alibabaAsrModels, alibabaAsrModelsLoading, handleAlibabaAsrModelSelect,
  } = useAsrEngineSettings({ engineLabel, retryModel });
  // Groups the model list so the current download is easy to find.
  useEffect(() => {
    let disposed = false;
    listInstalledModels()
      .then((keys) => { if (!disposed) setInstalledEngines(new Set(keys)); })
      .catch(() => { if (!disposed) setInstalledEngines(new Set()); });
    return () => { disposed = true; };
  }, [engine, modelReady]);

  const isReady = modelReady && !engineLoading && !alibabaAsrModelSaving;
  const stage = engineLoading || alibabaAsrModelSaving ? "checking" : modelStage;
  const modelSelectionDisabled = engineLoading || alibabaAsrModelSaving || isStarting || isRecording || isProcessing || isDownloading;

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [installedEngines, setInstalledEngines] = useState<ReadonlySet<string>>(() => new Set());
  const [errorDismissed, setErrorDismissed] = useState(false);
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => readLocalStorage(ONBOARDING_DISMISSED_KEY) === "true");
  const isToggleMode = readLocalStorage(RECORDING_MODE_KEY) === "toggle";
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const previousResult = useRef<{ id: string; top: number } | null>(null);

  useLayoutEffect(() => {
    const root = resultsRef.current;
    const current = history[0];
    const card = root?.querySelector<HTMLElement>(".result-card");
    if (!root || !card || !current || current.text !== transcriptionResult) return;
    const previous = previousResult.current;
    if (previous && previous.id !== current.id && !prefersReducedMotion()) {
      const oldItem = Array.from(root.querySelectorAll<HTMLElement>(".history-item"))
        .find((item) => item.dataset.historyId === previous.id);
      if (oldItem?.animate) {
        oldItem.animate([
          { transform: `translateY(${previous.top - oldItem.getBoundingClientRect().top}px)`, opacity: 0.5 },
          { transform: "translateY(0)", opacity: 1 },
        ], { duration: 220, easing: "cubic-bezier(0.16, 1, 0.3, 1)" });
      }
      card.querySelector("textarea")?.animate?.([{ opacity: 0 }, { opacity: 1 }], { duration: 160, easing: "ease-out" });
    }
    previousResult.current = { id: current.id, top: card.getBoundingClientRect().top };
  }, [history, transcriptionResult]);

  useEffect(() => { setErrorDismissed(false); }, [recordingError, modelError]);
  // Auto-dismiss onboarding after first successful transcription
  useEffect(() => {
    if (transcriptionResult && !onboardingDismissed) {
      setOnboardingDismissed(true);
      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
    }
  }, [transcriptionResult, onboardingDismissed]);
  useEffect(() => () => { if (copyTimerRef.current) clearTimeout(copyTimerRef.current); }, []);

  const pageContentClass = `page-content ${animClass || ""}`.trim();

  const correctionSubmit = useDebouncedCallback((previousText: string, nextText: string, rawOriginalText: string | null) => {
    submitUserCorrection(previousText, nextText, rawOriginalText)
      .then(() => toast.success(t("toast.correctionRecorded"), { duration: 1500 }))
      .catch(() => toast.error(t("toast.correctionFailed")));
  }, 900, { onUnmount: "flush" });

  const handleCopy = useCallback(async (text: string, id: string) => {
    try {
      await copyToClipboard(text);
      setCopiedId(id);
      toast.success(t("common.copiedToClipboard"));
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        setCopiedId(null);
        copyTimerRef.current = null;
      }, 1500);
    } catch {
      toast.error(t("common.copyFailed"));
    }
  }, [t]);

  const handleDraftChange = useCallback((newText: string) => {
    setTranscriptionResult(newText);
  }, [setTranscriptionResult]);

  const handleTextChange = useCallback((newText: string) => {
    if (recordingMode !== "dictation") {
      setTranscriptionResult(newText);
      return;
    }
    if (editBaselineText && newText !== editBaselineText) {
      const prevText = editBaselineText;
      setEditBaselineText(newText);
      setTranscriptionResult(newText);
      correctionSubmit.schedule(prevText, newText, originalAsrText);
    }
  }, [
    correctionSubmit,
    editBaselineText,
    originalAsrText,
    recordingMode,
    setTranscriptionResult,
    setEditBaselineText,
  ]);

  const flushPendingEditAndNavigate = useCallback((target: "main" | "settings" | "history") => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && (active.isContentEditable || active instanceof HTMLTextAreaElement || active instanceof HTMLInputElement)) {
      active.blur();
    }
    correctionSubmit.flush();
    onNavigate(target);
  }, [correctionSubmit, onNavigate]);

  const handleToggleRecording = useCallback(() => {
    if (!isReady) return;
    if (isStarting || isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  }, [isReady, isStarting, isRecording, stopRecording, startRecording]);

  return (
    <div className="page-root main-page">
      <TitleBar
        title={t("main.title")}
        rightActions={
          <>
            <button aria-label={t("common.minimize")} className="icon-btn" onClick={() => getCurrentWindow().minimize()}>
              <Minus size={12} strokeWidth={1.5} />
            </button>
            <button aria-label={t("common.close")} className="icon-btn" onClick={() => hideMainWindow()}>
              <X size={12} strokeWidth={1.5} />
            </button>
          </>
        }
      />

      <div className={`${pageContentClass} dictation-content`}>
        <header className="workspace-heading">
          <div>
            <h1>{t("main.dashboardTitle")}</h1>
            <p>{t("main.dashboardSubtitle")}</p>
          </div>
          <button className="workspace-settings-link" onClick={() => flushPendingEditAndNavigate("settings")}>
            <Settings size={15} aria-hidden="true" /> {t("main.configure")}
          </button>
        </header>
        <div className="dictation-dashboard">

        {(recordingError || modelError) && !errorDismissed && (
          <div className="error-section animate-shake">
            <div className="error-banner">
              <div className="error-banner-inner">
                <p className="settings-error" style={{ lineHeight: 1.6, flex: 1 }}>{recordingError || modelError}</p>
                <button onClick={() => setErrorDismissed(true)} aria-label={t("common.close")} className="error-dismiss-btn">
                  <X size={12} />
                </button>
              </div>
              {stage === "error" && <button onClick={retryModel} className="error-retry-link">{t("common.retry")}</button>}
            </div>
          </div>
        )}

        <section className="recording-panel" data-recording={isRecording || isStarting} aria-label={t("main.recordingControlsLabel")}>
          <div className="recording-dock-action">
            <RecordingButton
              isStarting={isStarting}
              isRecording={isRecording}
              isProcessing={isProcessing}
              isReady={isReady}
              onToggle={handleToggleRecording}
            />
            <div className="recording-dock-copy">
        <div className="recording-panel-heading">
          <h2>{isRecording ? t("main.statusListening") : isStarting ? t("main.statusConnecting") : isProcessing ? t("main.statusProcessing") : isReady ? t("main.dashboardReady") : stage === "error" ? t("main.statusError") : t("main.statusPreparing")}</h2>
          <p>{isRecording ? t("main.hintListening") : isReady ? t("main.hintReady") : t("main.hintDefault")}</p>
        </div>          {isReady && !recordingError && !modelError && (
            <div className="tactile-hotkey-bar" aria-hidden="true">
              <span className={`tactile-hotkey-badge${isRecording ? " is-active" : ""}`}>
                <kbd className="tactile-key">{hotkeyDisplay || "F2"}</kbd>
                <span className="tactile-hotkey-label">
                  {isToggleMode ? t("main.hotkeyHintToggle") : t("main.hotkeyHintHold")}
                </span>
              </span>
            </div>
          )}
            </div>
          </div>
          <div className="recording-dock-engine">
          <div className="dictation-model-selector">
            <label htmlFor="dictation-model">{t("main.modelLabel")}</label>
            <select
              id="dictation-model"
              value={engine}
              disabled={modelSelectionDisabled}
              onChange={(event) => { void handleEngineSwitch(event.target.value); }}
            >
              {([
                ["main.modelGroupInstalled", engineOptions.filter((option) => installedEngines.has(option.key))],
                ["main.modelGroupRecommended", engineOptions.filter((option) => !installedEngines.has(option.key) && option.recommended)],
                ["main.modelGroupOther", engineOptions.filter((option) => !installedEngines.has(option.key) && !option.recommended)],
              ] as const).map(([labelKey, options]) => options.length === 0 ? null : (
                <optgroup key={labelKey} label={t(labelKey)}>
                  {options.map((option) => (
                    <option key={option.key} value={option.key}>{engineOptionLabel(option, t)}</option>
                  ))}
                </optgroup>
              ))}
              {!engineOptions.some((option) => option.key === engine) && <option value={engine}>{engine}</option>}
            </select>
            <button
              type="button"
              className="dictation-model-browse"
              onClick={() => onNavigate("settings")}
            >
              {t("main.browseModels")}
            </button>
            {engine === "alibaba-asr" && (
              <>
                <label htmlFor="dictation-cloud-model">{t("main.cloudModelLabel")}</label>
                <select
                  id="dictation-cloud-model"
                  value={alibabaAsrModel}
                  disabled={modelSelectionDisabled || alibabaAsrModelsLoading}
                  onChange={(event) => { void handleAlibabaAsrModelSelect(event.target.value); }}
                >
                  {Array.from(new Set([alibabaAsrModel, ...alibabaAsrModels])).map((model) => (
                    <option key={model} value={model}>{model}</option>
                  ))}
                </select>
              </>
            )}
          </div>
          <div className="recording-dock-status">
          <StatusIndicator
            stage={stage}
            isReady={isReady}
            isStarting={isStarting}
            isRecording={isRecording}
            isProcessing={isProcessing}
            device={device}
            gpuName={gpuName}
            downloadProgress={downloadProgress}
            downloadMessage={downloadMessage}
            isDownloading={isDownloading}
            downloadModels={triggerDownload}
            cancelDownload={cancelDownload}
          >

          </StatusIndicator>
          </div>
          </div>
        </section>
        <div className="dashboard-grid">
        {/* Results */}
        <section ref={resultsRef} className="results-area" aria-label={t("main.workspaceLabel")}>
          <div className="transcript-heading">
            <h2><FileText size={16} aria-hidden="true" />{t("main.recentDictations")}</h2>
            <div className="studio-toolbar-actions">
            <button onClick={() => flushPendingEditAndNavigate("history")}>{t("main.viewHistory")} <ArrowUpRight size={14} aria-hidden="true" /></button>
            </div>
          </div>
          {!transcriptionResult && !isProcessing && history.length === 0 && (
            <div className="transcript-empty">
              <div className="studio-empty-mark"><AudioLines size={28} strokeWidth={1.3} aria-hidden="true" /></div>
              <h3>{t("main.dashboardEmptyTitle")}</h3>
              <p>{t("main.dashboardEmptySubtitle")}</p>
              <div className="studio-shortcut"><kbd>{hotkeyDisplay || "F2"}</kbd><span>{isToggleMode ? t("main.studioToggleHint") : t("main.studioHoldHint")}</span></div>
            </div>
          )}
          {polishAuditWarning && (
            <div className="polish-audit-notice" role="status">
              <p>{t("settings.jevAuditWarning")}</p>
              {originalAsrText && <details><summary>{t("settings.jevAuditOriginal")}</summary><p>{originalAsrText}</p></details>}
            </div>
          )}
          <TranscriptionResult
            text={transcriptionResult}
            originalText={originalAsrText}
            baselineText={editBaselineText}
            isProcessing={isProcessing}
            copiedId={copiedId}
            onCopy={handleCopy}
            onDraftChange={handleDraftChange}
            onTextChange={handleTextChange}
            durationSec={durationSec}
            charCount={charCount}
            detectedLanguage={detectedLanguage}
            editGrabStatus={editGrabStatus}
            timing={timing}
          />
          <TranscriptionHistory
            history={history.slice(0, 5)}
            currentResult={transcriptionResult}
            copiedId={copiedId}
            onCopy={handleCopy}
          />

        </section>

        <aside className="dashboard-setup" aria-label={t("main.yourSetup")}>
          <h2><Settings size={16} aria-hidden="true" />{t("main.yourSetup")}</h2>
          <dl className="setup-details">
            <div><dt>{t("main.modelLabel")}</dt><dd>{engineLabel(engine)}</dd></div>
            <div><dt>{t("main.shortcutLabel")}</dt><dd><kbd>{hotkeyDisplay || "F2"}</kbd><span>{isToggleMode ? t("main.studioToggleHint") : t("main.studioHoldHint")}</span></dd></div>
          </dl>
          <button className="workspace-settings-link" onClick={() => flushPendingEditAndNavigate("settings")}>
            {t("main.configure")}<ArrowUpRight size={14} aria-hidden="true" />
          </button>
          <p className="setup-app-hint"><Keyboard size={15} aria-hidden="true" />{t("main.footerActiveApp")} {t("main.footerNoSwitch")}</p>
          {/* First-use onboarding hint */}
          {!onboardingDismissed && !transcriptionResult && !isProcessing && isReady && history.length === 0 && (
            <div className="onboarding-hero-card">
                <div className="result-card-header">
                  <span className="result-card-title">
                    <span className="result-dot" />
                    {t("main.quickStart")}
                  </span>
                  <button
                    aria-label={t("common.close")}
                    className="icon-btn icon-btn-sm"
                    onClick={() => {
                      setOnboardingDismissed(true);
                      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
                    }}
                  >
                    <X size={12} strokeWidth={1.5} />
                  </button>
                </div>
                <div className="onboarding-body">
                  <div className="onboarding-step-row">
                    <span className="onboarding-step-pill">1</span>
                    <p>
                      {isToggleMode ? t("main.pressHotkey") : t("main.holdHotkey")} <strong className="onboarding-accent">{hotkeyDisplay || "F2"}</strong> {isToggleMode ? t("main.step1ToggleSuffix") : t("main.step1HoldSuffix")}
                    </p>
                  </div>
                  <div className="onboarding-step-row">
                    <span className="onboarding-step-pill">2</span>
                    <p>{t("main.step2Text")}</p>
                  </div>
                  <p className="onboarding-subtext">{t("main.onboardingSubtext")}</p>
                </div>
            </div>
          )}
        </aside>
        </div>
        </div>
      </div>
    </div>
  );
}
