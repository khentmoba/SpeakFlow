import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { useRecordingContext } from "@/contexts/RecordingContext";

export type SidebarStatusState = "recording" | "processing" | "ready" | "error" | "idle";

export interface RecordingStatusInput {
  isRecording: boolean;
  isStarting: boolean;
  isProcessing: boolean;
  isReady: boolean;
  stage: string;
}

export function resolveSidebarStatus({
  isRecording,
  isStarting,
  isProcessing,
  isReady,
  stage,
}: RecordingStatusInput): { state: SidebarStatusState; labelKey: string } {
  if (isRecording) return { state: "recording", labelKey: "main.statusListening" };
  if (isStarting) return { state: "recording", labelKey: "main.statusConnecting" };
  if (isProcessing) return { state: "processing", labelKey: "main.statusProcessing" };
  if (isReady) return { state: "ready", labelKey: "main.statusReady" };
  if (stage === "error") return { state: "error", labelKey: "main.statusError" };
  return { state: "idle", labelKey: "main.statusPreparing" };
}

export function useSidebarStatus(
  updateAvailable = false,
  latestVersion: string | null = null,
) {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");

  useEffect(() => {
    let active = true;
    void getVersion()
      .then((value) => {
        if (active) setVersion(value);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const { isRecording, isStarting, isProcessing, isReady, stage, hotkeyDisplay } =
    useRecordingContext();

  const { state, labelKey } = resolveSidebarStatus({
    isRecording,
    isStarting,
    isProcessing,
    isReady,
    stage,
  });

  const updateLabel =
    updateAvailable && latestVersion
      ? t("settings.newVersionAvailable", { version: latestVersion })
      : t("settings.checkUpdate");

  return {
    version,
    state,
    label: t(labelKey),
    hotkeyDisplay,
    updateLabel,
    statusBarLabel: t("statusBar.label"),
  };
}
