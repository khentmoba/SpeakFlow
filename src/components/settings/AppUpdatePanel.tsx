import { Download, RefreshCw, ShieldCheck, ExternalLink } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openAppReleasePage } from "@/api/tauri";
import { useAppUpdates } from "@/contexts/AppUpdateContext";

export default function AppUpdatePanel({ appVersion }: { appVersion: string }) {
  const { t } = useTranslation();
  const { updateChecking, updateInstalling, updateProgress, updateStatusText,
    latestAvailableVersion, latestRelease, handleCheckForUpdates, handleInstallUpdate } = useAppUpdates();
  const installing = updateProgress?.phase === "installing";
  const releaseNotes = latestRelease?.notes?.trim() ?? "";
  const releaseUrl = latestRelease?.url ?? null;
  const percent = updateProgress?.total && !installing
    ? Math.min(100, Math.max(0, updateProgress.downloaded * 100 / updateProgress.total)) : undefined;

  return (
    <section className="settings-card app-update-panel" data-nav-id="updates" aria-labelledby="update-title">
      <div className="settings-section-header">
        <RefreshCw size={15} className={updateChecking ? "icon-accent update-spinner" : "icon-accent"} aria-hidden="true" />
        <h2 id="update-title" className="settings-section-title">{t("settings.update")}</h2>
      </div>
      <div className="settings-row settings-update-row">
        <div className="settings-column settings-update-details">
          <span className="permission-label">{t("settings.currentVersion", { version: appVersion || "..." })}</span>
          <p className="settings-hint" role="status" aria-live="polite">
            {updateStatusText || t("updates.autoCheckHint")}
          </p>
          {latestAvailableVersion && <p className="settings-hint">{t("settings.newVersionAvailable", { version: latestAvailableVersion })}</p>}
        </div>
        <button className="test-btn settings-update-button" disabled={updateChecking} aria-busy={updateChecking}
          onClick={() => void (latestAvailableVersion ? handleInstallUpdate() : handleCheckForUpdates())}>
          {updateChecking && <RefreshCw size={13} className="update-spinner" aria-hidden="true" />}
          {updateInstalling ? t("settings.updating") : updateChecking ? t("settings.checking")
            : latestAvailableVersion ? t("settings.installUpdate") : t("settings.checkUpdate")}
        </button>
      </div>
      {updateInstalling && (
        <div className="update-transfer" data-phase={updateProgress?.phase}>
          <div className="update-progress-track" role="progressbar" aria-label={installing ? "Installing update" : "Downloading update"}
            aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent === undefined ? undefined : Math.floor(percent)}
            aria-valuetext={installing ? "Installing verified update. SpeakFlow will restart." : undefined}>
            <div className="update-progress-fill" data-indeterminate={percent === undefined}
              style={percent === undefined ? undefined : { transform: `scaleX(${percent / 100})` }} />
          </div>
          <div className="update-steps" aria-hidden="true">
            <span data-active={!installing}><Download size={13} /> {t("updates.download")}</span>
            <span data-active={installing}><ShieldCheck size={13} /> {t("updates.verifyAndInstall")}</span>
          </div>
        </div>
      )}
      <p className="settings-hint settings-update-source">{t("updates.scheduleHint")}</p>
      {latestAvailableVersion && releaseNotes && (
        <div className="update-notes">
          <div className="update-notes-head">
            <span className="permission-label">{t("updates.releaseNotes")}</span>
            {releaseUrl && (
              <button
                type="button"
                className="btn-ghost update-notes-link"
                onClick={() => { void openAppReleasePage(releaseUrl).catch(() => toast.error(t("updates.releaseOpenFailed"))); }}
              >
                {t("updates.viewRelease")}
                <ExternalLink size={11} aria-hidden="true" />
              </button>
            )}
          </div>
          <p className="update-notes-body">{releaseNotes}</p>
        </div>
      )}
      {latestAvailableVersion && <p className="settings-hint settings-update-source">{t("settings.updateSource")}</p>}
    </section>
  );
}
