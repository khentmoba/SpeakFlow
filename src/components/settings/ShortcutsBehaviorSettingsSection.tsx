import { useEffect, useState } from "react";
import { Keyboard } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setShortcutsBehavior } from "@/api/tauri";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { withShortcutsBehaviorDefaults } from "@/lib/behaviorConfig";
import { formatHotkeyForDisplay } from "@/lib/hotkey";
import type { ShortcutsBehaviorConfig, UserProfile } from "@/types";
import { SettingsSectionHeader, SettingsToggleRow } from "./SettingsControls";

interface ShortcutsBehaviorSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

type ShortcutKey =
  | "secondDictationHotkey"
  | "terminalPasteHotkey"
  | "typedTextHotkey"
  | "meetingHotkey";

const SHORTCUT_ROWS: readonly { key: ShortcutKey; labelKey: string }[] = [
  { key: "secondDictationHotkey", labelKey: "settings.secondDictationHotkey" },
  { key: "terminalPasteHotkey", labelKey: "settings.terminalPasteHotkey" },
  { key: "typedTextHotkey", labelKey: "settings.typedTextHotkey" },
  { key: "meetingHotkey", labelKey: "settings.meetingHotkey" },
];

export default function ShortcutsBehaviorSettingsSection({
  profile,
  onSaved,
}: ShortcutsBehaviorSettingsSectionProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<ShortcutsBehaviorConfig>(() =>
    withShortcutsBehaviorDefaults(profile?.shortcuts_behavior));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setConfig(withShortcutsBehaviorDefaults(profile?.shortcuts_behavior));
  }, [profile?.shortcuts_behavior]);

  const persist = async (next: ShortcutsBehaviorConfig) => {
    const previous = config;
    setConfig(next);
    setSaving(true);
    try {
      await setShortcutsBehavior(next);
      onSaved();
    } catch {
      setConfig(previous);
      toast.error(t("settings.behaviorSaveFailed"));
      throw new Error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  // One capture hook per row: hooks cannot be created in a loop.
  const secondCapture = useHotkeyCapture({
    save: async (shortcut) => { await persist({ ...config, secondDictationHotkey: shortcut }); },
    label: t("settings.secondDictationHotkey"),
  });
  const terminalCapture = useHotkeyCapture({
    save: async (shortcut) => { await persist({ ...config, terminalPasteHotkey: shortcut }); },
    label: t("settings.terminalPasteHotkey"),
  });
  const typedTextCapture = useHotkeyCapture({
    save: async (shortcut) => { await persist({ ...config, typedTextHotkey: shortcut }); },
    label: t("settings.typedTextHotkey"),
  });
  const meetingCapture = useHotkeyCapture({
    save: async (shortcut) => { await persist({ ...config, meetingHotkey: shortcut }); },
    label: t("settings.meetingHotkey"),
  });

  const captures = {
    secondDictationHotkey: secondCapture,
    terminalPasteHotkey: terminalCapture,
    typedTextHotkey: typedTextCapture,
    meetingHotkey: meetingCapture,
  } as const;

  return (
    <section className="settings-card" data-nav-id="shortcuts-behavior">
      <SettingsSectionHeader
        icon={<Keyboard size={15} className="icon-accent" />}
        title={t("settings.shortcutsBehavior")}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.shortcutsBehaviorHint")}</p>

        {SHORTCUT_ROWS.map(({ key, labelKey }) => {
          const capture = captures[key];
          const value = config[key];
          const label = t(labelKey);
          return (
            <div className="settings-row" key={key}>
              <span className="settings-option-label">{label}</span>
              <div className="settings-header-actions">
                <button
                  type="button"
                  className="theme-btn hotkey-capture-btn"
                  disabled={capture.saving || saving}
                  data-capturing={capture.capturing}
                  aria-label={label}
                  onClick={() => capture.startCapture()}
                >
                  {capture.capturing
                    ? t("settings.pressCombo")
                    : value
                      ? formatHotkeyForDisplay(value)
                      : t("settings.shortcutUnset")}
                </button>
                {value && (
                  <button
                    type="button"
                    className="btn-ghost btn-ghost-sm"
                    disabled={capture.saving || saving}
                    onClick={() => { void persist({ ...config, [key]: null }); }}
                  >
                    {t("common.clear")}
                  </button>
                )}
              </div>
            </div>
          );
        })}

        <SettingsToggleRow
          label={t("settings.shareDictationShortcut")}
          hint={t("settings.shareDictationShortcutHint")}
          checked={config.shareDictationShortcut}
          disabled={saving}
          onChange={(shareDictationShortcut) => { void persist({ ...config, shareDictationShortcut }); }}
        />
      </div>
    </section>
  );
}
