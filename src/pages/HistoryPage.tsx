import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Clock3,
  Copy,
  Download,
  FileAudio,
  PencilLine,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import {
  copyToClipboard,
  deleteTranscriptionHistory,
  exportTranscriptionHistory,
  getTranscriptionHistoryStats,
  listTranscriptionHistory,
  reprocessTranscriptionHistory,
  transcribeAudioFile,
} from "@/api/tauri";
import { prefersReducedMotion } from "@/lib/motion";
import FixThisDialog from "@/components/FixThisDialog";
import { LABS_OUTPUT_FIXES_KEY } from "@/lib/constants";
import { readLocalStorage } from "@/lib/storage";
import type {
  FileTranscriptionResult,
  PersistentHistoryRecord,
  PersistentHistoryStats,
  RecordingMode,
} from "@/types";

type View = "main" | "settings" | "history";
type StatusFilter = "" | "success" | "failed";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatLatency(ms?: number | null): string {
  if (ms == null) return "—";
  return ms >= 1000 ? `${(ms / 1000).toFixed(ms >= 10_000 ? 1 : 2)}s` : `${ms}ms`;
}

const dateTimeFormatCache = new Map<string, Intl.DateTimeFormat>();

function getDateFormat(language: string): Intl.DateTimeFormat {
  let formatter = dateTimeFormatCache.get(language);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat(language, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
    dateTimeFormatCache.set(language, formatter);
  }
  return formatter;
}

function recordDate(value: number, language: string): string {
  return getDateFormat(language).format(new Date(value));
}

export default function HistoryPage({
  animClass = "",
}: {
  onNavigate: (view: View) => void;
  animClass?: string;
}) {
  const { t, i18n } = useTranslation();
  const [items, setItems] = useState<PersistentHistoryRecord[]>([]);
  const [stats, setStats] = useState<PersistentHistoryStats | null>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<"" | RecordingMode>("");
  const [status, setStatus] = useState<StatusFilter>("");
  const [total, setTotal] = useState(0);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const [workingId, setWorkingId] = useState<number | null>(null);
  const [removingId, setRemovingId] = useState<number | null>(null);
  const deletionInProgress = useRef(false);
  const [exportFormat, setExportFormat] = useState<"json" | "markdown" | "csv" | "srt" | "vtt">("markdown");
  const [fileResult, setFileResult] = useState<FileTranscriptionResult | null>(null);
  const [fileBusy, setFileBusy] = useState(false);
  const [fixRecord, setFixRecord] = useState<PersistentHistoryRecord | null>(null);
  // Labs > "Fix this" is mirrored through localStorage so this page does not
  // need an extra profile round trip just to show one button.
  const outputFixesEnabled = useMemo(
    () => readLocalStorage(LABS_OUTPUT_FIXES_KEY) === "true",
    [],
  );
  const requestId = useRef(0);
  const itemCount = useRef(0);
  const pageSize = 50;

  useEffect(() => {
    itemCount.current = items.length;
  }, [items.length]);

  const refreshStats = useCallback(async () => {
    try {
      setStats(await getTranscriptionHistoryStats());
    } catch (error) {
      console.error("Failed to load history stats:", error);
    }
  }, []);

  const load = useCallback(async (reset: boolean) => {
    const currentRequest = ++requestId.current;
    const offset = reset ? 0 : itemCount.current;
    if (reset) setLoading(true);
    setLoadError("");
    try {
      const page = await listTranscriptionHistory({
        query,
        mode,
        status,
        limit: pageSize,
        offset,
      });
      if (currentRequest !== requestId.current) return;
      setItems((previous) => reset ? page.items : [...previous, ...page.items]);
      setTotal(page.total);
      setHasMore(page.hasMore);
    } catch (error) {
      if (currentRequest !== requestId.current) return;
      setLoadError(errorMessage(error));
    } finally {
      if (currentRequest === requestId.current) setLoading(false);
    }
  }, [mode, query, status]);
  const loadRef = useRef(load);

  useEffect(() => {
    loadRef.current = load;
  }, [load]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load(true);
    }, query ? 180 : 0);
    return () => window.clearTimeout(timer);
  }, [load, query, mode, status]);

  useEffect(() => {
    void refreshStats();
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen("history-updated", () => {
      if (deletionInProgress.current) return;
      void loadRef.current(true);
      void refreshStats();
    }).then((dispose) => {
      if (disposed) {
        dispose();
      } else {
        unlisten = dispose;
      }
    }).catch((error) => {
      console.error("Failed to listen for history updates:", error);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshStats]);

  const hasActiveFilter = Boolean(query || mode || status);
  const latencyCards = useMemo(() => stats ? [
    { label: t("historyPage.asrLatency"), value: stats.asr },
    { label: t("historyPage.totalLatency"), value: stats.totalLatency },
  ] : [], [stats, t]);

  // Saved time is an estimate, so it is rounded to the unit a person would say
  // out loud rather than shown to the second.
  const savedTime = useMemo(() => {
    const seconds = stats?.estimatedSecondsSaved ?? 0;
    if (seconds < 60) return `${seconds}s`;
    if (seconds < 3600) return `${Math.round(seconds / 60)} min`;
    return `${(seconds / 3600).toFixed(1)} h`;
  }, [stats?.estimatedSecondsSaved]);

  const copyRecord = async (record: PersistentHistoryRecord) => {
    const text = record.text || record.originalText;
    if (!text) return;
    try {
      await copyToClipboard(text);
      setCopiedId(record.id);
      window.setTimeout(() => setCopiedId((value) => value === record.id ? null : value), 1400);
    } catch {
      toast.error(t("common.copyFailed"));
    }
  };

  const reprocess = async (record: PersistentHistoryRecord, kind: "polish" | "asr") => {
    if (workingId != null) return;
    setWorkingId(record.id);
    try {
      await reprocessTranscriptionHistory(record.id, kind);
      toast.success(t("historyPage.reprocessDone"));
      await Promise.all([load(true), refreshStats()]);
    } catch (error) {
      toast.error(t("historyPage.reprocessFailed", { error: errorMessage(error) }));
    } finally {
      setWorkingId(null);
    }
  };

  const remove = async (record: PersistentHistoryRecord) => {
    if (workingId != null) return;
    if (!window.confirm(t("historyPage.deleteConfirm"))) return;
    setWorkingId(record.id);
    deletionInProgress.current = true;
    try {
      const removed = await deleteTranscriptionHistory(record.id);
      if (!removed) {
        await Promise.all([load(true), refreshStats()]);
        return;
      }
      if (!prefersReducedMotion()) {
        setRemovingId(record.id);
        await new Promise<void>((resolve) => window.setTimeout(resolve, 180));
      }
      setItems((previous) => previous.filter((item) => item.id !== record.id));
      setRemovingId(null);
      setTotal((value) => Math.max(0, value - 1));
      await Promise.all([load(true), refreshStats()]);
    } catch (error) {
      toast.error(t("historyPage.deleteFailed", { error: errorMessage(error) }));
    } finally {
      deletionInProgress.current = false;
      setWorkingId(null);
    }
  };

  const exportHistory = async () => {
    try {
      const path = await exportTranscriptionHistory(exportFormat);
      if (path) toast.success(t("historyPage.exportDone"));
    } catch (error) {
      toast.error(t("historyPage.exportFailed", { error: errorMessage(error) }));
    }
  };

  const transcribeFile = async () => {
    setFileBusy(true);
    try {
      const result = await transcribeAudioFile();
      if (result) {
        setFileResult(result);
        toast.success(t("historyPage.fileDone", { name: result.fileName }));
        void load(true);
      }
    } catch (error) {
      toast.error(t("historyPage.fileFailed", { error: errorMessage(error) }));
    } finally {
      setFileBusy(false);
    }
  };

  return (
    <div className="page-root">
      <main className={`page-content history-page ${animClass}`.trim()}>
        <section className="history-command-bar" aria-label={t("historyPage.title")}>
          <label className="history-search">
            <Search size={14} aria-hidden="true" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("historyPage.searchPlaceholder")}
              aria-label={t("historyPage.searchPlaceholder")}
            />
          </label>
          <div className="history-export-control">
            <select value={exportFormat} onChange={(event) => setExportFormat(event.target.value as "json" | "markdown" | "csv" | "srt" | "vtt")}>
              <option value="markdown">{t("historyPage.exportMarkdown")}</option>
              <option value="json">{t("historyPage.exportJson")}</option>
              <option value="csv">{t("historyPage.exportCsv")}</option>
              <option value="srt">{t("historyPage.exportSrt")}</option>
              <option value="vtt">{t("historyPage.exportVtt")}</option>
            </select>
            <button className="icon-btn" aria-label={t("historyPage.export")} onClick={() => void exportHistory()}>
              <Download size={14} />
            </button>
          </div>
          <button
            className="btn-ghost"
            disabled={fileBusy}
            onClick={() => void transcribeFile()}
            aria-label={t("historyPage.transcribeFile")}
          >
            <FileAudio size={14} />
            {fileBusy ? t("historyPage.fileBusy") : t("historyPage.transcribeFile")}
          </button>
        </section>

        {fileResult && (
          <section className="history-metrics" aria-label={t("historyPage.fileResultTitle")}>
            <div className="history-metric history-metric-primary">
              <strong>{fileResult.fileName}</strong>
              <span>{t("historyPage.fileResultTitle")}</span>
              <small>
                {t("historyPage.fileDuration", {
                  seconds: Math.round(fileResult.durationSec),
                })}
                {fileResult.language ? ` · ${fileResult.language}` : ""}
              </small>
            </div>
            <div className="settings-column" style={{ flex: 1, minWidth: 0 }}>
              <p className="settings-hint settings-hint-flush" style={{ whiteSpace: "pre-wrap" }}>
                {fileResult.text}
              </p>
              <div style={{ display: "flex", gap: 6 }}>
                <button
                  className="btn-ghost"
                  onClick={() => {
                    void copyToClipboard(fileResult.text)
                      .then(() => toast.success(t("common.copied")))
                      .catch(() => toast.error(t("common.copyFailed")));
                  }}
                >
                  <Copy size={13} />
                  {t("historyPage.copy")}
                </button>
                <button className="btn-ghost" onClick={() => setFileResult(null)}>
                  {t("historyPage.fileDismiss")}
                </button>
              </div>
            </div>
          </section>
        )}

        <section className="history-filter-row">
          <div className="history-filter-group" role="group" aria-label={t("historyPage.allModes")}>
            {(["", "dictation", "assistant"] as const).map((value) => (
              <button key={value || "all"} className={mode === value ? "active" : ""} onClick={() => setMode(value)}>
                {value === "" ? t("historyPage.allModes") : t(`historyPage.${value}`)}
              </button>
            ))}
          </div>
          <select className="history-status-filter" value={status} onChange={(event) => setStatus(event.target.value as StatusFilter)}>
            <option value="">{t("historyPage.allStatuses")}</option>
            <option value="success">{t("historyPage.success")}</option>
            <option value="failed">{t("historyPage.failed")}</option>
          </select>
        </section>

        {stats && stats.total > 0 && (
          <section className="history-metrics" aria-label={t("historyPage.total", { count: stats.total })}>
            <div className="history-metric history-metric-primary">
              <strong>{stats.totalWords.toLocaleString()}</strong>
              <span>{t("historyPage.words", { count: stats.totalWords.toLocaleString() })}</span>
              <small>{t("historyPage.characters", { count: stats.totalCharacters.toLocaleString() })}</small>
            </div>
            <div className="history-metric">
              <strong>{savedTime}</strong>
              <span>{t("historyPage.timeSaved")}</span>
              <small>{t("historyPage.total", { count: stats.total.toLocaleString() })}</small>
            </div>
            <div className="history-metric">
              <strong>{stats.activeDays.toLocaleString()}</strong>
              <span>{t("historyPage.activeDays")}</span>
              <small>{t("historyPage.streak", { count: stats.currentStreakDays })}</small>
            </div>
            {latencyCards.map(({ label, value }) => (
              <div className="history-metric" key={label}>
                <span>{label}</span>
                <strong>{formatLatency(value.p50Ms)}</strong>
                <small>P95 {formatLatency(value.p95Ms)}</small>
              </div>
            ))}
          </section>
        )}

        <section className="history-scroll-region">
          {!loading && loadError && (
            <div className="history-empty history-empty-error">
              <p>{t("historyPage.loadFailed", { error: loadError })}</p>
              <button className="btn-ghost" onClick={() => void load(true)}><RefreshCw size={13} />{t("common.retry")}</button>
            </div>
          )}

          {!loading && !loadError && items.length === 0 && (
            <div className="history-empty">
              <Clock3 size={24} />
              <strong>{t(hasActiveFilter ? "historyPage.noMatchTitle" : "historyPage.emptyTitle")}</strong>
              <p>{t(hasActiveFilter ? "historyPage.noMatchHint" : "historyPage.emptyHint")}</p>
            </div>
          )}

          {loading && items.length === 0 && (
            <div className="history-loading" aria-label={t("common.loading")}>
              <span /><span /><span />
            </div>
          )}

          <div className="persistent-history-list">
            {items.map((record) => {
              const failed = record.status !== "success";
              const hasDistinctRaw = Boolean(record.originalText && record.originalText !== record.text);
              const hasSourceText = Boolean(record.sourceText?.trim());
              const hasDetails = hasDistinctRaw || hasSourceText;
              const expanded = expandedId === record.id;
              const isWorking = workingId === record.id;
              const canReprocess = record.workflow === "dictation";
              return (
                <div className="history-record-motion" data-removing={removingId === record.id} key={record.id}>
                <div className="history-record-clip">
                <article className={`persistent-history-card${failed ? " history-card-failed" : ""}`}>
                  <header className="history-card-header">
                    <div className="history-card-context">
                      <span className={`history-status-dot${failed ? " failed" : ""}`} />
                      <strong>{record.appProcess || t(`historyPage.${record.mode}`)}</strong>
                      <time>{recordDate(record.createdAt, i18n.language)}</time>
                    </div>
                    <div className="history-card-flags">
                      {record.reprocessedFromId != null && <span>{t("historyPage.reprocessed")}</span>}
                      {hasDistinctRaw && (
                        <span className="history-flag-corrected" title={t("historyPage.correctedBadgeTitle")}>
                          {t("historyPage.correctedBadge")}
                        </span>
                      )}
                      {record.audioAvailable && <span title={t("historyPage.audioSaved")}><FileAudio size={11} /></span>}
                    </div>
                  </header>

                  <div className="history-card-copy">
                    {failed ? (
                      <p className="history-card-error">{record.error || record.status}</p>
                    ) : (
                      <p>{record.text}</p>
                    )}
                    {record.appRuleName && <small>{t("historyPage.appRule", { name: record.appRuleName })}</small>}
                  </div>

                  {hasDetails && (
                    <div className="history-details-motion" data-expanded={expanded} inert={!expanded} aria-hidden={!expanded}>
                    <div className="history-record-clip">
                    <div className="history-raw-panel">
                      {hasDistinctRaw && (
                        <>
                          <span>{t("historyPage.rawText")}</span>
                          <p>{record.originalText}</p>
                        </>
                      )}
                      {hasSourceText && (
                        <>
                          <span>{t("historyPage.sourceText")}</span>
                          <p>{record.sourceText}</p>
                        </>
                      )}
                    </div>
                    </div>
                    </div>
                  )}

                  <div className="history-latency-strip" aria-label={t("historyPage.totalLatency")}>
                    <span><b>{t("historyPage.asrLatency")}</b>{formatLatency(record.asrMs)}</span>
                    <span><b>{t("historyPage.totalLatency")}</b>{formatLatency(record.totalMs)}</span>
                    {record.engine && <span className="history-engine-label">{record.engine}</span>}
                  </div>

                  <footer className="history-card-actions">
                    {hasDetails && (
                      <button className="history-text-action" aria-expanded={expanded} onClick={() => setExpandedId(expanded ? null : record.id)}>
                        {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
                        {t(expanded ? "historyPage.hideDetails" : "historyPage.showDetails")}
                      </button>
                    )}
                    <span className="history-action-spacer" />
                    {outputFixesEnabled && !failed && (
                      <button className="icon-btn icon-btn-sm" title={t("settings.fixThis")} aria-label={t("settings.fixThis")} onClick={() => setFixRecord(record)}>
                        <PencilLine size={13} />
                      </button>
                    )}
                    {canReprocess && record.audioAvailable && (
                      <button className="icon-btn icon-btn-sm" disabled={isWorking} title={t("historyPage.reprocessAsr")} aria-label={t("historyPage.reprocessAsr")} onClick={() => void reprocess(record, "asr")}>
                        <RefreshCw size={13} />
                      </button>
                    )}
                    <button className="icon-btn icon-btn-sm" title={t("common.copy")} aria-label={t("common.copy")} onClick={() => void copyRecord(record)}>
                      {copiedId === record.id ? <Check size={13} /> : <Copy size={13} />}
                    </button>
                    <button className="icon-btn icon-btn-sm history-delete-action" disabled={isWorking} title={t("historyPage.delete")} aria-label={t("historyPage.delete")} onClick={() => void remove(record)}>
                      <Trash2 size={13} />
                    </button>
                  </footer>
                </article>
                </div>
                </div>
              );
            })}
          </div>

          {hasMore && !loading && (
            <button className="history-load-more" onClick={() => void load(false)}>
              {t("historyPage.loadMore")} · {items.length}/{total}
            </button>
          )}
        </section>
      </main>
      {fixRecord && (
        <FixThisDialog
          record={fixRecord}
          onClose={() => setFixRecord(null)}
          onSaved={() => { void load(true); }}
        />
      )}
    </div>
  );
}
