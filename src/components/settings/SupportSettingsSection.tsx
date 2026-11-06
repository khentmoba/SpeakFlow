import { useState } from "react";
import { ClipboardCopy, FolderOpen, LifeBuoy, LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { copyToClipboard, getDiagnostics, openLogsFolder } from "@/api/tauri";

/**
 * Support actions that stay on this PC: a redacted diagnostics report for an
 * issue, and the log folder for manual inspection. No telemetry is sent.
 */
export default function SupportSettingsSection() {
  const { t } = useTranslation();
  const [busy, setBusy] = useState<"diagnostics" | "logs" | null>(null);
  const [status, setStatus] = useState("");

  const handleCopyDiagnostics = async () => {
    setBusy("diagnostics");
    setStatus("");
    try {
      const report = await getDiagnostics();
      await copyToClipboard(report);
      setStatus(t("settings.diagnosticsCopied"));
      toast.success(t("settings.diagnosticsCopied"));
    } catch {
      setStatus(t("settings.diagnosticsFailed"));
      toast.error(t("settings.diagnosticsFailed"));
    } finally {
      setBusy(null);
    }
  };

  const handleOpenLogs = async () => {
    setBusy("logs");
    setStatus("");
    try {
      const folder = await openLogsFolder();
      setStatus(t("settings.logsOpened", { path: folder }));
    } catch {
      setStatus(t("settings.logsOpenFailed"));
      toast.error(t("settings.logsOpenFailed"));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="settings-card" data-nav-id="support" aria-labelledby="support-title">
      <div className="settings-section-header">
        <LifeBuoy size={15} className="icon-accent" aria-hidden="true" />
        <h2 id="support-title" className="settings-section-title">{t("settings.support")}</h2>
      </div>
      <p className="settings-hint">{t("settings.supportDesc")}</p>
      <div className="settings-data-actions">
        <button
          type="button"
          className="btn-ghost settings-data-action"
          onClick={() => void handleCopyDiagnostics()}
          disabled={busy !== null}
          aria-busy={busy === "diagnostics"}
        >
          {busy === "diagnostics"
            ? <LoaderCircle size={13} className="animate-spin" aria-hidden="true" />
            : <ClipboardCopy size={13} aria-hidden="true" />}
          {t("settings.copyDiagnostics")}
        </button>
        <button
          type="button"
          className="btn-ghost settings-data-action"
          onClick={() => void handleOpenLogs()}
          disabled={busy !== null}
          aria-busy={busy === "logs"}
        >
          {busy === "logs"
            ? <LoaderCircle size={13} className="animate-spin" aria-hidden="true" />
            : <FolderOpen size={13} aria-hidden="true" />}
          {t("settings.openLogsFolder")}
        </button>
      </div>
      <p className="settings-hint" aria-live="polite">{status}</p>
    </section>
  );
}
