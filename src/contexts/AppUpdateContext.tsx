import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { checkAppUpdate, installAppUpdate, type AppUpdateProgress } from "@/api/tauri";

const CHECK_INTERVAL = 60 * 60 * 1000;

function useUpdateController(automatic: boolean) {
  const { t } = useTranslation();
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateStatusText, setUpdateStatusText] = useState("");
  const [latestAvailableVersion, setLatestAvailableVersion] = useState<string | null>(null);
  const [latestRelease, setLatestRelease] = useState<{ notes: string | null; url: string | null } | null>(null);
  const [updateProgress, setUpdateProgress] = useState<AppUpdateProgress | null>(null);
  const pending = useRef(false);
  const lastCheck = useRef(0);

  const handleCheckForUpdates = useCallback(async (silent = false) => {
    if (pending.current) return;
    pending.current = true;
    lastCheck.current = Date.now();
    setUpdateChecking(true);
    if (!silent) setUpdateStatusText(t("toast.checkingGitHub"));
    try {
      const info = await checkAppUpdate();
      setLatestAvailableVersion(info.available ? info.latestVersion ?? null : null);
      setLatestRelease(info.available
        ? { notes: info.notes ?? null, url: info.releaseUrl ?? null }
        : null);
      const message = !info.latestVersion ? t("toast.noPublishedReleases")
        : info.available ? t("toast.newVersionFound", { version: info.latestVersion })
        : t("toast.alreadyLatest");
      setUpdateStatusText(message);
      if (!silent) {
        if (!info.latestVersion) toast.info(message);
        else if (!info.available) toast.success(message);
        else toast.info(t("toast.newVersionToast", { version: info.latestVersion }));
      }
    } catch (error) {
      // Background failures must not interrupt dictation or erase a known update.
      if (!silent) {
        const message = error instanceof Error ? error.message : t("toast.checkUpdateFailed");
        setUpdateStatusText(message);
        toast.error(message);
      }
    } finally {
      pending.current = false;
      setUpdateChecking(false);
    }
  }, [t]);

  useEffect(() => {
    if (!automatic) return;
    const checkIfDue = () => {
      if (Date.now() - lastCheck.current >= CHECK_INTERVAL) void handleCheckForUpdates(true);
    };
    checkIfDue();
    const timer = window.setInterval(checkIfDue, CHECK_INTERVAL);
    window.addEventListener("focus", checkIfDue);
    window.addEventListener("online", checkIfDue);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", checkIfDue);
      window.removeEventListener("online", checkIfDue);
    };
  }, [automatic, handleCheckForUpdates]);

  const handleInstallUpdate = useCallback(async () => {
    if (pending.current || !latestAvailableVersion) return;
    pending.current = true;
    setUpdateChecking(true);
    setUpdateInstalling(true);
    setUpdateProgress({ phase: "downloading", downloaded: 0, total: null });
    setUpdateStatusText(t("settings.downloadingUpdate"));
    try {
      await installAppUpdate(latestAvailableVersion, (progress) => {
        setUpdateProgress(progress);
        setUpdateStatusText(progress.phase === "installing" ? t("settings.installingUpdate")
          : progress.total ? t("settings.updateDownloadProgress", {
            percent: Math.min(100, Math.floor(progress.downloaded * 100 / progress.total)),
          }) : t("settings.downloadingUpdate"));
      });
      setLatestAvailableVersion(null);
      setLatestRelease(null);
      setUpdateStatusText(t("settings.installingUpdate"));
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toast.installUpdateFailed");
      setUpdateStatusText(message);
      toast.error(message);
    } finally {
      pending.current = false;
      setUpdateChecking(false);
      setUpdateInstalling(false);
      setUpdateProgress(null);
    }
  }, [latestAvailableVersion, t]);

  return { updateChecking, updateInstalling, updateStatusText, latestAvailableVersion,
    latestRelease, updateProgress, handleCheckForUpdates, handleInstallUpdate };
}

const AppUpdateContext = createContext<ReturnType<typeof useUpdateController> | null>(null);

export function AppUpdateProvider({ children, automatic = true }: { children: ReactNode; automatic?: boolean }) {
  const updates = useUpdateController(automatic);
  return <AppUpdateContext.Provider value={updates}>{children}</AppUpdateContext.Provider>;
}

export function useAppUpdates() {
  const updates = useContext(AppUpdateContext);
  if (!updates) throw new Error("AppUpdateProvider is required");
  return updates;
}
