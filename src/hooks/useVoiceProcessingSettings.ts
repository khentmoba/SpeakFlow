import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  getVoiceProcessing,
  setAsrLanguage,
  setRemoveFillers,
  setSilenceFilter,
  setTranslateToEnglish,
} from "@/api/tauri";

/**
 * Recognition post-processing settings. Each change applies optimistically and
 * rolls back with a toast if the backend rejects it.
 */
export function useVoiceProcessingSettings() {
  const { t } = useTranslation();
  const [silenceFilter, setSilenceFilterState] = useState(true);
  const [removeFillers, setRemoveFillersState] = useState(true);
  const [asrLanguage, setAsrLanguageState] = useState<string | null>(null);
  const [translateToEnglish, setTranslateToEnglishState] = useState(false);
  const [languageSaving, setLanguageSaving] = useState(false);

  useEffect(() => {
    let disposed = false;
    getVoiceProcessing()
      .then((settings) => {
        if (disposed || !settings) return;
        setSilenceFilterState(settings.silenceFilter !== false);
        setRemoveFillersState(settings.removeFillers !== false);
        setAsrLanguageState(settings.language ?? null);
        setTranslateToEnglishState(settings.translateToEnglish === true);
      })
      .catch(() => {});
    return () => { disposed = true; };
  }, []);

  const handleSilenceFilterToggle = useCallback(async (enabled: boolean) => {
    setSilenceFilterState(enabled);
    try {
      await setSilenceFilter(enabled);
    } catch {
      setSilenceFilterState(!enabled);
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [t]);

  const handleRemoveFillersToggle = useCallback(async (enabled: boolean) => {
    setRemoveFillersState(enabled);
    try {
      await setRemoveFillers(enabled);
    } catch {
      setRemoveFillersState(!enabled);
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [t]);

  const handleLanguageChange = useCallback(async (language: string | null) => {
    if (languageSaving) return;
    setLanguageSaving(true);
    const previous = asrLanguage;
    setAsrLanguageState(language);
    try {
      const saved = await setAsrLanguage(language);
      setAsrLanguageState(saved);
    } catch {
      setAsrLanguageState(previous);
      toast.error(t("toast.settingSaveFailed"));
    } finally {
      setLanguageSaving(false);
    }
  }, [asrLanguage, languageSaving, t]);

  const handleTranslateToEnglishToggle = useCallback(async (enabled: boolean) => {
    setTranslateToEnglishState(enabled);
    try {
      await setTranslateToEnglish(enabled);
    } catch {
      setTranslateToEnglishState(!enabled);
      toast.error(t("toast.settingSaveFailed"));
    }
  }, [t]);

  return {
    asrLanguage,
    handleLanguageChange,
    handleRemoveFillersToggle,
    handleSilenceFilterToggle,
    handleTranslateToEnglishToggle,
    languageSaving,
    removeFillers,
    silenceFilter,
    translateToEnglish,
  };
}
