import { useEffect, useState } from "react";
import { AppWindow, ArrowDown, ArrowUp, Pencil, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setAppProfileRules } from "@/api/tauri";
import type {
  AppPasteMethod,
  AppProfileRule,
  AppRuleOverride,
  UserProfile,
} from "@/types";

interface AppProfileRulesSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

function createRule(): AppProfileRule {
  return {
    id: crypto.randomUUID(),
    name: "",
    enabled: true,
    process_name: "",
    window_title_contains: null,
    browser_domain: null,
    ai_polish: "inherit",
    translation: "inherit",
    translation_target: null,
    screen_context: "inherit",
    history: "inherit",
    custom_prompt: null,
    tone_mode: null,
    paste_method: "inherit",
    vocabulary: [],
    transformations: [],
  };
}

function PasteMethodSelect({ value, onChange }: {
  value: AppPasteMethod;
  onChange: (value: AppPasteMethod) => void;
}) {
  const { t } = useTranslation();
  return (
    <select
      className="settings-input app-rule-select"
      value={value}
      onChange={(event) => onChange(event.target.value as AppPasteMethod)}
    >
      <option value="inherit">{t("settings.appRuleInherit")}</option>
      <option value="send_input">{t("settings.appRulePasteSendInput")}</option>
      <option value="clipboard">{t("settings.appRulePasteClipboard")}</option>
      <option value="terminal_ctrl_shift_v">{t("settings.appRulePasteTerminal")}</option>
      <option value="copy_only">{t("settings.appRulePasteCopyOnly")}</option>
    </select>
  );
}

function OverrideSelect({ value, onChange }: {
  value: AppRuleOverride;
  onChange: (value: AppRuleOverride) => void;
}) {
  const { t } = useTranslation();
  return (
    <select
      className="settings-input app-rule-select"
      value={value}
      onChange={(event) => onChange(event.target.value as AppRuleOverride)}
    >
      <option value="inherit">{t("settings.appRuleInherit")}</option>
      <option value="enabled">{t("settings.appRuleEnabled")}</option>
      <option value="disabled">{t("settings.appRuleDisabled")}</option>
    </select>
  );
}

function RuleEditor({ value, onChange, onCancel, onSubmit }: {
  value: AppProfileRule;
  onChange: (rule: AppProfileRule) => void;
  onCancel: () => void;
  onSubmit: () => void;
}) {
  const { t } = useTranslation();
  const patch = (updates: Partial<AppProfileRule>) => onChange({ ...value, ...updates });

  return (
    <div className="app-rule-editor">
      <div className="app-rule-editor-grid">
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleName")}</span>
          <input
            className="settings-input"
            value={value.name}
            placeholder={t("settings.appRuleNamePlaceholder")}
            onChange={(event) => patch({ name: event.target.value })}
          />
        </label>
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleProcess")}</span>
          <input
            className="settings-input"
            value={value.process_name}
            placeholder="Code.exe"
            autoFocus
            onChange={(event) => patch({ process_name: event.target.value })}
          />
        </label>
      </div>

      <label className="settings-column app-rule-field">
        <span>{t("settings.appRuleWindowTitle")}</span>
        <input
          className="settings-input"
          value={value.window_title_contains ?? ""}
          placeholder={t("settings.appRuleWindowTitlePlaceholder")}
          onChange={(event) => patch({ window_title_contains: event.target.value || null })}
        />
      </label>

      <div className="app-rule-editor-grid">
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleBrowserDomain")}</span>
          <input
            className="settings-input"
            value={value.browser_domain ?? ""}
            placeholder="docs.example.com"
            onChange={(event) => patch({ browser_domain: event.target.value || null })}
          />
        </label>

        <label className="settings-column app-rule-field">
          <span>{t("settings.appRulePasteMethod")}</span>
          <PasteMethodSelect
            value={value.paste_method}
            onChange={(paste_method) => patch({ paste_method })}
          />
        </label>
      </div>

      <div className="app-rule-editor-grid">
        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleToneMode")}</span>
          <select
            className="settings-input app-rule-select"
            value={value.tone_mode ?? ""}
            onChange={(event) => patch({ tone_mode: event.target.value || null })}
          >
            <option value="">{t("settings.appRuleInherit")}</option>
            <option value="formal">{t("settings.toneFormal")}</option>
            <option value="casual">{t("settings.toneCasual")}</option>
            <option value="concise">{t("settings.toneConcise")}</option>
            <option value="bullets">{t("settings.toneBullets")}</option>
            <option value="email">{t("settings.toneEmail")}</option>
          </select>
        </label>

        <label className="settings-column app-rule-field">
          <span>{t("settings.appRuleHistory")}</span>
          <OverrideSelect value={value.history} onChange={(history) => patch({ history })} />
        </label>
      </div>

      <label className="settings-column app-rule-field">
        <span>{t("settings.appRuleVocabulary")}</span>
        <input
          className="settings-input"
          value={(value.vocabulary ?? []).join(", ")}
          placeholder="Tauri, SpeakFlow"
          onChange={(event) => patch({
            vocabulary: event.target.value.split(",").map((item) => item.trim()).filter(Boolean),
          })}
        />
      </label>

      <label className="settings-column app-rule-field">
        <span>{t("settings.appRuleTransformations")}</span>
        <input
          className="settings-input"
          value={(value.transformations ?? []).join(", ")}
          placeholder="spoken_symbols, spoken_formatting"
          onChange={(event) => patch({
            transformations: event.target.value.split(",").map((item) => item.trim()).filter(Boolean),
          })}
        />
      </label>

      <div className="app-rule-editor-actions">
        <button type="button" className="btn-ghost btn-ghost-sm" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="test-btn"
          disabled={!value.process_name.trim()}
          onClick={onSubmit}
        >
          {t("settings.appRuleSave")}
        </button>
      </div>
    </div>
  );
}

export default function AppProfileRulesSettingsSection({ profile, onSaved }: AppProfileRulesSettingsSectionProps) {
  const { t } = useTranslation();
  const [rules, setRules] = useState<AppProfileRule[]>([]);
  const [editing, setEditing] = useState<AppProfileRule | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setRules(profile?.app_profile_rules ?? []);
  }, [profile?.app_profile_rules]);

  const persist = async (next: AppProfileRule[]) => {
    const previous = rules;
    setRules(next);
    setSaving(true);
    try {
      await setAppProfileRules(next);
      onSaved();
      return true;
    } catch {
      setRules(previous);
      toast.error(t("settings.appRulesSaveFailed"));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const saveEditor = async () => {
    if (!editing) return;
    const normalized: AppProfileRule = {
      ...editing,
      name: editing.name.trim(),
      process_name: editing.process_name.trim(),
      window_title_contains: editing.window_title_contains?.trim() || null,
      browser_domain: editing.browser_domain?.trim() || null,
      translation_target: editing.translation_target?.trim() || null,
      custom_prompt: editing.custom_prompt?.trim() || null,
      tone_mode: editing.tone_mode?.trim() || null,
      vocabulary: (editing.vocabulary ?? []).map((item) => item.trim()).filter(Boolean),
      transformations: (editing.transformations ?? []).map((item) => item.trim()).filter(Boolean),
    };
    const exists = rules.some((rule) => rule.id === normalized.id);
    const next = exists
      ? rules.map((rule) => rule.id === normalized.id ? normalized : rule)
      : [...rules, normalized];
    if (await persist(next)) setEditing(null);
  };

  const move = (index: number, offset: -1 | 1) => {
    const target = index + offset;
    if (target < 0 || target >= rules.length) return;
    const next = [...rules];
    [next[index], next[target]] = [next[target], next[index]];
    void persist(next);
  };

  return (
    <section className="settings-card" data-nav-id="app-profiles">
      <div className="settings-section-header app-rules-header">
        <AppWindow size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.appProfiles")}</h2>
        <span className="app-rules-count">{rules.length}/100</span>
      </div>
      <div className="settings-column app-rules-column">
        <p className="settings-hint settings-hint-flush">{t("settings.appProfilesHint")}</p>

        {rules.length === 0 && !editing && (
          <div className="app-rules-empty">{t("settings.appRulesEmpty")}</div>
        )}

        {rules.map((rule, index) => (
          <div className="app-rule-card" key={rule.id} data-enabled={rule.enabled}>
            <button
              type="button"
              role="switch"
              aria-checked={rule.enabled}
              aria-label={t("settings.appRuleToggle", { name: rule.name || rule.process_name })}
              className="toggle-switch app-rule-toggle"
              data-active={rule.enabled}
              disabled={saving}
              onClick={() => {
                void persist(rules.map((item) => item.id === rule.id
                  ? { ...item, enabled: !item.enabled }
                  : item));
              }}
            >
              <div className="toggle-knob" />
            </button>
            <div className="app-rule-summary">
              <strong>{rule.name || rule.process_name}</strong>
              <span>{rule.process_name}{rule.window_title_contains ? ` · ${rule.window_title_contains}` : ""}{rule.browser_domain ? ` · ${rule.browser_domain}` : ""}</span>
            </div>
            <div className="app-rule-actions">
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleMoveUp")} disabled={saving || index === 0} onClick={() => move(index, -1)}>
                <ArrowUp size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleMoveDown")} disabled={saving || index === rules.length - 1} onClick={() => move(index, 1)}>
                <ArrowDown size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button" aria-label={t("settings.appRuleEdit")} disabled={saving} onClick={() => setEditing({ ...rule })}>
                <Pencil size={13} />
              </button>
              <button type="button" className="btn-ghost app-rule-icon-button app-rule-delete" aria-label={t("settings.appRuleDelete")} disabled={saving} onClick={() => { void persist(rules.filter((item) => item.id !== rule.id)); }}>
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        ))}

        {editing ? (
          <RuleEditor
            value={editing}
            onChange={setEditing}
            onCancel={() => setEditing(null)}
            onSubmit={() => { void saveEditor(); }}
          />
        ) : (
          <button type="button" className="btn-ghost app-rule-add" disabled={saving || rules.length >= 100} onClick={() => setEditing(createRule())}>
            <Plus size={13} />
            {t("settings.appRuleAdd")}
          </button>
        )}
      </div>
    </section>
  );
}
