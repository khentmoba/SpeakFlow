import { useTranslation } from "react-i18next";
import { useRecordingContext } from "@/contexts/RecordingContext";

import { formatDuration } from "@/lib/formatDuration";

function deviceLabel(device: string | null, t: (key: string) => string): string {
  if (device === "cloud") return t("status.online");
  if (device === "cuda" || device === "gpu") return "GPU";
  if (device === "cpu") return "CPU";
  return "";
}

/**
 * Shell status bar. Reads the live recording state so the user can see what the
 * app is doing from any workspace, not only from Dictation.
 */
export default function ShellStatusBar() {
  const { t } = useTranslation();
  const {
    isRecording, isStarting, isProcessing, isReady, stage,
    durationSec, device, hotkeyDisplay, recordingMode,
  } = useRecordingContext();

  const state = isRecording || isStarting
    ? "recording"
    : isProcessing
      ? "processing"
      : isReady
        ? "ready"
        : stage === "error"
          ? "error"
          : "idle";

  const label = isRecording
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

  const deviceText = deviceLabel(device, t);
  const modeLabel = recordingMode === "assistant" ? t("historyPage.assistant") : t("historyPage.dictation");

  return (
    <footer className="app-statusbar" aria-label={t("statusBar.label")}>
      <div className="status-seg">
        <span className="status-dot" data-state={state} aria-hidden="true" />
        <span>{label}</span>
      </div>
      <div className="status-seg">
        <span className="status-value">{formatDuration(durationSec)}</span>
      </div>
      <div className="status-seg">
        <span>{modeLabel}</span>
      </div>
      {deviceText && (
        <div className="status-seg">
          <span>{deviceText}</span>
        </div>
      )}
      <div className="status-seg">
        <span className="status-value">{hotkeyDisplay || "F2"}</span>
        <span>{t("main.shortcutLabel")}</span>
      </div>
    </footer>
  );
}
