import { useMemo, useState } from "react";
import { Check, Download, Languages, Search, Star } from "lucide-react";
import { useTranslation } from "react-i18next";
import { engineDisplayDescription, engineOptionLabel, NON_CATALOG_OPTIONS } from "@/lib/asrEngineOptions";
import {
  CATALOG_MODELS,
  catalogLanguages,
  formatModelSize,
  languageName,
  type CatalogModel,
} from "@/lib/asrCatalog";
import {
  accuracyFor,
  benchmarkFor,
  filterCatalogModels,
  formatModelSpeed,
  gradeScore,
} from "@/lib/asrBenchmarks";

interface ModelBrowserProps {
  engine: string;
  onSelect: (engine: string) => void;
  disabled: boolean;
  installed: ReadonlySet<string>;
}

/** Installed models plus this many recommendations when the list is folded. */
const SHORT_LIST_RECOMMENDATIONS = 5;

function gradeTone(grade: string): "good" | "fair" | "poor" {
  const score = gradeScore(grade);
  if (score >= 6) return "good";
  if (score >= 3) return "fair";
  return "poor";
}

function ModelRow({
  model,
  language,
  selected,
  installed,
  disabled,
  onSelect,
}: {
  model: CatalogModel;
  language: string | "all";
  selected: boolean;
  installed: boolean;
  disabled: boolean;
  onSelect: (engine: string) => void;
}) {
  const { t } = useTranslation();
  const accuracy = accuracyFor(model, language);
  const speed = formatModelSpeed(benchmarkFor(model.key)?.speed ?? null);
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      className="model-browser-row"
      data-active={selected}
      disabled={disabled}
      onClick={() => onSelect(model.key)}
    >
      <span className="model-browser-row-head">
        <strong>{model.name}</strong>
        <span className="model-badges">
          {model.recommended && (
            <span className="model-badge model-badge-recommended">
              <Star size={10} strokeWidth={2} aria-hidden="true" />
              {t("settings.modelRecommended")}
            </span>
          )}
          {model.translation && (
            <span className="model-badge">
              <Languages size={10} strokeWidth={2} aria-hidden="true" />
              {t("settings.modelTranslate")}
            </span>
          )}
        </span>
      </span>
      <span className="model-browser-desc">{model.description}</span>
      <span className="model-browser-stats">
        {accuracy && (
          <span className="model-stat">
            <span className="model-stat-label">{languageName(accuracy.language)}</span>
            <span className="model-grade" data-tone={gradeTone(accuracy.grade)}>
              {accuracy.grade}
            </span>
            <span>{t("settings.modelErrorRate", { percent: accuracy.errorPercent })}</span>
          </span>
        )}
        {speed && (
          <span className="model-stat">
            {t("settings.modelSpeed", { seconds: speed })}
          </span>
        )}
        <span className="model-stat">
          {t("settings.modelLanguageCount", { count: model.languages.length })}
        </span>
        <span className="model-stat">{formatModelSize(model.size)}</span>
        <span className="model-license">{model.license}</span>
      </span>
      <span className="model-browser-action">
        {selected ? (
          <>
            <Check size={12} strokeWidth={2} aria-hidden="true" />
            {t("settings.modelSelected")}
          </>
        ) : installed ? (
          t("settings.modelUse")
        ) : (
          <>
            <Download size={12} strokeWidth={2} aria-hidden="true" />
            {t("settings.modelDownload")}
          </>
        )}
      </span>
    </button>
  );
}

function OtherEngineRow({
  engineKey,
  selected,
  disabled,
  onSelect,
}: {
  engineKey: string;
  selected: boolean;
  disabled: boolean;
  onSelect: (engine: string) => void;
}) {
  const { t } = useTranslation();
  const option = NON_CATALOG_OPTIONS.find((item) => item.key === engineKey)!;
  const Icon = option.icon;
  return (
    <button
      type="button"
      role="option"
      aria-selected={selected}
      className="model-browser-row"
      data-active={selected}
      disabled={disabled}
      onClick={() => onSelect(engineKey)}
    >
      <span className="model-browser-row-head">
        <Icon size={13} strokeWidth={1.5} aria-hidden="true" />
        <strong>{engineOptionLabel(option, t)}</strong>
      </span>
      <span className="model-browser-desc">{engineDisplayDescription(engineKey, t)}</span>
      <span className="model-browser-action">
        {selected ? (
          <>
            <Check size={12} strokeWidth={2} aria-hidden="true" />
            {t("settings.modelSelected")}
          </>
        ) : (
          t("settings.modelUse")
        )}
      </span>
    </button>
  );
}

/**
 * Language rail plus a ranked model list. Grades, speed and sizes come from the
 * generated catalog; the ranking is per selected language, best grade first.
 */
export function ModelBrowser({ engine, onSelect, disabled, installed }: ModelBrowserProps) {
  const { t } = useTranslation();
  const [language, setLanguage] = useState<string | "all">("all");
  const [query, setQuery] = useState("");
  const [showAll, setShowAll] = useState(false);
  const languages = useMemo(() => catalogLanguages(), []);
  const models = useMemo(
    () => filterCatalogModels({ language, query }, CATALOG_MODELS),
    [language, query],
  );
  // A short first view: what is installed, plus a few recommendations. Search
  // and "Show all" reveal the rest.
  const searching = query.trim().length > 0;
  // The collapsed list is computed whether or not it is shown, so "Show fewer"
  // only appears when collapsing actually hides something.
  const collapsed = useMemo(() => {
    if (searching) return models;
    const installedModels = models.filter((model) => installed.has(model.key));
    const recommended = models.filter((model) => !installed.has(model.key) && model.recommended);
    const fallback = models.filter((model) => !installed.has(model.key) && !model.recommended);
    const short = [
      ...installedModels,
      ...(recommended.length > 0 ? recommended : fallback).slice(0, SHORT_LIST_RECOMMENDATIONS),
    ];
    // The selected model stays visible even when it is neither installed nor
    // recommended, so the list never hides the current choice.
    const selected = models.find((model) => model.key === engine);
    if (selected) short.push(selected);
    // Keep the ranked order rather than the grouping above.
    return models.filter((model) => short.includes(model));
  }, [engine, installed, models, searching]);
  const folded = showAll || searching ? models : collapsed;
  const hiddenCount = models.length - collapsed.length;
  // R2T2 and the cloud services are not catalog models, so the language rail
  // does not apply to them; they are listed whenever no language is chosen.
  const otherEngines = useMemo(() => {
    if (language !== "all") return [];
    const needle = query.trim().toLowerCase();
    return NON_CATALOG_OPTIONS.filter((option) => !needle
      || engineOptionLabel(option, t).toLowerCase().includes(needle)
      || engineDisplayDescription(option.key, t).toLowerCase().includes(needle));
  }, [language, query, t]);

  return (
    <div className="model-browser">
      <div className="model-browser-toolbar">
        <label className="model-browser-search">
          <Search size={13} strokeWidth={1.5} aria-hidden="true" />
          <input
            type="search"
            className="picker-search-input"
            placeholder={t("settings.searchModelsPlaceholder")}
            aria-label={t("settings.searchModels")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <span className="model-browser-count">
          {t("settings.modelCount", { count: models.length })}
        </span>
      </div>
      <div className="model-browser-body">
        <div className="model-browser-rail" role="group" aria-label={t("settings.filterByLanguage")}>
          <button
            type="button"
            className="model-browser-rail-item"
            aria-pressed={language === "all"}
            onClick={() => setLanguage("all")}
          >
            <span>{t("settings.allLanguages")}</span>
          </button>
          {languages.map(({ code, modelCount }) => (
            <button
              key={code}
              type="button"
              className="model-browser-rail-item"
              aria-pressed={language === code}
              onClick={() => setLanguage(code)}
            >
              <span>{languageName(code)}</span>
              <span className="model-browser-rail-count">{modelCount}</span>
            </button>
          ))}
        </div>
        <div className="model-browser-list" role="listbox" aria-label={t("settings.browseModels")}>
          {models.length === 0 && otherEngines.length === 0 ? (
            <div className="picker-empty">{t("settings.modelNoResults")}</div>
          ) : (
            <>
              <div role="group" aria-label={t("settings.modelListGroup")}>
                {folded.map((model) => (
                  <ModelRow
                    key={model.key}
                    model={model}
                    language={language}
                    selected={engine === model.key}
                    installed={installed.has(model.key)}
                    disabled={disabled}
                    onSelect={onSelect}
                  />
                ))}
              </div>
              {otherEngines.length > 0 && (
                <div role="group" aria-label={t("settings.otherEngines")}>
                  <div className="model-browser-divider">{t("settings.otherEngines")}</div>
                  {otherEngines.map((option) => (
                    <OtherEngineRow
                      key={option.key}
                      engineKey={option.key}
                      selected={engine === option.key}
                      disabled={disabled}
                      onSelect={onSelect}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
        {!showAll && hiddenCount > 0 && (
          <button
            type="button"
            className="model-browser-more"
            onClick={() => setShowAll(true)}
          >
            {t("settings.showAllModels", { count: models.length })}
          </button>
        )}
        {showAll && !searching && hiddenCount > 0 && (
          <button
            type="button"
            className="model-browser-more"
            onClick={() => setShowAll(false)}
          >
            {t("settings.showFewerModels")}
          </button>
        )}
      </div>
      <p className="settings-hint model-browser-hint">
        {t("settings.modelBrowserHint")}
      </p>
    </div>
  );
}
