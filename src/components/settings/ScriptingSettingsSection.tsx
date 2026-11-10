import { useCallback, useEffect, useState } from "react";
import { FileCode2, LoaderCircle, Play, Save } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import {
  getScriptingStatus,
  readScriptConfig,
  saveScriptConfig,
  scaffoldScriptConfig,
  testScriptTransformations,
} from "@/api/tauri";
import type { ScriptingStatus, UserProfile } from "@/types";
import { SettingsSectionHeader } from "./SettingsControls";

interface ScriptingSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

export default function ScriptingSettingsSection({ profile, onSaved }: ScriptingSettingsSectionProps) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ScriptingStatus | null>(null);
  const [source, setSource] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [sample, setSample] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  const globalTransformations = profile?.global_transformations ?? [];

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const next = await getScriptingStatus();
      setStatus(next);
      const current = await readScriptConfig();
      setSource(current ?? "");
    } catch {
      toast.error(t("settings.scriptingSaveFailed"));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const handleScaffold = async () => {
    setSaving(true);
    try {
      const next = await scaffoldScriptConfig();
      setStatus(next);
      const current = await readScriptConfig();
      setSource(current ?? "");
      onSaved();
    } catch {
      toast.error(t("settings.scriptingSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      const next = await saveScriptConfig(source);
      setStatus(next);
      if (next.valid) {
        toast.success(t("settings.scriptingSaved"));
        onSaved();
      } else {
        toast.error(t("settings.scriptingInvalid"));
      }
    } catch {
      toast.error(t("settings.scriptingSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const handleTest = async () => {
    if (!sample.trim()) {
      toast.error(t("settings.scriptingTestEmpty"));
      return;
    }
    setTesting(true);
    setTestResult(null);
    try {
      const ids = globalTransformations.length > 0
        ? globalTransformations
        : (status?.allTransforms ?? []).map((transform) => transform.id);
      setTestResult(await testScriptTransformations({ source, transformIds: ids, text: sample }));
    } catch {
      toast.error(t("settings.scriptingTestFailed"));
    } finally {
      setTesting(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="scripting">
      <SettingsSectionHeader
        icon={<FileCode2 size={15} className="icon-accent" />}
        title={t("settings.scripting")}
        trailing={(
          <span className="settings-option-desc">
            {status?.valid
              ? t("settings.scriptingValid")
              : status?.configExists
                ? t("settings.scriptingInvalid")
                : t("settings.scriptingMissing")}
          </span>
        )}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.scriptingHint")}</p>

        {status && (
          <div className="settings-row">
            <span className="settings-option-desc">{t("settings.scriptingPath")}</span>
            <code className="settings-option-desc" title={status.configPath}>{status.configPath}</code>
          </div>
        )}

        {status?.error && <p className="settings-error">{status.error}</p>}

        {loading ? (
          <p className="settings-hint"><LoaderCircle size={12} className="animate-spin" aria-hidden="true" /> {t("common.loading")}</p>
        ) : (
          <>
            <textarea
              className="settings-input settings-code-block"
              aria-label={t("settings.scripting")}
              spellCheck={false}
              value={source}
              onChange={(event) => setSource(event.target.value)}
            />

            <div className="settings-row">
              <div className="settings-header-actions">
                {!status?.configExists && (
                  <button type="button" className="btn-ghost btn-ghost-sm" disabled={saving} onClick={() => { void handleScaffold(); }}>
                    {t("settings.scriptingScaffold")}
                  </button>
                )}
                <button type="button" className="btn-ghost btn-ghost-sm" disabled={saving} onClick={() => { void reload(); }}>
                  {t("settings.scriptingReload")}
                </button>
                <button type="button" className="test-btn" disabled={saving} onClick={() => { void handleSave(); }}>
                  <Save size={12} aria-hidden="true" /> {t("settings.scriptingSave")}
                </button>
              </div>
            </div>

            {status && status.allTransforms.length > 0 && (
              <div className="settings-column">
                <span className="settings-option-label">{t("settings.scriptingTransforms")}</span>
                <div className="settings-column">
                  {status.allTransforms.map((transform) => (
                    <div className="settings-row" key={transform.id}>
                      <span className="settings-option-desc">{transform.label}</span>
                      <span className="settings-option-desc">
                        {transform.kind === "script" ? t("settings.scriptingScript") : t("settings.scriptingBuiltin")}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div className="settings-column">
              <span className="settings-option-label">{t("settings.scriptingTest")}</span>
              <input
                className="settings-input"
                value={sample}
                placeholder={t("settings.scriptingTestPlaceholder")}
                aria-label={t("settings.scriptingTestPlaceholder")}
                onChange={(event) => setSample(event.target.value)}
              />
              <div className="settings-row">
                <button type="button" className="test-btn" disabled={testing} onClick={() => { void handleTest(); }}>
                  <Play size={12} aria-hidden="true" /> {t("settings.scriptingTestRun")}
                </button>
              </div>
              {testResult !== null && <p className="settings-hint">{testResult}</p>}
            </div>
          </>
        )}
      </div>
    </section>
  );
}
