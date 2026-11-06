import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, Trash2, Wand2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { getDictationTools, setDictationTools } from "@/api/tauri";
import type { DictationSnippet, DictationToolsSettings, ToneMode } from "@/types";

const DEFAULT_SETTINGS: DictationToolsSettings = {
  snippets: [],
  toneMode: "off",
  tidyTranscript: true,
  voiceCommands: false,
  whisperMode: false,
  localOnly: false,
};

const TONE_OPTIONS: ToneMode[] = ["off", "formal", "casual", "concise", "bullets", "email"];

/** A snippet row that is still being typed is kept locally but not saved. */
function isComplete(snippet: DictationSnippet): boolean {
  return snippet.trigger.trim().length > 0 && snippet.expansion.trim().length > 0;
}

export default function DictationToolsSettingsSection() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<DictationToolsSettings>(DEFAULT_SETTINGS);
  const [draftSnippets, setDraftSnippets] = useState<DictationSnippet[]>([]);
  const [saving, setSaving] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // The load runs once. A ref keeps the latest translator without making the
  // effect re-run whenever the i18n instance hands back a new function.
  const translateRef = useRef(t);
  translateRef.current = t;

  useEffect(() => {
    let cancelled = false;
    getDictationTools()
      .then((stored) => {
        if (cancelled || !stored) return;
        setSettings(stored);
        setDraftSnippets(stored.snippets);
      })
      .catch(() => {
        if (!cancelled) toast.error(translateRef.current("settings.dictationToolsLoadFailed"));
      })
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = useCallback(
    async (next: DictationToolsSettings) => {
      const previous = settings;
      setSettings(next);
      setSaving(true);
      try {
        const stored = await setDictationTools(next);
        if (!stored) return;
        setSettings(stored);
        // Keep rows the user is still typing: they are not part of the save and
        // must not disappear when the stored list comes back.
        setDraftSnippets((current) => [
          ...stored.snippets,
          ...current.filter((row) => !isComplete(row)),
        ]);
      } catch {
        setSettings(previous);
        toast.error(t("settings.dictationToolsSaveFailed"));
      } finally {
        setSaving(false);
      }
    },
    [settings, t],
  );

  /** Saves only the rows that have both fields filled in. */
  const commitSnippets = (rows: DictationSnippet[]) => {
    const complete = rows.filter(isComplete);
    const unchanged =
      complete.length === settings.snippets.length &&
      complete.every(
        (row, index) =>
          row.trigger.trim() === settings.snippets[index].trigger &&
          row.expansion.trim() === settings.snippets[index].expansion,
      );
    // Tabbing out of an unfinished row must not rewrite the saved list.
    if (unchanged) return;
    void persist({ ...settings, snippets: complete });
  };

  const updateRow = (index: number, patch: Partial<DictationSnippet>) => {
    setDraftSnippets((rows) =>
      rows.map((row, position) => (position === index ? { ...row, ...patch } : row)),
    );
  };

  const toggle = (key: "tidyTranscript" | "voiceCommands" | "whisperMode" | "localOnly") => {
    void persist({ ...settings, [key]: !settings[key] });
  };

  return (
    <section className="settings-card" data-nav-id="dictation-tools">
      <div className="settings-section-header">
        <Wand2 size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.dictationTools")}</h2>
      </div>
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.dictationToolsHint")}</p>

        <label className="settings-column history-retention-field">
          <span className="permission-label">{t("settings.toneMode")}</span>
          <select
            className="settings-input"
            value={settings.toneMode}
            disabled={!loaded || saving}
            aria-label={t("settings.toneMode")}
            onChange={(event) => {
              void persist({ ...settings, toneMode: event.target.value as ToneMode });
            }}
          >
            {TONE_OPTIONS.map((mode) => (
              <option key={mode} value={mode}>
                {t(`settings.toneMode_${mode}`)}
              </option>
            ))}
          </select>
          <span className="settings-hint settings-hint-flush">{t("settings.toneModeHint")}</span>
        </label>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.tidyTranscript")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.tidyTranscriptHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.tidyTranscript}
            aria-label={t("settings.tidyTranscript")}
            className="toggle-switch"
            data-active={settings.tidyTranscript}
            disabled={!loaded || saving}
            onClick={() => toggle("tidyTranscript")}
          >
            <div className="toggle-knob" />
          </button>
        </div>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.voiceCommands")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.voiceCommandsHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.voiceCommands}
            aria-label={t("settings.voiceCommands")}
            className="toggle-switch"
            data-active={settings.voiceCommands}
            disabled={!loaded || saving}
            onClick={() => toggle("voiceCommands")}
          >
            <div className="toggle-knob" />
          </button>
        </div>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.whisperMode")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.whisperModeHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.whisperMode}
            aria-label={t("settings.whisperMode")}
            className="toggle-switch"
            data-active={settings.whisperMode}
            disabled={!loaded || saving}
            onClick={() => toggle("whisperMode")}
          >
            <div className="toggle-knob" />
          </button>
        </div>

        <div className="settings-column" style={{ gap: 8 }}>
          <span className="permission-label">{t("settings.snippets")}</span>
          <span className="settings-hint settings-hint-flush">{t("settings.snippetsHint")}</span>
          {draftSnippets.map((snippet, index) => (
            <div key={`snippet-${index}`} style={{ display: "flex", gap: 6 }}>
              <input
                type="text"
                className="settings-input"
                placeholder={t("settings.snippetTriggerPlaceholder")}
                aria-label={t("settings.snippetTrigger")}
                value={snippet.trigger}
                disabled={!loaded || saving}
                onChange={(event) => updateRow(index, { trigger: event.target.value })}
                onBlur={() => commitSnippets(draftSnippets)}
              />
              <input
                type="text"
                className="settings-input"
                placeholder={t("settings.snippetExpansionPlaceholder")}
                aria-label={t("settings.snippetExpansion")}
                value={snippet.expansion}
                disabled={!loaded || saving}
                onChange={(event) => updateRow(index, { expansion: event.target.value })}
                onBlur={() => commitSnippets(draftSnippets)}
              />
              <button
                type="button"
                className="btn-ghost"
                aria-label={t("settings.snippetRemove")}
                disabled={!loaded || saving}
                onClick={() => {
                  const rows = draftSnippets.filter((_, position) => position !== index);
                  setDraftSnippets(rows);
                  commitSnippets(rows);
                }}
              >
                <Trash2 size={14} />
              </button>
            </div>
          ))}
          <div>
            <button
              type="button"
              className="btn-ghost"
              disabled={!loaded || saving}
              onClick={() =>
                setDraftSnippets((rows) => [...rows, { trigger: "", expansion: "" }])
              }
            >
              <Plus size={14} />
              {t("settings.snippetAdd")}
            </button>
          </div>
        </div>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.localOnly")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.localOnlyHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.localOnly}
            aria-label={t("settings.localOnly")}
            className="toggle-switch"
            data-active={settings.localOnly}
            disabled={!loaded || saving}
            onClick={() => toggle("localOnly")}
          >
            <div className="toggle-knob" />
          </button>
        </div>
      </div>
    </section>
  );
}
