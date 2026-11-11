import type { RefObject } from "react";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SettingsGroup, SettingsSection } from "@/pages/settings/settingsGroups";

interface SettingsPanesProps {
  groups: readonly SettingsGroup[];
  sections: readonly SettingsSection[];
  activeGroupId: string;
  activeSectionId: string;
  query: string;
  searching: boolean;
  chipsRef: RefObject<HTMLDivElement | null>;
  onQueryChange: (value: string) => void;
  onSelectGroup: (id: string) => void;
  onSelectSection: (id: string) => void;
}

/**
 * Settings navigation as panes: a group tab strip over the chips of the group's
 * own sections. A search replaces the chips with every match across groups, so
 * the tab strip stays put and the content never moves under the pointer.
 */
export default function SettingsPanes({
  groups,
  sections,
  activeGroupId,
  activeSectionId,
  query,
  searching,
  chipsRef,
  onQueryChange,
  onSelectGroup,
  onSelectSection,
}: SettingsPanesProps) {
  const { t } = useTranslation();

  return (
    <div className="settings-panes">
      <div className="settings-panes-head">
        <div className="settings-panes-tabs" role="group" aria-label={t("settings.title")}>
          {groups.map((group) => (
            <button
              key={group.id}
              type="button"
              className="settings-panes-tab"
              aria-pressed={!searching && activeGroupId === group.id}
              data-active={!searching && activeGroupId === group.id}
              onClick={() => onSelectGroup(group.id)}
            >
              {t(group.labelKey)}
            </button>
          ))}
        </div>
        <label className="settings-panes-search">
          <Search size={12} strokeWidth={1.5} aria-hidden="true" />
          <input
            type="search"
            value={query}
            placeholder={t("settings.searchSettingsPlaceholder")}
            aria-label={t("settings.searchSettings")}
            onChange={(event) => onQueryChange(event.target.value)}
          />
        </label>
      </div>

      <div className="settings-panes-chips" ref={chipsRef}>
        {sections.map(({ id, labelKey }) => (
          <button
            key={id}
            type="button"
            className="settings-panes-chip"
            aria-current={activeSectionId === id ? "location" : undefined}
            data-active={activeSectionId === id}
            data-nav-tab={id}
            onClick={() => onSelectSection(id)}
          >
            {t(labelKey)}
          </button>
        ))}
        {sections.length === 0 && (
          <p className="settings-panes-empty" role="status">{t("settings.noSettingsMatch")}</p>
        )}
      </div>
    </div>
  );
}
