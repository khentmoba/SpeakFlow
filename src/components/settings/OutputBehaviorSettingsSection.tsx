import { useEffect, useState } from "react";
import { ClipboardPaste } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setOutputBehavior } from "@/api/tauri";
import { withOutputBehaviorDefaults } from "@/lib/behaviorConfig";
import type { OutputBehaviorConfig, UserProfile } from "@/types";
import { SettingsSectionHeader, SettingsToggleRow } from "./SettingsControls";

interface OutputBehaviorSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

export default function OutputBehaviorSettingsSection({
  profile,
  onSaved,
}: OutputBehaviorSettingsSectionProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<OutputBehaviorConfig>(() =>
    withOutputBehaviorDefaults(profile?.output_behavior));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setConfig(withOutputBehaviorDefaults(profile?.output_behavior));
  }, [profile?.output_behavior]);

  const patch = async (updates: Partial<OutputBehaviorConfig>) => {
    const previous = config;
    const next = { ...config, ...updates };
    setConfig(next);
    setSaving(true);
    try {
      await setOutputBehavior(next);
      onSaved();
    } catch {
      setConfig(previous);
      toast.error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="output-behavior">
      <SettingsSectionHeader
        icon={<ClipboardPaste size={15} className="icon-accent" />}
        title={t("settings.outputBehavior")}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.outputBehaviorHint")}</p>

        <SettingsToggleRow
          label={t("settings.copyOnly")}
          hint={t("settings.copyOnlyHint")}
          checked={config.copyOnly}
          disabled={saving}
          onChange={(copyOnly) => { void patch({ copyOnly }); }}
        />

        <SettingsToggleRow
          label={t("settings.keepOnClipboard")}
          hint={t("settings.keepOnClipboardHint")}
          checked={config.keepOnClipboard}
          disabled={saving || config.copyOnly}
          onChange={(keepOnClipboard) => { void patch({ keepOnClipboard }); }}
        />

        <SettingsToggleRow
          label={t("settings.autoTerminalCtrlShiftV")}
          hint={t("settings.autoTerminalCtrlShiftVHint")}
          checked={config.autoTerminalCtrlShiftV}
          disabled={saving || config.copyOnly}
          onChange={(autoTerminalCtrlShiftV) => { void patch({ autoTerminalCtrlShiftV }); }}
        />

        <SettingsToggleRow
          label={t("settings.pasteWithShift")}
          hint={t("settings.pasteWithShiftHint")}
          checked={config.pasteWithShift}
          disabled={saving || config.copyOnly}
          onChange={(pasteWithShift) => { void patch({ pasteWithShift }); }}
        />
      </div>
    </section>
  );
}
