import type {
  AudioBehaviorConfig,
  LabsConfig,
  MeetingsConfig,
  OutputBehaviorConfig,
  ShortcutsBehaviorConfig,
} from "@/types";

/**
 * Defaults mirror `AudioBehaviorConfig::default()` and friends in
 * `src-tauri/src/state/user_profile.rs`. The backend always sends a full
 * object, so these only fill gaps for older profiles or a failed load.
 */
export const DEFAULT_AUDIO_BEHAVIOR: AudioBehaviorConfig = {
  duckingMode: "off",
  duckVolume: 0.2,
  speakersOnly: false,
  avoidBluetoothMicrophone: true,
  releaseMicWhileIdle: false,
  preRollEnabled: true,
  adaptiveReleaseTail: true,
};

export const DEFAULT_OUTPUT_BEHAVIOR: OutputBehaviorConfig = {
  pasteWithShift: false,
  autoTerminalCtrlShiftV: false,
  keepOnClipboard: false,
  copyOnly: false,
};

export const DEFAULT_SHORTCUTS_BEHAVIOR: ShortcutsBehaviorConfig = {
  secondDictationHotkey: null,
  terminalPasteHotkey: null,
  typedTextHotkey: null,
  meetingHotkey: null,
  shareDictationShortcut: false,
};

export const DEFAULT_LABS: LabsConfig = {
  liveText: false,
  failureNotices: true,
  returnToApp: false,
  smartSpacing: false,
  spokenFormatting: false,
  spokenSymbols: false,
  skipPauses: false,
  tapOrHold: false,
  standaloneSpecialKeys: false,
  modelInPlace: false,
  pickerSortFilter: true,
  benchmarkDetails: false,
  outputFixes: false,
  apiProcessing: false,
  meetingFinishedEvent: false,
  meetingNotesVocabulary: true,
  expireMeetingAudio: false,
};

export const DEFAULT_MEETINGS: MeetingsConfig = {
  callDetectionEnabled: true,
  moreCallApps: false,
  appPolicies: {},
  autoSummarize: true,
  saveMarkdownTranscripts: false,
  markdownOutputDir: null,
  keepAudioDays: 30,
  transcriptFontSize: 13,
};

export function withAudioBehaviorDefaults(
  value?: Partial<AudioBehaviorConfig> | null,
): AudioBehaviorConfig {
  return { ...DEFAULT_AUDIO_BEHAVIOR, ...(value ?? {}) };
}

export function withOutputBehaviorDefaults(
  value?: Partial<OutputBehaviorConfig> | null,
): OutputBehaviorConfig {
  return { ...DEFAULT_OUTPUT_BEHAVIOR, ...(value ?? {}) };
}

export function withShortcutsBehaviorDefaults(
  value?: Partial<ShortcutsBehaviorConfig> | null,
): ShortcutsBehaviorConfig {
  return { ...DEFAULT_SHORTCUTS_BEHAVIOR, ...(value ?? {}) };
}

export function withLabsDefaults(value?: Partial<LabsConfig> | null): LabsConfig {
  return { ...DEFAULT_LABS, ...(value ?? {}) };
}

export function withMeetingsDefaults(value?: Partial<MeetingsConfig> | null): MeetingsConfig {
  return { ...DEFAULT_MEETINGS, ...(value ?? {}) };
}

/** The backend clamps to 0..1; keep the slider honest before the round trip. */
export function clampDuckVolume(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_AUDIO_BEHAVIOR.duckVolume;
  return Math.min(1, Math.max(0, value));
}

export function formatDuckVolume(value: number): string {
  return `${Math.round(clampDuckVolume(value) * 100)}%`;
}

/** Empty shortcut fields mean "disabled", never an empty accelerator string. */
export function normalizeHotkey(value: string | null | undefined): string | null {
  const trimmed = (value ?? "").trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** The backend clamps to 0..3650 days. */
export function clampKeepAudioDays(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_MEETINGS.keepAudioDays;
  return Math.min(3650, Math.max(0, Math.round(value)));
}

/** The backend clamps to 10..24 px. */
export function clampTranscriptFontSize(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_MEETINGS.transcriptFontSize;
  return Math.min(24, Math.max(10, Math.round(value)));
}

/** `12.4` -> `12.4s`, `95` -> `1:35`, `3725` -> `1:02:05`. */
export function formatMeetingDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(Number.isFinite(totalSeconds) ? totalSeconds : 0));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(rest).padStart(2, "0")}`;
  }
  if (minutes > 0) {
    return `${minutes}:${String(rest).padStart(2, "0")}`;
  }
  return `${rest}s`;
}

/** `1536` -> `1.5 KB`. Keeps model sizes readable in the picker. */
export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = value >= 10 || unit === 0 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${rounded} ${units[unit]}`;
}

/** Splits a comma-separated field into trimmed, non-empty entries. */
export function parseListField(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}
