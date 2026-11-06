import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, Download, Mic, Settings } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  listInputDevices,
  setInputDevice,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
} from "@/api/tauri";
import type { InputDeviceInfo } from "@/types";
import Kbd from "@/components/Kbd";
import { findCatalogModel, formatModelSize } from "@/lib/asrCatalog";
import { engineDisplayLabel } from "@/lib/asrEngineOptions";
import { ENGLISH_RECOMMENDED_MODEL } from "@/lib/recommendedModel";
import { INPUT_DEVICE_STORAGE_KEY } from "@/lib/constants";
import { writeLocalStorage } from "@/lib/storage";
import type { ModelStage } from "@/hooks/useModelStatus";

interface SetupWizardProps {
  engine: string;
  installed: ReadonlySet<string>;
  isToggleMode: boolean;
  stage: ModelStage;
  isReady: boolean;
  isDownloading: boolean;
  downloadProgress: number;
  downloadMessage: string | null;
  onSelectEngine: (engine: string) => Promise<boolean>;
  onDownload: () => void;
  onRetryModel: () => void;
  onCancelDownload: () => void;
  hotkeyDisplay: string;
  onOpenSettings: () => void;
  onDismiss: () => void;
}

/**
 * First-run guide: microphone, the recommended engine, the hotkey, and the
 * first sentence. Every step is visible at once so the user can skip ahead;
 * the card disappears after the first transcript.
 */
export default function SetupWizard({
  engine,
  installed,
  isToggleMode,
  stage,
  isReady,
  isDownloading,
  downloadProgress,
  downloadMessage,
  onSelectEngine,
  onDownload,
  onRetryModel,
  onCancelDownload,
  hotkeyDisplay,
  onOpenSettings,
  onDismiss,
}: SetupWizardProps) {
  const { t } = useTranslation();
  const recommended = findCatalogModel(ENGLISH_RECOMMENDED_MODEL);
  const recommendedInstalled = recommended ? installed.has(recommended.key) : false;
  const recommendedSelected = recommended ? engine === recommended.key : false;
  const recommendedFailed = recommendedSelected && recommendedInstalled && !isReady && stage === "error";
  const recommendedStarting = recommendedSelected && recommendedInstalled && !isReady && !recommendedFailed;
  const [devices, setDevices] = useState<InputDeviceInfo[]>([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [level, setLevel] = useState(0);

  useEffect(() => {
    let disposed = false;
    void listInputDevices()
      .then((payload) => {
        if (disposed) return;
        setDevices(payload.devices);
        setSelectedDevice(payload.selectedDeviceName ?? "");
      })
      .catch(() => undefined);
    return () => { disposed = true; };
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let sawLevel = false;
    let rearm: ReturnType<typeof setTimeout> | null = null;
    const startMonitor = () => { void startMicrophoneLevelMonitor().catch(() => undefined); };
    startMonitor();
    // A settings-page round trip queues its own stop; if no level arrives,
    // start again so the meter does not stay at zero for the session.
    rearm = setTimeout(() => { if (!disposed && !sawLevel) startMonitor(); }, 1500);
    void listen<{ level?: number }>("microphone-level", ({ payload }) => {
      if (disposed) return;
      sawLevel = true;
      const value = typeof payload?.level === "number" ? payload.level : 0;
      setLevel(Math.max(0, Math.min(1, value)));
    })
      .then((dispose) => { if (disposed) dispose(); else unlisten = dispose; })
      .catch(() => undefined);
    return () => {
      disposed = true;
      if (rearm) clearTimeout(rearm);
      unlisten?.();
      void stopMicrophoneLevelMonitor().catch(() => undefined);
    };
  }, []);

  const handleDeviceChange = async (name: string) => {
    setSelectedDevice(name);
    try {
      await setInputDevice(name || null);
      writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, name || "");
    } catch {
      // Keep the list usable; the settings page reports the failure in detail.
    }
  };

  const handleUseRecommended = async () => {
    if (!recommended) return;
    // Switch first so the download's disk check reads the recommended model.
    // An already-selected model is a valid starting point, not a no-op.
    const switched = recommendedSelected || await onSelectEngine(recommended.key);
    if (switched && !recommendedInstalled) onDownload();
  };

  return (
    <section className="setup-wizard" aria-labelledby="setup-wizard-title">
      <div className="setup-wizard-head">
        <span className="signal-label" id="setup-wizard-title">{t("setup.title")}</span>
        <button type="button" className="btn-ghost setup-wizard-skip" onClick={onDismiss}>
          {t("setup.skip")}
        </button>
      </div>

      <ol className="setup-wizard-steps">
        <li className="setup-step" data-done={Boolean(selectedDevice) || level > 0}>
          <span className="setup-step-icon" aria-hidden="true">
            {selectedDevice ? <Check size={12} /> : <Mic size={12} />}
          </span>
          <div className="setup-step-body">
            <span className="setup-step-title">{t("setup.micTitle")}</span>
            <p className="setup-step-desc">{t("setup.micDesc")}</p>
            <div className="setup-step-controls">
              <select
                className="settings-input setup-wizard-select"
                aria-label={t("setup.micSelect")}
                value={selectedDevice}
                onChange={(event) => void handleDeviceChange(event.target.value)}
              >
                <option value="">{t("setup.micDefault")}</option>
                {devices.map((device) => (
                  <option key={device.name} value={device.name}>{device.name}</option>
                ))}
              </select>
              <span className="setup-level" aria-hidden="true">
                <span className="setup-level-fill" style={{ transform: `scaleX(${level})` }} />
              </span>
              <span className="setup-level-label">{t("setup.micLevel")}</span>
            </div>
          </div>
        </li>

        <li className="setup-step" data-done={recommendedSelected && recommendedInstalled && isReady}>
          <span className="setup-step-icon" aria-hidden="true">
            {recommendedSelected && recommendedInstalled && isReady ? <Check size={12} /> : <Download size={12} />}
          </span>
          <div className="setup-step-body">
            <span className="setup-step-title">{t("setup.engineTitle")}</span>
            <p className="setup-step-desc">
              {recommended
                ? t("setup.engineDesc", {
                    model: recommended.name,
                    size: formatModelSize(recommended.size),
                  })
                : t("setup.engineFallback")}
            </p>
            {isDownloading ? (
              <div className="setup-step-controls">
                <span className="setup-progress" role="progressbar"
                  aria-valuemin={0} aria-valuemax={100}
                  aria-valuenow={Math.round(downloadProgress)}>
                  <span className="setup-progress-fill" style={{ transform: `scaleX(${downloadProgress / 100})` }} />
                </span>
                <span className="setup-step-meta">
                  {downloadMessage
                    || t("setup.engineDownloadingModel", {
                      model: engineDisplayLabel(engine, t),
                      progress: Math.round(downloadProgress),
                    })}
                </span>
                <button type="button" className="btn-ghost-sm" onClick={onCancelDownload}>
                  {t("setup.engineCancel")}
                </button>
              </div>
            ) : (
              <div className="setup-step-controls">
                <button
                  type="button"
                  className="btn-primary btn-primary-sm"
                  disabled={!recommended || (recommendedSelected && recommendedInstalled && isReady) || recommendedStarting}
                  onClick={() => {
                    if (recommendedFailed) { onRetryModel(); return; }
                    void handleUseRecommended();
                  }}
                >
                  {recommendedSelected && recommendedInstalled && isReady
                    ? t("setup.engineReady")
                    : recommendedFailed
                      ? t("setup.engineRetry")
                      : recommendedStarting
                        ? t("setup.engineStarting")
                        : recommendedInstalled
                          ? t("setup.engineUse")
                          : t("setup.engineDownload")}
                </button>
                {recommendedSelected && !recommendedInstalled && (
                  <span className="setup-step-meta">{t("setup.engineNeedsDownload")}</span>
                )}
              </div>
            )}
          </div>
        </li>

        <li className="setup-step" data-done={false}>
          <span className="setup-step-icon" aria-hidden="true"><Settings size={12} /></span>
          <div className="setup-step-body">
            <span className="setup-step-title">{t("setup.hotkeyTitle")}</span>
            <p className="setup-step-desc">
              {t(isToggleMode ? "setup.hotkeyDescToggle" : "setup.hotkeyDesc", {
                hotkey: hotkeyDisplay || "F2",
              })}
            </p>
            <div className="setup-step-controls">
              <Kbd combo={hotkeyDisplay || "F2"} />
              <button type="button" className="btn-ghost-sm" onClick={onOpenSettings}>
                {t("setup.hotkeyChange")}
              </button>
            </div>
          </div>
        </li>

        <li className="setup-step" data-done={false}>
          <span className="setup-step-icon" aria-hidden="true"><Mic size={12} /></span>
          <div className="setup-step-body">
            <span className="setup-step-title">{t("setup.tryTitle")}</span>
            <p className="setup-step-desc">
              {t(isToggleMode ? "setup.tryDescToggle" : "setup.tryDesc", {
                hotkey: hotkeyDisplay || "F2",
              })}
            </p>
          </div>
        </li>
      </ol>

      <div className="setup-wizard-foot">
        <span className="setup-wizard-hint">{t("setup.footHint")}</span>
        <button type="button" className="btn-ghost" onClick={onOpenSettings}>
          {t("setup.openSettings")}
        </button>
      </div>
    </section>
  );
}
