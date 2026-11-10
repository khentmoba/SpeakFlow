import { useEffect, useState } from "react";
import { Volume2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setAudioBehavior } from "@/api/tauri";
import {
  clampDuckVolume,
  formatDuckVolume,
  withAudioBehaviorDefaults,
} from "@/lib/behaviorConfig";
import type { AudioBehaviorConfig, AudioDuckingMode, UserProfile } from "@/types";
import {
  SettingsRangeRow,
  SettingsSectionHeader,
  SettingsSelectRow,
  SettingsToggleRow,
} from "./SettingsControls";

interface AudioBehaviorSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

export default function AudioBehaviorSettingsSection({
  profile,
  onSaved,
}: AudioBehaviorSettingsSectionProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<AudioBehaviorConfig>(() =>
    withAudioBehaviorDefaults(profile?.audio_behavior));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setConfig(withAudioBehaviorDefaults(profile?.audio_behavior));
  }, [profile?.audio_behavior]);

  const persist = async (next: AudioBehaviorConfig) => {
    const previous = config;
    setConfig(next);
    setSaving(true);
    try {
      await setAudioBehavior(next);
      onSaved();
    } catch {
      setConfig(previous);
      toast.error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  const patch = (updates: Partial<AudioBehaviorConfig>) => {
    void persist({ ...config, ...updates });
  };

  const duckingOptions: readonly { value: AudioDuckingMode; label: string }[] = [
    { value: "off", label: t("settings.duckingOff") },
    { value: "duck", label: t("settings.duckingDuck") },
    { value: "mute", label: t("settings.duckingMute") },
    { value: "pause_media", label: t("settings.duckingPauseMedia") },
  ];

  return (
    <section className="settings-card" data-nav-id="audio-behavior">
      <SettingsSectionHeader
        icon={<Volume2 size={15} className="icon-accent" />}
        title={t("settings.audioBehavior")}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.audioBehaviorHint")}</p>

        <SettingsSelectRow
          label={t("settings.duckingMode")}
          value={config.duckingMode}
          options={duckingOptions}
          disabled={saving}
          onChange={(duckingMode) => patch({ duckingMode })}
        />

        {config.duckingMode === "duck" && (
          <SettingsRangeRow
            label={t("settings.duckVolume")}
            value={config.duckVolume}
            min={0}
            max={1}
            step={0.05}
            display={formatDuckVolume(config.duckVolume)}
            disabled={saving}
            onChange={(value) => patch({ duckVolume: clampDuckVolume(value) })}
          />
        )}

        {config.duckingMode !== "off" && (
          <SettingsToggleRow
            label={t("settings.speakersOnly")}
            hint={t("settings.speakersOnlyHint")}
            checked={config.speakersOnly}
            disabled={saving}
            onChange={(speakersOnly) => patch({ speakersOnly })}
          />
        )}

        <SettingsToggleRow
          label={t("settings.avoidBluetoothMic")}
          hint={t("settings.avoidBluetoothMicHint")}
          checked={config.avoidBluetoothMicrophone}
          disabled={saving}
          onChange={(avoidBluetoothMicrophone) => patch({ avoidBluetoothMicrophone })}
        />

        <SettingsToggleRow
          label={t("settings.releaseMicWhileIdle")}
          hint={t("settings.releaseMicWhileIdleHint")}
          checked={config.releaseMicWhileIdle}
          disabled={saving}
          onChange={(releaseMicWhileIdle) => patch({ releaseMicWhileIdle })}
        />

        <SettingsToggleRow
          label={t("settings.preRoll")}
          hint={t("settings.preRollHint")}
          checked={config.preRollEnabled}
          disabled={saving || config.releaseMicWhileIdle}
          onChange={(preRollEnabled) => patch({ preRollEnabled })}
        />

        <SettingsToggleRow
          label={t("settings.adaptiveReleaseTail")}
          hint={t("settings.adaptiveReleaseTailHint")}
          checked={config.adaptiveReleaseTail}
          disabled={saving}
          onChange={(adaptiveReleaseTail) => patch({ adaptiveReleaseTail })}
        />
      </div>
    </section>
  );
}
