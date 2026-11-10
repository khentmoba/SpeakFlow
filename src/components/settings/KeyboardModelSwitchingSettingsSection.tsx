import { useEffect, useState } from "react";
import { Keyboard, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { getActiveKeyboardLocale, setKeyboardModelSwitching } from "@/api/tauri";
import { engineOptions } from "@/lib/asrEngineOptions";
import type { KeyboardModelRule, UserProfile } from "@/types";
import { SettingsSectionHeader, SettingsToggleRow } from "./SettingsControls";

interface KeyboardModelSwitchingSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

/**
 * Switches the ASR engine when the active keyboard layout changes, e.g. a
 * Chinese layout selecting a Chinese-tuned model. The backend reads the rules
 * from the profile, so this card is the only place they are authored.
 */
export default function KeyboardModelSwitchingSettingsSection({
  profile,
  onSaved,
}: KeyboardModelSwitchingSettingsSectionProps) {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(profile?.keyboard_model_switching ?? false);
  const [rules, setRules] = useState<KeyboardModelRule[]>(profile?.keyboard_model_rules ?? []);
  const [activeLocale, setActiveLocale] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setEnabled(profile?.keyboard_model_switching ?? false);
    setRules(profile?.keyboard_model_rules ?? []);
  }, [profile?.keyboard_model_switching, profile?.keyboard_model_rules]);

  useEffect(() => {
    void getActiveKeyboardLocale()
      .then(setActiveLocale)
      .catch(() => setActiveLocale(null));
  }, []);

  const persist = async (nextEnabled: boolean, nextRules: KeyboardModelRule[]) => {
    const previous = { enabled, rules };
    setEnabled(nextEnabled);
    setRules(nextRules);
    setSaving(true);
    try {
      await setKeyboardModelSwitching(nextEnabled, nextRules);
      onSaved();
    } catch {
      setEnabled(previous.enabled);
      setRules(previous.rules);
      toast.error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const updateRule = (index: number, patch: Partial<KeyboardModelRule>) => {
    const next = rules.map((rule, i) => (i === index ? { ...rule, ...patch } : rule));
    void persist(enabled, next);
  };

  return (
    <section className="settings-card" data-nav-id="keyboard-models">
      <SettingsSectionHeader
        icon={<Keyboard size={15} className="icon-accent" />}
        title={t("settings.keyboardModels")}
        trailing={(
          <span className="settings-option-desc">
            {activeLocale
              ? t("settings.keyboardModelsActive", { locale: activeLocale })
              : t("settings.keyboardModelsUnknown")}
          </span>
        )}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.keyboardModelsHint")}</p>

        <SettingsToggleRow
          label={t("settings.keyboardModelsEnabled")}
          hint={t("settings.keyboardModelsEnabledHint")}
          checked={enabled}
          disabled={saving}
          onChange={(value) => { void persist(value, rules); }}
        />

        {enabled && (
          <>
            {rules.length === 0 ? (
              <div className="app-rules-empty">{t("settings.keyboardModelsEmpty")}</div>
            ) : (
              rules.map((rule, index) => (
                <div className="app-rule-card" key={`${rule.layoutTag}-${index}`}>
                  <div className="settings-row">
                    <input
                      className="settings-input"
                      aria-label={t("settings.keyboardModelsLayout")}
                      placeholder={t("settings.keyboardModelsLayoutPlaceholder")}
                      value={rule.layoutTag}
                      onChange={(event) => {
                        const next = rules.map((item, i) =>
                          i === index ? { ...item, layoutTag: event.target.value } : item,
                        );
                        setRules(next);
                      }}
                      onBlur={() => { void persist(enabled, rules); }}
                    />
                    <select
                      className="settings-input"
                      aria-label={t("settings.keyboardModelsEngine")}
                      value={rule.engine}
                      onChange={(event) => updateRule(index, { engine: event.target.value })}
                    >
                      {engineOptions.map((option) => (
                        <option key={option.key} value={option.key}>{option.label}</option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className="btn-ghost app-rule-icon-button app-rule-delete"
                      aria-label={t("settings.keyboardModelsRemove")}
                      disabled={saving}
                      onClick={() => {
                        void persist(enabled, rules.filter((_, i) => i !== index));
                      }}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              ))
            )}

            <div className="settings-row">
              <span className="history-action-spacer" />
              <button
                type="button"
                className="btn-ghost btn-ghost-sm"
                disabled={saving}
                onClick={() => {
                  void persist(enabled, [
                    ...rules,
                    { layoutTag: "", engine: engineOptions[0]?.key ?? "" },
                  ]);
                }}
              >
                <Plus size={12} aria-hidden="true" /> {t("settings.keyboardModelsAdd")}
              </button>
            </div>
          </>
        )}
      </div>
    </section>
  );
}
