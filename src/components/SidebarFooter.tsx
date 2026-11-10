import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useRecordingContext } from "@/contexts/RecordingContext";
import Kbd from "./Kbd";
import type { AppView } from "./AppNavigation";

interface SidebarFooterProps {
  onNavigate: (view: AppView) => void;
  updateAvailable?: boolean;
  latestVersion?: string | null;
}

export default function SidebarFooter({
  onNavigate,
  updateAvailable = false,
  latestVersion = null,
}: SidebarFooterProps) {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");

  useEffect(() => {
    void getVersion().then(setVersion).catch(() => {});
  }, []);

  const {
    isRecording, isStarting, isProcessing, isReady, stage,
    hotkeyDisplay,
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

  return (
    <footer className="app-sidebar-footer" aria-label={t("statusBar.label")}>
      <div className="app-sidebar-status">
        <span className="status-dot" data-state={state} aria-hidden="true" />
        <span className="app-sidebar-status-text">{label}</span>
        {hotkeyDisplay && <Kbd combo={hotkeyDisplay} className="app-sidebar-key" />}
      </div>
      <div className="app-sidebar-meta">
        {version && <span className="app-sidebar-version">v{version}</span>}
        <button
          type="button"
          className="app-sidebar-link"
          onClick={() => onNavigate("updates")}
        >
          {updateAvailable && latestVersion
            ? t("settings.newVersionAvailable", { version: latestVersion })
            : t("settings.checkUpdate")}
        </button>
      </div>
    </footer>
  );
}
