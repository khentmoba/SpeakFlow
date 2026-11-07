import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Cpu, Globe, Loader2, Download, Pause, Play } from "lucide-react";
import type { ModelStage } from "@/hooks/useModelStatus";
import { formatDownloadSpeed, formatEta, type DownloadEstimate } from "@/lib/downloadEstimate";

interface StatusIndicatorProps {
  stage: ModelStage;
  isReady: boolean;
  isStarting: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  device: string | null;
  gpuName: string | null;
  downloadProgress: number;
  downloadMessage: string | null;
  isDownloading: boolean;
  isDownloadPaused?: boolean;
  downloadEstimate?: DownloadEstimate | null;
  downloadModels: () => void;
  cancelDownload: () => void;
  pauseDownload?: () => void;
  resumeDownload?: () => void;
  children?: ReactNode;
}

function getDeviceLabel(device: string, gpuName: string | null, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (device === "cloud") return t("status.online");
  if (device === "cuda" || device === "gpu") return gpuName || "GPU";
  return "CPU";
}

function getLoadingProgress(message: string | null): number | null {
  if (!message) return null;
  const match = message.match(/(\d{1,3})%/);
  if (!match) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value)) return null;
  return Math.max(0, Math.min(100, value));
}

function getStatusText(
  isStarting: boolean,
  isRecording: boolean,
  isProcessing: boolean,
  isReady: boolean,
  stage: ModelStage,
  downloadProgress: number,
  downloadMessage: string | null,
  isPaused: boolean,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (isStarting) return t("status.connectingMicrophone");
  if (isRecording) return t("status.listening");
  if (isProcessing) return t("status.recognizing");
  if (isReady) return t("status.clickToStart");
  if (isPaused) return t("status.downloadPaused");
  if (stage === "downloading") {
    return downloadProgress > 1
      ? t("status.downloadingModel", { progress: Math.round(downloadProgress) })
      : (downloadMessage ?? t("status.downloadPreparing"));
  }
  if (stage === "error") return t("status.modelUnavailable");
  if (stage === "need_download") return t("status.needDownload");
  if (stage === "need_repair") return t("status.needRepair");
  if (stage === "loading") return downloadMessage || t("status.modelLoading");
  return t("status.preparing");
}

function getChipLabel(stage: ModelStage, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (stage === "ready") return t("status.chipReady");
  if (stage === "error") return t("status.chipError");
  if (stage === "need_download") return t("status.needDownload");
  if (stage === "need_repair") return t("status.chipRepair");
  if (stage === "downloading") return t("status.chipDownloading");
  if (stage === "loading") return t("status.chipLoading");
  return t("status.chipPreparing");
}

export default function StatusIndicator({
  stage, isReady, isStarting, isRecording, isProcessing,
  device, gpuName, downloadProgress, downloadMessage,
  isDownloading, isDownloadPaused = false, downloadEstimate = null,
  downloadModels, cancelDownload, pauseDownload, resumeDownload, children,
}: StatusIndicatorProps) {
  const { t } = useTranslation();
  const showProgressBar = stage === "downloading" || stage === "loading";
  const loadingProgress = stage === "loading" ? getLoadingProgress(downloadMessage) : null;
  const determinateProgress = stage === "downloading"
    ? (downloadProgress > 1 ? downloadProgress : null)
    : loadingProgress;

  return (
    <>
      <div className="chip-container">
        <span className="chip model-status" data-state={stage} role="status">
          {stage === "checking" || stage === "loading" || stage === "downloading" || isStarting || isProcessing
            ? <Loader2 size={11} className="animate-spin" aria-hidden="true" />
            : <span className="model-status-dot" aria-hidden="true" />}
          <span>{isStarting ? t("main.statusConnecting") : isRecording ? t("status.chipActive") : isProcessing ? t("status.recognizing") : getChipLabel(stage, t)}</span>
        </span>
        {device && isReady && (
          <span className={`chip chip-device ${isReady ? "animate-success" : "animate-fade-in"}`}>
            <span
              className={`chip-dot ${device === "cloud" ? "chip-dot-cloud" : (device === "cuda" || device === "gpu") ? "chip-dot-gpu" : "chip-dot-cpu"}`}
              aria-hidden="true"
            />
            {device === "cloud" ? <Globe size={11} strokeWidth={1.8} /> : <Cpu size={11} strokeWidth={1.8} />}
            <span>{getDeviceLabel(device, gpuName, t)}</span>
          </span>
        )}
      </div>

      {children}

      <p
        key={isStarting ? "starting" : isRecording ? "recording" : isProcessing ? "processing" : isReady ? "ready" : stage}
        aria-live="polite"
        className="status-text animate-text-swap"
      >
        {getStatusText(isStarting, isRecording, isProcessing, isReady, stage, downloadProgress, downloadMessage, isDownloadPaused, t)}
      </p>

      {(stage === "need_download" || stage === "need_repair") && !isDownloading && (
        <button onClick={() => (isDownloadPaused && resumeDownload ? resumeDownload() : downloadModels())} className="btn-primary btn-primary-sm" style={{ marginTop: 4 }}>
          {isDownloadPaused ? <Play size={12} /> : <Download size={12} />}
          {t(isDownloadPaused ? "status.resumeDownload" : stage === "need_repair" ? "status.repairModels" : "status.startDownload")}
        </button>
      )}
      {stage === "need_download" && isDownloading && (
        <div className="download-info">
          <span>{t("status.anotherDownloading")}</span>
          <button onClick={() => cancelDownload()} className="btn-ghost-sm">{t("status.cancelDownload")}</button>
        </div>
      )}
      {showProgressBar && (
        <div className="download-progress">
          <div
            role="progressbar"
            aria-valuenow={determinateProgress !== null ? Math.round(determinateProgress) : undefined}
            aria-valuemin={0}
            aria-valuemax={100}
            className={`download-progress-track${determinateProgress !== null && determinateProgress >= 100 ? " progress-complete" : ""}`}
          >
            {determinateProgress !== null
              ? <div style={{ height: "100%", width: "100%", background: determinateProgress >= 100 ? "var(--color-success)" : "var(--color-accent)", borderRadius: 4, transform: `scaleX(${Math.max(0, Math.min(100, determinateProgress)) / 100})`, transformOrigin: "left", transition: "transform 0.5s ease, background 0.3s ease" }} />
              : <div className="download-pulse-bar" />}
          </div>
          {stage === "downloading" && (
            <div className="download-actions">
              {downloadEstimate && (
                <span className="download-estimate">
                  {formatDownloadSpeed(downloadEstimate.bytesPerSecond)}
                  {" · "}
                  {t("status.downloadEta", { time: formatEta(downloadEstimate.secondsRemaining) })}
                </span>
              )}
              {pauseDownload && (
                <button onClick={() => pauseDownload()} className="btn-ghost-sm">
                  <Pause size={11} aria-hidden="true" /> {t("status.pauseDownload")}
                </button>
              )}
              <button onClick={() => cancelDownload()} className="btn-ghost-sm">{t("status.cancelDownload")}</button>
            </div>
          )}
        </div>
      )}
    </>
  );
}
