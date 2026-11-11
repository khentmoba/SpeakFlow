/**
 * The settings page is organised as panes: one group of sections is visible at
 * a time, chosen by the tab strip, and the sections inside it are chips. The
 * groups partition every section exactly once, so no section can be lost by a
 * layout change — `settingsGroups.test.ts` holds that invariant.
 */

export interface SettingsSection {
  id: string;
  labelKey: string;
}

export interface SettingsGroup {
  id: string;
  labelKey: string;
  sectionIds: readonly string[];
}

export const SETTINGS_SECTIONS: readonly SettingsSection[] = [
  { id: "appearance", labelKey: "settings.appearance" },
  { id: "engine", labelKey: "settings.engine" },
  { id: "custom-models", labelKey: "settings.customModels" },
  { id: "ai-models", labelKey: "settings.aiModels" },
  { id: "hotkey", labelKey: "settings.hotkeySection" },
  { id: "shortcuts-behavior", labelKey: "settings.shortcutsBehavior" },
  { id: "microphone", labelKey: "settings.microphone" },
  { id: "audio-behavior", labelKey: "settings.audioBehavior" },
  { id: "input", labelKey: "settings.inputMethod" },
  { id: "output-behavior", labelKey: "settings.outputBehavior" },
  { id: "dictation-tools", labelKey: "settings.dictationTools" },
  { id: "scripting", labelKey: "settings.scripting" },
  { id: "app-profiles", labelKey: "settings.appProfiles" },
  { id: "meetings", labelKey: "settings.meetings" },
  { id: "history-settings", labelKey: "settings.historySettings" },
  { id: "vocabulary", labelKey: "settings.vocabulary" },
  { id: "startup", labelKey: "settings.startup" },
  { id: "labs", labelKey: "settings.labs" },
  { id: "updates", labelKey: "settings.update" },
  { id: "support", labelKey: "settings.support" },
  { id: "keyboard-models", labelKey: "settings.keyboardModels" },
];

export const SETTINGS_GROUPS: readonly SettingsGroup[] = [
  {
    id: "general",
    labelKey: "settings.groupGeneral",
    sectionIds: ["appearance", "startup", "labs", "updates", "support"],
  },
  {
    id: "dictation",
    labelKey: "settings.groupDictation",
    sectionIds: ["engine", "keyboard-models", "custom-models", "ai-models", "hotkey", "shortcuts-behavior"],
  },
  {
    id: "audio",
    labelKey: "settings.groupAudio",
    sectionIds: ["microphone", "audio-behavior"],
  },
  {
    id: "output",
    labelKey: "settings.groupOutput",
    sectionIds: ["input", "output-behavior", "dictation-tools", "scripting", "app-profiles"],
  },
  {
    id: "data",
    labelKey: "settings.groupData",
    sectionIds: ["meetings", "history-settings", "vocabulary"],
  },
];

/** Sections of one group, in the order the group lists them. */
export function sectionsInGroup(
  groupId: string,
  sections: readonly SettingsSection[] = SETTINGS_SECTIONS,
): SettingsSection[] {
  const group = SETTINGS_GROUPS.find((candidate) => candidate.id === groupId);
  if (!group) return [];
  return group.sectionIds
    .map((id) => sections.find((section) => section.id === id))
    .filter((section): section is SettingsSection => Boolean(section));
}

/** The group that owns a section, for keeping the tab strip in step with the content. */
export function groupForSection(sectionId: string): SettingsGroup | undefined {
  return SETTINGS_GROUPS.find((group) => group.sectionIds.includes(sectionId));
}
