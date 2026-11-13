import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronUp,
  Clock3,
  Copy,
  Download,
  LoaderCircle,
  Mic,
  RefreshCw,
  Square,
  Trash2,
  Users,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import {
  copyToClipboard,
  deleteMeeting,
  exportMeeting,
  getMeetingStatus,
  listMeetings,
  reprocessMeeting,
  startMeetingRecording,
  stopMeetingRecording,
} from "@/api/tauri";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import { formatMeetingDuration } from "@/lib/behaviorConfig";
import type {
  MeetingExportFormat,
  MeetingRecord,
  MeetingStatusPayload,
} from "@/types";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const EXPORT_FORMATS: readonly { value: MeetingExportFormat; labelKey: string }[] = [
  { value: "md", labelKey: "settings.meetingsExportMd" },
  { value: "srt", labelKey: "settings.meetingsExportSrt" },
  { value: "vtt", labelKey: "settings.meetingsExportVtt" },
  { value: "json", labelKey: "settings.meetingsExportJson" },
];

export default function MeetingsPage({ animClass = "" }: { animClass?: string }) {
  const { t } = useTranslation();
  const [meetings, setMeetings] = useState<MeetingRecord[]>([]);
  const [status, setStatus] = useState<MeetingStatusPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [exportFormat, setExportFormat] = useState<MeetingExportFormat>("md");
  const mountedRef = useRef(true);

  const load = useCallback(async () => {
    try {
      const next = await listMeetings();
      if (mountedRef.current) setMeetings(next);
    } catch {
      toast.error(t("settings.meetingsLoadFailed"));
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, [t]);

  const refreshStatus = useCallback(async () => {
    try {
      const next = await getMeetingStatus();
      if (mountedRef.current) setStatus(next);
    } catch {
      // Status polling is best effort; the list still works.
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void load();
    void refreshStatus();
    return () => { mountedRef.current = false; };
  }, [load, refreshStatus]);

  // Poll while a meeting is active so the timer and sample counts stay live.
  useEffect(() => {
    if (!status?.recording && !status?.processing) return;
    const timer = window.setInterval(() => { void refreshStatus(); }, 2000);
    return () => window.clearInterval(timer);
  }, [status?.recording, status?.processing, refreshStatus]);

  useTauriEvent("meeting-finished", () => {
    void load();
    void refreshStatus();
  });
  useTauriEvent("meeting-status", () => { void refreshStatus(); });

  const toggleRecording = async () => {
    setBusy(true);
    try {
      if (status?.recording) {
        await stopMeetingRecording();
        await load();
      } else {
        await startMeetingRecording();
      }
      await refreshStatus();
    } catch (error) {
      toast.error(errorMessage(error));
    } finally {
      setBusy(false);
    }
  };

  const handleExport = async (id: string) => {
    try {
      const content = await exportMeeting(id, exportFormat, false);
      await copyToClipboard(content);
      toast.success(t("common.copiedToClipboard"));
    } catch {
      toast.error(t("settings.meetingsExportFailed"));
    }
  };

  const handleSaveExport = async (id: string) => {
    try {
      const path = await exportMeeting(id, exportFormat, true);
      if (path) toast.success(path);
    } catch {
      toast.error(t("settings.meetingsExportFailed"));
    }
  };

  const handleDelete = async (id: string) => {
    setBusy(true);
    try {
      await deleteMeeting(id);
      await load();
    } catch {
      toast.error(t("settings.meetingsDeleteFailed"));
    } finally {
      setBusy(false);
    }
  };

  const handleReprocess = async (id: string) => {
    setBusy(true);
    try {
      await reprocessMeeting(id);
      await load();
    } catch {
      toast.error(t("settings.meetingsReprocessFailed"));
    } finally {
      setBusy(false);
    }
  };

  const statusLabel = useMemo(() => {
    if (status?.recording) return t("settings.meetingsRecording");
    if (status?.processing) return t("settings.meetingsProcessing");
    return null;
  }, [status?.processing, status?.recording, t]);

  return (
    <div className="page-root">
      <main className={`page-content meetings-page ${animClass}`.trim()}>
        <section className="history-command-bar" aria-label={t("settings.meetings")}>
          <button
            type="button"
            className={status?.recording ? "btn-ghost" : "test-btn"}
            disabled={busy || status?.processing}
            onClick={() => { void toggleRecording(); }}
          >
            {status?.recording
              ? <><Square size={13} aria-hidden="true" /> {t("settings.meetingsStop")}</>
              : <><Mic size={13} aria-hidden="true" /> {t("settings.meetingsStart")}</>}
          </button>

          {statusLabel && (
            <span className="settings-option-desc">
              {statusLabel}
              {status?.recording && ` · ${formatMeetingDuration(status.durationSec)}`}
            </span>
          )}

          <button
            type="button"
            className="icon-btn"
            aria-label={t("common.refresh")}
            disabled={loading}
            onClick={() => { void load(); void refreshStatus(); }}
          >
            <RefreshCw size={14} />
          </button>
        </section>

        {loading ? (
          <p className="settings-hint" role="status">
            <LoaderCircle size={12} className="animate-spin" aria-hidden="true" /> {t("common.loading")}
          </p>
        ) : meetings.length === 0 ? (
          <div className="app-rules-empty">{t("settings.meetingsEmpty")}</div>
        ) : (
          meetings.map((meeting) => {
            const expanded = expandedId === meeting.id;
            return (
              <section className="settings-card" key={meeting.id}>
                <div className="settings-section-header">
                  <Users size={15} className="icon-accent" />
                  <h2 className="settings-section-title">{meeting.title}</h2>
                  <div className="settings-header-actions">
                    <span className="settings-option-desc">
                      <Clock3 size={11} aria-hidden="true" /> {formatMeetingDuration(meeting.durationSec)}
                    </span>
                    <button
                      type="button"
                      className="btn-ghost btn-ghost-sm"
                      aria-expanded={expanded}
                      onClick={() => setExpandedId(expanded ? null : meeting.id)}
                    >
                      {expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
                      {t("settings.meetingsTranscript")}
                    </button>
                  </div>
                </div>

                <div className="settings-row">
                  <span className="settings-option-desc">
                    {t("settings.meetingsWords", { count: meeting.wordCount })}
                    {" · "}
                    {t("settings.meetingsSpeakers", { count: meeting.speakerCount })}
                    {meeting.appName ? ` · ${meeting.appName}` : ""}
                  </span>
                  <div className="settings-header-actions">
                    <select
                      className="settings-input app-rule-select"
                      aria-label={t("settings.meetingsExport")}
                      value={exportFormat}
                      onChange={(event) => setExportFormat(event.target.value as MeetingExportFormat)}
                    >
                      {EXPORT_FORMATS.map(({ value, labelKey }) => (
                        <option key={value} value={value}>{t(labelKey)}</option>
                      ))}
                    </select>
                    <button type="button" className="icon-btn" aria-label={t("common.copy")} onClick={() => { void handleExport(meeting.id); }}>
                      <Copy size={13} />
                    </button>
                    <button type="button" className="icon-btn" aria-label={t("settings.meetingsExport")} onClick={() => { void handleSaveExport(meeting.id); }}>
                      <Download size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-ghost btn-ghost-sm"
                      disabled={busy}
                      onClick={() => { void handleReprocess(meeting.id); }}
                    >
                      {t("settings.meetingsReprocess")}
                    </button>
                    <button
                      type="button"
                      className="btn-ghost app-rule-icon-button app-rule-delete"
                      aria-label={t("settings.meetingsDelete")}
                      disabled={busy}
                      onClick={() => { void handleDelete(meeting.id); }}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>

                {meeting.status === "failed" && (
                  <p className="settings-error">
                    {t("settings.meetingsFailed", { error: meeting.error ?? "" })}
                  </p>
                )}

                {meeting.summary && (
                  <div className="settings-column">
                    <span className="settings-option-label">{t("settings.meetingsSummary")}</span>
                    <p className="settings-hint settings-hint-flush" style={{ whiteSpace: "pre-wrap" }}>
                      {meeting.summary}
                    </p>
                  </div>
                )}

                {expanded && (
                  <div className="settings-column">
                    {meeting.segments.length === 0 ? (
                      <p className="settings-hint">{t("settings.meetingsNoTranscript")}</p>
                    ) : (
                      meeting.segments.map((segment) => (
                        <div className="settings-row" key={segment.id}>
                          <div className="settings-toggle-copy">
                            <span className="settings-option-label">
                              {segment.speaker}
                              <span className="settings-option-desc">
                                {" · "}{formatMeetingDuration(segment.startMs / 1000)}
                              </span>
                            </span>
                            <span className="settings-option-desc">{segment.text}</span>
                          </div>
                        </div>
                      ))
                    )}
                  </div>
                )}
              </section>
            );
          })
        )}
      </main>
    </div>
  );
}
