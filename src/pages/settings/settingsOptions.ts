import { ClipboardPaste, Keyboard } from "lucide-react";
import { getAsrEngineCapability } from "@/lib/asrEngineCapabilities";

const ONLINE_ENGINES: ReadonlySet<string> = new Set(["glm-asr", "alibaba-asr"]);
export const isOnlineEngineKey = (engine: string) => ONLINE_ENGINES.has(engine);

export const asrCapabilityTagKeys = (engineKey: string) => {
  const capability = getAsrEngineCapability(engineKey);
  if (!capability) return [];
  return [
    capability.execution === "local" ? "settings.capabilityLocal" : "settings.capabilityCloud",
    capability.interim ? "settings.capabilityFinalInterim" : "settings.capabilityFinalOnly",
  ];
};

export const inputOptions = [
  { key: "sendInput" as const, icon: Keyboard, labelKey: "settings.directInput", descKey: "settings.directInputDesc" },
  { key: "clipboard" as const, icon: ClipboardPaste, labelKey: "settings.clipboardPaste", descKey: "settings.clipboardPasteDesc" },
];

export const recordingModeOptions: Array<{
  key: "hold" | "toggle";
  labelKey: string;
  descKey: string;
}> = [
  { key: "hold", labelKey: "settings.holdToTalk", descKey: "settings.holdToTalkDesc" },
  { key: "toggle", labelKey: "settings.toggleMode", descKey: "settings.toggleModeDesc" },
];

export const sourceLabels: Record<string, string> = {
  user: "settings.sourceManual",
  learned: "settings.sourceLearned",
};

export const sourceColors: Record<string, string> = {
  user: "var(--color-accent)",
  learned: "var(--color-learned)",
};

export function findRecordingModeOption(mode: "hold" | "toggle") {
  return recordingModeOptions.find((option) => option.key === mode) ?? recordingModeOptions[0];
}
