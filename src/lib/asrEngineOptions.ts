import { AudioWaveform, Cloud, FileAudio, Globe, Languages, Layers, Mic, Sparkles, Waves, Zap } from "lucide-react";
import { CATALOG_MODELS, findCatalogModel } from "@/lib/asrCatalog";

type EngineIcon = typeof Mic;

export interface EngineOption {
  key: string;
  icon: EngineIcon;
  label: string;
  labelKey?: string;
  descKey?: string;
  /** Catalog description, used for every model in `asr_catalog.json`. */
  description?: string;
  recommended?: boolean;
  /** Local model served by the bundled engine. */
  local?: boolean;
}

const ICONS: Record<string, EngineIcon> = {
  "qwen3-asr-1.7b": Layers,
  "qwen3-asr-0.6b": Zap,
  "parakeet-tdt-0.6b-v2": Mic,
  "parakeet-unified-en-0.6b": Languages,
  "canary-qwen-2.5b": Sparkles,
  "cohere-transcribe-03-2026": Waves,
};

function catalogIcon(key: string): EngineIcon {
  if (ICONS[key]) return ICONS[key];
  if (key.startsWith("whisper")) return FileAudio;
  if (key.startsWith("qwen")) return Layers;
  if (key.startsWith("parakeet") || key.startsWith("nemotron") || key.startsWith("multitalker")) return Mic;
  if (key.startsWith("moonshine") || key.startsWith("gigaam")) return Zap;
  if (key.startsWith("canary") || key.startsWith("granite")) return Sparkles;
  if (key.startsWith("voxtral") || key.startsWith("cohere")) return Waves;
  return AudioWaveform;
}

const catalogOptions: EngineOption[] = CATALOG_MODELS.map((model) => ({
  key: model.key,
  icon: catalogIcon(model.key),
  label: model.name,
  description: model.description,
  recommended: model.recommended,
  local: true,
}));

export const NON_CATALOG_OPTIONS: EngineOption[] = [
  {
    key: "confucius4-r2t2",
    icon: AudioWaveform,
    label: "Confucius4-R2T2 Q8",
    descKey: "settings.r2t2Desc",
    local: true,
  },
  { key: "glm-asr", icon: Globe, label: "GLM-ASR", descKey: "settings.glmAsrDesc" },
  {
    key: "alibaba-asr",
    icon: Cloud,
    label: "Alibaba DashScope",
    labelKey: "settings.alibabaAsrLabel",
    descKey: "settings.alibabaAsrDesc",
  },
];

/** Every selectable engine: catalog models first, then R2T2 and the cloud services. */
export const engineOptions: readonly EngineOption[] = [...catalogOptions, ...NON_CATALOG_OPTIONS];

const optionsByKey = new Map(engineOptions.map((option) => [option.key, option]));

export function findEngineOption(key: string): EngineOption | undefined {
  return optionsByKey.get(key);
}

export function engineOptionLabel(
  option: EngineOption,
  translate: (key: string) => string,
) {
  return option.labelKey ? translate(option.labelKey) : option.label;
}

/** Display name for any engine key, including ones this build does not offer. */
export function engineDisplayLabel(key: string, translate: (key: string) => string): string {
  const option = findEngineOption(key);
  if (option) return engineOptionLabel(option, translate);
  const model = findCatalogModel(key);
  return model ? model.name : key;
}

/** One-line description for an engine, from the catalog or the interface strings. */
export function engineDisplayDescription(key: string, translate: (key: string) => string): string {
  const option = findEngineOption(key);
  if (option?.description) return option.description;
  return option?.descKey ? translate(option.descKey) : "";
}
