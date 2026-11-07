import type { MouseEvent } from "react";
import { ExternalLink } from "lucide-react";
import { useTranslation } from "react-i18next";
import GoogleSearchEntryPoint from "@/features/assistant/GoogleSearchEntryPoint";
import type { AssistantSource } from "@/pages/subtitle/overlayModel";

export interface SubtitleAssistantResultProps {
  sources: AssistantSource[];
  googleSearchEntryPoint: string;
  searchQuery: string;
  searchProvider: string | null;
  searchElapsedMs: number | null;
  elapsedMs: number | null;
  searchError: string | null;
  retryBusy: boolean;
  onOpenSource: (event: MouseEvent<HTMLButtonElement>, source: AssistantSource) => void;
  onOpenUrl: (url: string) => void;
  onRetry: (event: MouseEvent<HTMLButtonElement>) => void;
}

/**
 * Everything under an assistant answer: sources, the Google entry point, search
 * timing and the retry notice. Split out so the overlay's render stays about
 * recording state.
 */
export function SubtitleAssistantResult({
  sources,
  googleSearchEntryPoint,
  searchQuery,
  searchProvider,
  searchElapsedMs,
  elapsedMs,
  searchError,
  retryBusy,
  onOpenSource,
  onOpenUrl,
  onRetry,
}: SubtitleAssistantResultProps) {
  const { t } = useTranslation();

  return (
    <>
      {sources.length > 0 && (
        <div className="subtitle-source-list" aria-label={t("subtitle.conversation.sources")}>
          {sources.map((source, index) => (
            <button
              key={`${source.url}-${index}`}
              type="button"
              className="subtitle-source-link"
              title={source.url}
              onClick={(event) => onOpenSource(event, source)}
            >
              <span>{source.title || source.url}</span>
              <ExternalLink size={11} aria-hidden="true" />
            </button>
          ))}
        </div>
      )}
      {googleSearchEntryPoint && (
        <GoogleSearchEntryPoint
          html={googleSearchEntryPoint}
          label="Google Search"
          onOpen={onOpenUrl}
        />
      )}
      {(searchQuery || elapsedMs !== null) && (
        <div className="subtitle-search-meta" role="status">
          {searchQuery && (
            <span className="subtitle-search-query" title={searchQuery}>{searchQuery}</span>
          )}
          {searchProvider && (
            <span>{t("subtitle.conversation.searchProvider", { provider: searchProvider })}</span>
          )}
          {searchElapsedMs !== null && (
            <span>{t("subtitle.conversation.searchTiming", { ms: searchElapsedMs })}</span>
          )}
          {elapsedMs !== null && (
            <span>{t("subtitle.conversation.totalTiming", { ms: elapsedMs })}</span>
          )}
        </div>
      )}
      {searchError && (
        <div className="subtitle-search-warning" role="status">
          <span>{searchError}</span>
          <button type="button" onClick={onRetry} disabled={retryBusy}>
            {retryBusy ? t("subtitle.conversation.retrying") : t("common.retry")}
          </button>
        </div>
      )}
    </>
  );
}
