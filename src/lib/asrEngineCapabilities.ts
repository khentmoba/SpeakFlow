import { CATALOG_MODELS, findCatalogModel, formatModelSize } from "@/lib/asrCatalog";

export type AsrEngineKey = string;

interface AsrEngineCapabilityBase {
  final: true;
  interim: boolean;
  /** Smart Vocabulary conditions recognition. Otherwise it only drives spelling correction afterwards. */
  vocabulary: boolean;
}

export type AsrEngineCapability = AsrEngineCapabilityBase & (
  | { execution: "local"; downloadSize: string }
  | { execution: "cloud"; downloadSize: null }
);

/** Engines that are not catalog models: one native local engine and two cloud services. */
export const NON_CATALOG_CAPABILITIES: Record<string, AsrEngineCapability> = {
  "confucius4-r2t2": { execution: "local", final: true, interim: true, downloadSize: "2.48 GB", vocabulary: true },
  "glm-asr": { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true },
  "alibaba-asr": { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true },
};

export const ASR_ENGINE_CAPABILITIES: Record<string, AsrEngineCapability> = {
  ...Object.fromEntries(
    CATALOG_MODELS.map((model) => [
      model.key,
      {
        execution: "local" as const,
        final: true as const,
        interim: true,
        downloadSize: formatModelSize(model.size),
        vocabulary: model.vocabulary,
      },
    ]),
  ),
  ...NON_CATALOG_CAPABILITIES,
};

export function getAsrEngineCapability(engine: string): AsrEngineCapability | null {
  return Object.prototype.hasOwnProperty.call(ASR_ENGINE_CAPABILITIES, engine)
    ? ASR_ENGINE_CAPABILITIES[engine]
    : null;
}

export function formatAsrEngineDescription(engine: string, description: string): string {
  const downloadSize = getAsrEngineCapability(engine)?.downloadSize;
  return downloadSize ? `${downloadSize} · ${description}` : description;
}

/** Catalog entry for a local engine, or null for R2T2 and cloud engines. */
export function catalogModelFor(engine: string) {
  return findCatalogModel(engine);
}
