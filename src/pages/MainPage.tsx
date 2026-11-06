import { useState, useEffect, useLayoutEffect, useRef, useCallback } from "react";
import { ArrowUpRight, AudioLines, Settings, X } from "lucide-react";
import { toast } from "sonner";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { copyToClipboard, listInstalledModels, submitUserCorrection } from "@/api/tauri";
import RecordingButton from "@/components/RecordingButton";
import SetupWizard from "@/components/SetupWizard";
import StatusIndicator from "@/components/StatusIndicator";
import TranscriptionResult from "@/components/TranscriptionResult";
import TranscriptionHistory from "@/components/TranscriptionHistory";
import { useAsrEngineSettings } from "@/hooks/useAsrEngineSettings";
import { engineOptions, engineOptionLabel } from "@/lib/asrEngineOptions";
import { findCatalogModel } from "@/lib/asrCatalog";
import { useDownloadEstimate } from "@/hooks/useDownloadEstimate";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useWaveformBars, WAVEFORM_BAR_COUNT, WAVEFORM_LIVE_TAIL } from "@/hooks/useWaveformBars";
import { useTranslation } from "react-i18next";
import { ONBOARDING_DISMISSED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { prefersReducedMotion } from "@/lib/motion";
import { formatDuration } from "@/lib/formatDuration";

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
    isDownloading, isDownloadPaused, modelError, hotkeyDisplay,
    downloadModels: triggerDownload, cancelDownload, pauseDownload, resumeDownload, retryModel,
  } = useRecordingContext();

  const engineLabel = useCallback((value: string) => {
    const option = engineOptions.find((item) => item.key === value);
    return option ? engineOptionLabel(option, t) : value;
  }, [t]);
  const {
    engine, engineLoading, handleEngineSwitch,
    alibabaAsrModel, alibabaAsrModelSaving, alibabaAsrModels, alibabaAsrModelsLoading, handleAlibabaAsrModelSelect,
  } = useAsrEngineSettings({ engineLabel, retryModel });
  const isReady = modelReady && !engineLoading && !alibabaAsrModelSaving;
  const stage = engineLoading || alibabaAsrModelSaving ? "checking" : modelStage;
  const modelSelectionDisabled = engineLoading || alibabaAsrModelSaving || isStarting || isRecording || isProcessing || isDownloading;

  const downloadTotalBytes = findCatalogModel(engine)?.size ?? null;
  const downloadEstimate = useDownloadEstimate(downloadProgress, downloadTotalBytes, isDownloading);

  const [installedEngines, setInstalledEngines] = useState<ReadonlySet<string>>(() => new Set());
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => readLocalStorage(ONBOARDING_DISMISSED_KEY) === "true");
  const isToggleMode = readLocalStorage(RECORDING_MODE_KEY) === "toggle";
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const rundownRef = useRef<HTMLElement>(null);
  const waveRef = useRef<HTMLDivElement>(null);
  const previousResult = useRef<{ id: string; top: number } | null>(null);

  useWaveformBars(waveRef, isRecording);

  // Groups the model list so the current download is easy to find.
  useEffect(() => {
    let disposed = false;
    listInstalledModels()
      .then((keys) => { if (!disposed) setInstalledEngines(new Set(keys)); })
      .catch(() => { if (!disposed) setInstalledEngines(new Set()); });
    return () => { disposed = true; };
  }, [engine, modelReady, stage]);

  useLayoutEffect(() => {
    const root = resultsRef.current;
    const rundown = rundownRef.current;
    const current = history[0];
    const card = root?.querySelector<HTMLElement>(".result-card");
    if (!root || !card || !current || current.text !== transcriptionResult) return;
    const previous = previousResult.current;
    if (previous && previous.id !== current.id && !prefersReducedMotion()) {
      const oldItem = Array.from(rundown?.querySelectorAll<HTMLElement>(".history-item") ?? [])
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
  useEffect(() => {
    if (transcriptionResult && !onboardingDismissed) {
      setOnboardingDismissed(true);
      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
    }
  }, [transcriptionResult, onboardingDismissed]);
  // A user with history never saw this guide; treat them as set up so a
  // missing model still downloads automatically on a cold start.
  useEffect(() => {
    if (history.length > 0 && readLocalStorage(ONBOARDING_DISMISSED_KEY) !== "true") {
      setOnboardingDismissed(true);
      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
    }
  }, [history.length]);
  useEffect(() => () => { if (copyTimerRef.current) clearTimeout(copyTimerRef.current); }, []);

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
    } else {
      setTranscriptionResult(newText);
    }
  }, [correctionSubmit, editBaselineText, originalAsrText, recordingMode, setEditBaselineText, setTranscriptionResult]);

  const flushPendingEditAndNavigate = useCallback((target: "settings" | "history") => {
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    onNavigate(target);
  }, [onNavigate]);

  const handleToggleRecording = useCallback(() => {
    if (isRecording || isStarting) void stopRecording();
    else if (isReady) void startRecording();
  }, [isReady, isRecording, isStarting, startRecording, stopRecording]);

  // Space starts or stops dictation when focus is not in a text field. Escape
  // stops a recording that is already starting or running. Both ignore
  // modified keys so they never shadow a system shortcut.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      const target = event.target instanceof HTMLElement ? event.target : null;
      if (target && (target.isContentEditable
        || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))) return;
      if (event.key === " " || event.code === "Space") {
        // A focused button or link already activates on Space.
        if (target?.closest("button, [role='button'], a[href]")) return;
        event.preventDefault();
        handleToggleRecording();
        return;
      }
      if (event.key === "Escape" && (isRecording || isStarting)) {
        event.preventDefault();
        void stopRecording();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleToggleRecording, isRecording, isStarting, stopRecording]);

  const stateLabel = isRecording
    ? t("main.statusListening")
    : isStarting
      ? t("main.statusConnecting")
      : isProcessing
        ? t("main.statusProcessing")
        : isReady
          ? t("main.statusReady")
          : stage === "error"
            ? t("main.statusError")
            : t("main.statusPreparing");

  const hasError = Boolean(recordingError || modelError) && !errorDismissed;
  const showEmpty = !transcriptionResult && !isProcessing && history.length === 0;

  return (
    <div className="page-root signal-page">
      <h1 className="signal-visually-hidden">{t("viewTitle.main")}</h1>
      <div className={`signal-body ${animClass}`.trim()}>

        <div className="signal-main">

          {/* ── signal panel: the voice, drawn ── */}
          <section className="wave-panel" aria-label={t("main.recordingControlsLabel")}>
            <div className="wave-head">
              <RecordingButton
                isStarting={isStarting}
                isRecording={isRecording}
                isProcessing={isProcessing}
                isReady={isReady}
                onToggle={handleToggleRecording}
              />
              <div className="wave-status">
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
                  isDownloadPaused={isDownloadPaused}
                  downloadEstimate={downloadEstimate}
                  downloadModels={triggerDownload}
                  cancelDownload={cancelDownload}
                  pauseDownload={pauseDownload}
                  resumeDownload={resumeDownload}
                />
              </div>
              <div className="wave-clock">
                <span className="wave-clock-value">{formatDuration(durationSec)}</span>
              </div>
            </div>

            <div className="wave-bars" ref={waveRef} aria-hidden="true">
              {Array.from({ length: WAVEFORM_BAR_COUNT }, (_, index) => (
                <i key={index} data-live={index >= WAVEFORM_BAR_COUNT - WAVEFORM_LIVE_TAIL ? "" : undefined} />
              ))}
            </div>
            <div className="wave-base" aria-hidden="true" />

            <div className="wave-foot">
              <span className="signal-label">{t("signal.input")}</span>
              <div className="wave-foot-right">
                <span className="signal-label">{isToggleMode ? t("main.hotkeyHintToggle") : t("main.hotkeyHintHold")}</span>
                <kbd className="signal-key">{hotkeyDisplay || "F2"}</kbd>
              </div>
            </div>
          </section>

          {/* ── transcript ── */}
          <section className="transcript-panel" ref={resultsRef} aria-label={t("main.workspaceLabel")}>
            <div className="transcript-head">
              <h2 className="signal-visually-hidden">{t("result.title")}</h2>
              <span className="signal-tag" data-live={isRecording || isStarting ? "" : undefined}>
                {isRecording || isStarting ? t("main.statusListening") : t("result.title")}
              </span>
              <span className="signal-label">
                {recordingMode === "assistant" ? t("historyPage.assistant") : t("historyPage.dictation")}
                {charCount ? ` · ${charCount}` : ""}
              </span>
              <div className="transcript-actions">
                <label className="signal-select">
                  <span className="signal-label">{t("main.modelLabel")}</span>
                  <select
                    aria-label={t("main.modelLabel")}
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
                </label>
                <button
                  type="button"
                  className="dictation-model-browse"
                  onClick={() => flushPendingEditAndNavigate("settings")}
                >
                  {t("main.browseModels")}
                </button>
                {engine === "alibaba-asr" && (
                  <label className="signal-select">
                    <span className="signal-label">{t("main.cloudModelLabel")}</span>
                    <select
                      aria-label={t("main.cloudModelLabel")}
                      value={alibabaAsrModel}
                      disabled={modelSelectionDisabled || alibabaAsrModelsLoading}
                      onChange={(event) => { void handleAlibabaAsrModelSelect(event.target.value); }}
                    >
                      {Array.from(new Set([alibabaAsrModel, ...alibabaAsrModels])).map((model) => (
                        <option key={model} value={model}>{model}</option>
                      ))}
                    </select>
                  </label>
                )}
              </div>
            </div>

            <div className="transcript-body">
              {hasError && (
                <div className="signal-error" role="alert">
                  <p>{recordingError || modelError}</p>
                  <div className="signal-error-actions">
                    {stage === "error" && <button className="signal-link" onClick={retryModel}>{t("common.retry")}</button>}
                    <button className="icon-btn icon-btn-sm" aria-label={t("common.close")} onClick={() => setErrorDismissed(true)}>
                      <X size={11} />
                    </button>
                  </div>
                </div>
              )}

              {polishAuditWarning && (
                <div className="signal-notice" role="status">
                  <p>{t("settings.jevAuditWarning")}</p>
                  {originalAsrText && <details><summary>{t("settings.jevAuditOriginal")}</summary><p>{originalAsrText}</p></details>}
                </div>
              )}

              {showEmpty && (
                <div className="signal-empty">
                  <AudioLines size={18} strokeWidth={1.4} aria-hidden="true" />
                  <div>
                    <h3>{t("main.dashboardEmptyTitle")}</h3>
                    <p>{t("main.dashboardEmptySubtitle")}</p>
                  </div>
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

              {!onboardingDismissed && !transcriptionResult && !isProcessing && history.length === 0 && (
                <SetupWizard
                  engine={engine}
                  installed={installedEngines}
                  isToggleMode={isToggleMode}
                  stage={stage}
                  isReady={isReady}
                  isDownloading={isDownloading}
                  downloadProgress={downloadProgress}
                  downloadMessage={downloadMessage}
                  onSelectEngine={handleEngineSwitch}
                  onDownload={triggerDownload}
                  onRetryModel={retryModel}
                  onCancelDownload={cancelDownload}
                  hotkeyDisplay={hotkeyDisplay}
                  onOpenSettings={() => onNavigate("settings")}
                  onDismiss={() => {
                    setOnboardingDismissed(true);
                    writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
                  }}
                />
              )}
            </div>
          </section>

          <div className="signal-hsplit" aria-hidden="true" />

          {/* ── rundown ── */}
          <section className="rundown-panel" ref={rundownRef} aria-label={t("main.recentDictations")}>
            <div className="rundown-head">
              <h2 className="signal-label">{t("main.recentDictations")}</h2>
              <span className="rundown-count">{history.length}</span>
              <button className="signal-link" onClick={() => flushPendingEditAndNavigate("history")}>
                {t("main.viewHistory")} <ArrowUpRight size={11} aria-hidden="true" />
              </button>
            </div>
            <div className="rundown-cols" aria-hidden="true">
              <span>{t("signal.time")}</span>
              <span>{t("signal.transcript")}</span>
              <span />
            </div>
            <div className="rundown-rows">
              <TranscriptionHistory
                history={history.slice(0, 12)}
                currentResult={transcriptionResult}
                copiedId={copiedId}
                onCopy={handleCopy}
              />
            </div>
          </section>

        </div>

        <div className="signal-vsplit" aria-hidden="true" />

        {/* ── sources rail ── */}
        <aside className="signal-rail" aria-label={t("main.yourSetup")}>
          <div className="rail-head">
            <h2 className="signal-label">{t("main.yourSetup")}</h2>
            <button className="icon-btn icon-btn-sm" aria-label={t("main.configure")} onClick={() => flushPendingEditAndNavigate("settings")}>
              <Settings size={12} />
            </button>
          </div>

          <div className="rail-count">
            <span className="rail-count-value">{String(history.length).padStart(2, "0")}</span>
            <span className="signal-label">{t("main.recentDictations")}</span>
          </div>

          <dl className="rail-sources">
            <div>
              <dt className="signal-label">{t("main.modelLabel")}</dt>
              <dd>{engineLabel(engine)}</dd>
            </div>
            <div>
              <dt className="signal-label">{t("main.shortcutLabel")}</dt>
              <dd><kbd className="signal-key">{hotkeyDisplay || "F2"}</kbd></dd>
            </div>
            <div>
              <dt className="signal-label">{t("signal.status")}</dt>
              <dd className="rail-state" data-ready={isReady ? "" : undefined}>
                <span className="rail-dot" aria-hidden="true" />
                {stateLabel}
              </dd>
            </div>
            <div>
              <dt className="signal-label">{t("signal.destination")}</dt>
              <dd className="rail-muted">{t("main.footerNoSwitch")}</dd>
            </div>
          </dl>

          <div className="rail-foot">
            <button className="signal-link" onClick={() => flushPendingEditAndNavigate("settings")}>
              {t("main.configure")} <ArrowUpRight size={11} aria-hidden="true" />
            </button>
          </div>
        </aside>

      </div>
    </div>
  );
}
