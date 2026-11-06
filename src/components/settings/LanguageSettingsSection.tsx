import { Languages } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  languageName,
  modelLanguageOptions,
  resolveLanguage,
  type CatalogModel,
} from "@/lib/asrCatalog";
import { accuracyFor, languageQuality } from "@/lib/asrBenchmarks";

interface LanguageSettingsSectionProps {
  model: CatalogModel | null;
  language: string | null;
  translateToEnglish: boolean;
  disabled?: boolean;
  onLanguageChange: (language: string | null) => void;
  onTranslateChange: (enabled: boolean) => void;
}

/**
 * Per-model language pinning for the local transcribe.cpp models.
 *
 * The saved language is not tied to one model. It is resolved against the
 * selected model's own list, and the UI says when the saved choice is not
 * available instead of silently sending it to a model that cannot use it.
 */
export default function LanguageSettingsSection({
  model,
  language,
  translateToEnglish,
  disabled = false,
  onLanguageChange,
  onTranslateChange,
}: LanguageSettingsSectionProps) {
  const { t } = useTranslation();
  if (!model) return null;

  const effective = resolveLanguage(model, language);
  const options = modelLanguageOptions(model);
  const fixed = model.languages.length === 1;
  const accuracy = effective === "auto" ? null : accuracyFor(model, effective);
  const quality = languageQuality(accuracy?.grade);
  const savedUnavailable = Boolean(
    language && language !== "auto" && language !== effective,
  );

  return (
    <div className="settings-inline-panel" style={{ marginTop: 8 }}>
      <div className="settings-column" style={{ gap: 6 }}>
        <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
          <Languages size={13} className="icon-accent" aria-hidden="true" />
          <span className="settings-option-desc">
            {t("settings.transcriptionLanguage")}
          </span>
          {model.long_tail && (
            <span className="asr-capability-tag">{t("settings.longTail")}</span>
          )}
        </div>
        <select
          className="settings-input"
          aria-label={t("settings.transcriptionLanguage")}
          value={effective}
          disabled={disabled || fixed}
          onChange={(event) =>
            onLanguageChange(event.target.value === "auto" ? null : event.target.value)
          }
        >
          {model.language_detection && (
            <option value="auto">{t("settings.languageAuto")}</option>
          )}
          {savedUnavailable && (
            <option value={language!} disabled>
              {languageName(language!)} ({t("settings.languageUnavailable")})
            </option>
          )}
          {options.map(({ code, name }) => {
            const grade = accuracyFor(model, code)?.grade;
            return (
              <option key={code} value={code}>
                {grade ? `${name} (${grade})` : name}
              </option>
            );
          })}
        </select>
        <p className="settings-hint settings-hint-flush">
          {effective === "auto"
            ? t("settings.languageAutoHint")
            : fixed
              ? t("settings.languageFixed", { language: languageName(effective) })
              : language && language !== "auto"
                ? t("settings.languagePinnedHint", { language: languageName(effective) })
                : t("settings.languageDefaultHint", { language: languageName(effective) })}
        </p>
        {savedUnavailable && (
          <p className="settings-hint settings-hint-flush">
            {t("settings.languageUnavailableHint", {
              language: languageName(language!),
              effective:
                effective === "auto" ? t("settings.languageAuto") : languageName(effective),
            })}
          </p>
        )}
        {accuracy && quality !== "unknown" && (
          <p
            className={`settings-hint settings-hint-flush language-quality language-quality-${quality}`}
          >
            {t(`settings.languageQuality_${quality}`, {
              grade: accuracy.grade,
              percent: accuracy.errorPercent,
            })}
          </p>
        )}
        {model.long_tail && (
          <p className="settings-hint settings-hint-flush">
            {t("settings.longTailHint")}
          </p>
        )}
      </div>
      {model.translation && (
        <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
          <span className="settings-option-desc" style={{ flex: 1 }}>
            {t("settings.translateToEnglish")}
            <span className="settings-hint" style={{ display: "block" }}>
              {t("settings.translateToEnglishHint")}
            </span>
            {!model.language_detection && (!language || language === "auto") && (
              <span className="settings-hint" style={{ display: "block" }}>
                {t("settings.translateNeedsLanguage")}
              </span>
            )}
          </span>
          <button
            type="button"
            role="switch"
            aria-checked={translateToEnglish}
            aria-label={t("settings.translateToEnglish")}
            className="toggle-switch"
            disabled={disabled}
            onClick={() => onTranslateChange(!translateToEnglish)}
          >
            <div className="toggle-knob" />
          </button>
        </div>
      )}
    </div>
  );
}
