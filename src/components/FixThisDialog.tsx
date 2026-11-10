import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { submitOutputFix } from "@/api/tauri";
import type { PersistentHistoryRecord } from "@/types";

interface FixThisDialogProps {
  record: PersistentHistoryRecord;
  onClose: () => void;
  onSaved: () => void;
}

/**
 * "Fix this" keeps a corrected transcript as a learning example.
 * The original stays visible so the diff is obvious while editing.
 */
export default function FixThisDialog({ record, onClose, onSaved }: FixThisDialogProps) {
  const { t } = useTranslation();
  const [fixed, setFixed] = useState(record.text);
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const [closing, setClosing] = useState(false);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    textareaRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setClosing(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    if (!closing) return;
    const timer = window.setTimeout(onClose, 140);
    return () => window.clearTimeout(timer);
  }, [closing, onClose]);

  const close = () => setClosing(true);

  const submit = async () => {
    const trimmed = fixed.trim();
    if (!trimmed || trimmed === record.text.trim()) {
      toast.error(t("settings.fixThisEmpty"));
      return;
    }
    setSaving(true);
    try {
      await submitOutputFix({
        original: record.text,
        fixed: trimmed,
        rawOriginal: record.originalText || null,
        note: note.trim() || null,
        sessionId: record.sessionId,
        processName: record.appProcess ?? null,
        windowTitle: record.appWindowTitle ?? null,
      });
      toast.success(t("settings.fixThisSaved"));
      onSaved();
      close();
    } catch {
      toast.error(t("settings.fixThisFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="correction-modal" data-closing={closing}>
      <button
        type="button"
        className="modal-dismiss"
        tabIndex={-1}
        aria-label={t("common.close")}
        onClick={close}
      />
      <div
        ref={dialogRef}
        className="correction-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="fix-this-title"
        tabIndex={-1}
      >
        <div className="settings-section-header">
          <span id="fix-this-title" className="settings-section-title">{t("settings.fixThisTitle")}</span>
          <div className="settings-header-actions">
            <button className="icon-btn" type="button" onClick={close} aria-label={t("common.close")}>
              <X size={15} />
            </button>
          </div>
        </div>

        <div className="settings-column" style={{ padding: "0 16px 16px" }}>
          <span className="settings-option-label">{t("settings.fixThisOriginal")}</span>
          <p className="settings-hint settings-hint-flush" style={{ whiteSpace: "pre-wrap" }}>
            {record.text}
          </p>

          <label className="settings-column">
            <span className="settings-option-label">{t("settings.fixThisFixed")}</span>
            <textarea
              ref={textareaRef}
              className="settings-input settings-code-block"
              value={fixed}
              onChange={(event) => setFixed(event.target.value)}
            />
          </label>

          <label className="settings-column">
            <span className="settings-option-label">{t("settings.fixThisNote")}</span>
            <input
              className="settings-input"
              value={note}
              onChange={(event) => setNote(event.target.value)}
            />
          </label>

          <div className="settings-row">
            <span className="history-action-spacer" />
            <button type="button" className="btn-ghost btn-ghost-sm" onClick={close}>
              {t("common.cancel")}
            </button>
            <button type="button" className="test-btn" disabled={saving} onClick={() => { void submit(); }}>
              {t("settings.fixThisSubmit")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
