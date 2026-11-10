import { useEffect, useState } from "react";
import { Users } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setMeetingsConfig } from "@/api/tauri";
import {
  clampKeepAudioDays,
  clampTranscriptFontSize,
  withMeetingsDefaults,
} from "@/lib/behaviorConfig";
import type { MeetingsConfig, UserProfile } from "@/types";
import {
  SettingsRangeRow,
  SettingsSectionHeader,
  SettingsToggleRow,
} from "./SettingsControls";

interface MeetingsSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

export default function MeetingsSettingsSection({
  profile,
  onSaved,
}: MeetingsSettingsSectionProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<MeetingsConfig>(() =>
    withMeetingsDefaults(profile?.meetings));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setConfig(withMeetingsDefaults(profile?.meetings));
  }, [profile?.meetings]);

  const patch = async (updates: Partial<MeetingsConfig>) => {
    const previous = config;
    const next = { ...config, ...updates };
    setConfig(next);
    setSaving(true);
    try {
      await setMeetingsConfig(next);
      onSaved();
    } catch {
      setConfig(previous);
      toast.error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="meetings">
      <SettingsSectionHeader
        icon={<Users size={15} className="icon-accent" />}
        title={t("settings.meetings")}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.meetingsHint")}</p>

        <SettingsToggleRow
          label={t("settings.meetingsCallDetection")}
          hint={t("settings.meetingsCallDetectionHint")}
          checked={config.callDetectionEnabled}
          disabled={saving}
          onChange={(callDetectionEnabled) => { void patch({ callDetectionEnabled }); }}
        />

        <SettingsToggleRow
          label={t("settings.meetingsMoreCallApps")}
          checked={config.moreCallApps}
          disabled={saving || !config.callDetectionEnabled}
          onChange={(moreCallApps) => { void patch({ moreCallApps }); }}
        />

        <SettingsToggleRow
          label={t("settings.meetingsAutoSummarize")}
          hint={t("settings.meetingsAutoSummarizeHint")}
          checked={config.autoSummarize}
          disabled={saving}
          onChange={(autoSummarize) => { void patch({ autoSummarize }); }}
        />

        <SettingsToggleRow
          label={t("settings.meetingsSaveMarkdown")}
          checked={config.saveMarkdownTranscripts}
          disabled={saving}
          onChange={(saveMarkdownTranscripts) => { void patch({ saveMarkdownTranscripts }); }}
        />

        {config.saveMarkdownTranscripts && (
          <label className="settings-column">
            <span className="settings-option-label">{t("settings.meetingsMarkdownDir")}</span>
            <input
              className="settings-input"
              value={config.markdownOutputDir ?? ""}
              placeholder={t("settings.meetingsMarkdownDirHint")}
              disabled={saving}
              onChange={(event) => { void patch({ markdownOutputDir: event.target.value || null }); }}
            />
          </label>
        )}

        <SettingsRangeRow
          label={t("settings.meetingsKeepAudioDays")}
          value={config.keepAudioDays}
          min={0}
          max={365}
          step={1}
          display={t("settings.meetingsKeepAudioDaysValue", { days: config.keepAudioDays })}
          disabled={saving}
          onChange={(value) => { void patch({ keepAudioDays: clampKeepAudioDays(value) }); }}
        />

        <SettingsRangeRow
          label={t("settings.meetingsTranscriptFontSize")}
          value={config.transcriptFontSize}
          min={10}
          max={24}
          step={1}
          display={`${config.transcriptFontSize}px`}
          disabled={saving}
          onChange={(value) => { void patch({ transcriptFontSize: clampTranscriptFontSize(value) }); }}
        />
      </div>
    </section>
  );
}
