import type { RefObject } from "react";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { settingsSectionMatches } from "@/pages/settings/settingsSearch";

export interface SettingsNavSection {
  id: string;
  labelKey: string;
}

interface SettingsNavProps {
  sections: readonly SettingsNavSection[];
  activeId: string;
  query: string;
  navRef: RefObject<HTMLElement | null>;
  onQueryChange: (value: string) => void;
  onSelect: (id: string) => void;
}

/**
 * Section rail with a filter. The page hides non-matching sections with the
 * same matcher, so the rail and the content never disagree.
 */
export default function SettingsNav({
  sections,
  activeId,
  query,
  navRef,
  onQueryChange,
  onSelect,
}: SettingsNavProps) {
  const { t } = useTranslation();
  const visible = sections.filter(({ id, labelKey }) =>
    settingsSectionMatches(id, t(labelKey), query));
  const filtering = query.trim().length > 0;

  return (
    <nav className="settings-nav" aria-label={t("settings.title")} ref={navRef}>
      <div className="settings-nav-head">
        <span className="settings-nav-title">{t("settings.title")}</span>
        <span className="settings-nav-count">
          {String(filtering ? visible.length : sections.length).padStart(2, "0")}
        </span>
      </div>
      <label className="settings-nav-search">
        <Search size={12} strokeWidth={1.5} aria-hidden="true" />
        <input
          type="search"
          value={query}
          placeholder={t("settings.searchSettingsPlaceholder")}
          aria-label={t("settings.searchSettings")}
          onChange={(event) => onQueryChange(event.target.value)}
        />
      </label>
      {visible.map(({ id, labelKey }) => {
        const index = sections.findIndex((section) => section.id === id);
        return (
          <button
            key={id}
            type="button"
            className="settings-nav-tab"
            aria-current={activeId === id ? "location" : undefined}
            data-active={activeId === id}
            data-nav-tab={id}
            onClick={() => onSelect(id)}
          >
            <span className="settings-nav-index" aria-hidden="true">
              {String(index + 1).padStart(2, "0")}
            </span>
            <span className="settings-nav-label">{t(labelKey)}</span>
          </button>
        );
      })}
      {filtering && visible.length === 0 && (
        <p className="settings-nav-empty" role="status">{t("settings.noSettingsMatch")}</p>
      )}
    </nav>
  );
}
