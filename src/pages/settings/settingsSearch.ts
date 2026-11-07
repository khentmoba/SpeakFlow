/**
 * Settings search keywords. The nav filter matches the section label plus these
 * everyday words, so "mic" finds Microphone and "logs" finds Support.
 */
export const SETTINGS_SECTION_KEYWORDS: Record<string, readonly string[]> = {
  appearance: ["theme", "dark", "light", "system", "live transcript", "motion"],
  engine: ["model", "asr", "download", "language", "gpu", "speech engine", "vocabulary"],
  "ai-models": ["llm", "provider", "api key", "polish", "assistant", "decision model", "search"],
  hotkey: ["shortcut", "f2", "recording mode", "hold", "toggle", "keyboard"],
  microphone: ["mic", "input device", "level", "audio input"],
  input: ["paste", "typing", "send input", "clipboard", "insert"],
  "dictation-tools": ["snippets", "commands", "filler", "silence", "tidy", "cleanup"],
  "app-profiles": ["per app", "rules", "instructions", "translation", "profile"],
  "history-settings": ["history", "audio", "export", "retention", "save", "storage"],
  vocabulary: ["hotwords", "names", "terms", "spelling", "dictionary"],
  startup: ["autostart", "login", "tray", "launch", "minimized"],
  updates: ["update", "version", "release", "changelog"],
  support: ["logs", "diagnostics", "help", "issue", "report", "troubleshoot"],
};

/** Every whitespace-separated token must appear in the label, id or keywords. */
export function settingsSectionMatches(id: string, label: string, query: string): boolean {
  const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return true;
  const haystack = [id, label, ...(SETTINGS_SECTION_KEYWORDS[id] ?? [])]
    .join(" ")
    .toLowerCase();
  return tokens.every((token) => haystack.includes(token));
}
