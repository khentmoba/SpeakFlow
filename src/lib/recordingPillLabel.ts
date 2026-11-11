/**
 * i18n key for the recording pill label. Assistant recordings keep their own
 * wording, because the subtitle hint that used to say "AI listening" is gone.
 */
export function pillLabelKey(phase: string, mode?: string | null): string {
  if (phase === "recording" && mode === "assistant") return "pill.assistantRecording";
  return `pill.${phase}`;
}
