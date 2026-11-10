import { useEffect, useState } from "react";
import { FlaskConical } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setLabsConfig } from "@/api/tauri";
import { LABS_FAILURE_NOTICES_KEY, LABS_OUTPUT_FIXES_KEY } from "@/lib/constants";
import { withLabsDefaults } from "@/lib/behaviorConfig";
import { writeLocalStorage } from "@/lib/storage";
import type { LabsConfig, UserProfile } from "@/types";
import { SettingsSectionHeader, SettingsToggleRow } from "./SettingsControls";

interface LabsSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

type LabsFlag = Exclude<keyof LabsConfig, never>;

const LABS_ROWS: readonly { key: LabsFlag; labelKey: string; hintKey: string }[] = [
  { key: "failureNotices", labelKey: "settings.labsFailureNotices", hintKey: "settings.labsFailureNoticesHint" },
  { key: "returnToApp", labelKey: "settings.labsReturnToApp", hintKey: "settings.labsReturnToAppHint" },
  { key: "smartSpacing", labelKey: "settings.labsSmartSpacing", hintKey: "settings.labsSmartSpacingHint" },
  { key: "spokenFormatting", labelKey: "settings.labsSpokenFormatting", hintKey: "settings.labsSpokenFormattingHint" },
  { key: "spokenSymbols", labelKey: "settings.labsSpokenSymbols", hintKey: "settings.labsSpokenSymbolsHint" },
  { key: "skipPauses", labelKey: "settings.labsSkipPauses", hintKey: "settings.labsSkipPausesHint" },
  { key: "tapOrHold", labelKey: "settings.labsTapOrHold", hintKey: "settings.labsTapOrHoldHint" },
  { key: "standaloneSpecialKeys", labelKey: "settings.labsStandaloneSpecialKeys", hintKey: "settings.labsStandaloneSpecialKeysHint" },
  { key: "modelInPlace", labelKey: "settings.labsModelInPlace", hintKey: "settings.labsModelInPlaceHint" },
  { key: "pickerSortFilter", labelKey: "settings.labsPickerSortFilter", hintKey: "" },
  { key: "benchmarkDetails", labelKey: "settings.labsBenchmarkDetails", hintKey: "" },
  { key: "outputFixes", labelKey: "settings.labsOutputFixes", hintKey: "settings.labsOutputFixesHint" },
  { key: "apiProcessing", labelKey: "settings.labsApiProcessing", hintKey: "settings.labsApiProcessingHint" },
  { key: "meetingFinishedEvent", labelKey: "settings.labsMeetingFinishedEvent", hintKey: "" },
  { key: "meetingNotesVocabulary", labelKey: "settings.labsMeetingNotesVocabulary", hintKey: "" },
  { key: "expireMeetingAudio", labelKey: "settings.labsExpireMeetingAudio", hintKey: "" },
];

export default function LabsSettingsSection({ profile, onSaved }: LabsSettingsSectionProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<LabsConfig>(() => withLabsDefaults(profile?.labs));
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setConfig(withLabsDefaults(profile?.labs));
  }, [profile?.labs]);

  const patch = async (key: LabsFlag, value: boolean) => {
    const previous = config;
    const next = { ...config, [key]: value };
    setConfig(next);
    setSaving(true);
    try {
      await setLabsConfig(next);
      // The subtitle overlay is a separate window; it reads this flag from
      // shared localStorage instead of widening its IPC allowlist.
      writeLocalStorage(LABS_FAILURE_NOTICES_KEY, String(next.failureNotices));
      writeLocalStorage(LABS_OUTPUT_FIXES_KEY, String(next.outputFixes));
      onSaved();
    } catch {
      setConfig(previous);
      toast.error(t("settings.behaviorSaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="labs">
      <SettingsSectionHeader
        icon={<FlaskConical size={15} className="icon-accent" />}
        title={t("settings.labs")}
      />
      <div className="settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.labsHint")}</p>
        {LABS_ROWS.map(({ key, labelKey, hintKey }) => (
          <SettingsToggleRow
            key={key}
            label={t(labelKey)}
            hint={hintKey ? t(hintKey) : undefined}
            checked={config[key]}
            disabled={saving}
            onChange={(value) => { void patch(key, value); }}
          />
        ))}
      </div>
    </section>
  );
}
