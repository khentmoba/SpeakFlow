export type AsrEngineKey =
  | "qwen3-asr-0.6b"
  | "confucius4-r2t2"
  | "qwen3-asr-1.7b"
  | "parakeet-unified-en-0.6b"
  | "parakeet-tdt-0.6b-v2"
  | "canary-qwen-2.5b"
  | "cohere-transcribe-03-2026"
  | "glm-asr"
  | "alibaba-asr";

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

export const ASR_ENGINE_CAPABILITIES: Record<AsrEngineKey, AsrEngineCapability> = {
  "qwen3-asr-0.6b": { execution: "local", final: true, interim: true, downloadSize: "850 MB", vocabulary: true },
  "confucius4-r2t2": { execution: "local", final: true, interim: true, downloadSize: "2.48 GB", vocabulary: true },
  "qwen3-asr-1.7b": { execution: "local", final: true, interim: true, downloadSize: "2.19 GB", vocabulary: true },
  "parakeet-unified-en-0.6b": { execution: "local", final: true, interim: true, downloadSize: "731 MB", vocabulary: false },
  "parakeet-tdt-0.6b-v2": { execution: "local", final: true, interim: true, downloadSize: "730 MB", vocabulary: false },
  "canary-qwen-2.5b": { execution: "local", final: true, interim: true, downloadSize: "2.80 GB", vocabulary: false },
  "cohere-transcribe-03-2026": { execution: "local", final: true, interim: true, downloadSize: "2.41 GB", vocabulary: false },
  "glm-asr": { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true },
  "alibaba-asr": { execution: "cloud", final: true, interim: false, downloadSize: null, vocabulary: true },
};

export function getAsrEngineCapability(engine: string): AsrEngineCapability | null {
  return Object.prototype.hasOwnProperty.call(ASR_ENGINE_CAPABILITIES, engine)
    ? ASR_ENGINE_CAPABILITIES[engine as AsrEngineKey]
    : null;
}

export function formatAsrEngineDescription(engine: string, description: string): string {
  const downloadSize = getAsrEngineCapability(engine)?.downloadSize;
  return downloadSize ? `${downloadSize} · ${description}` : description;
}
