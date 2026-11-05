import {
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
  type UIEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { ExternalLink, MessageCircle, Send, X } from "lucide-react";
import GoogleSearchEntryPoint from "@/features/assistant/GoogleSearchEntryPoint";
import type { AssistantSource, ConversationMessage } from "@/pages/subtitle/overlayModel";

export interface SubtitleConversationProps {
  conversationListRef: RefObject<HTMLDivElement | null>;
  conversationInputRef: RefObject<HTMLTextAreaElement | null>;
  conversationMessages: ConversationMessage[];
  conversationBusy: boolean;
  conversationDraft: string;
  conversationError: string | null;
  conversationInput: string;
  assistantRequest: string;
  onCloseConversation: (event: MouseEvent<HTMLButtonElement>) => void;
  onConversationScroll: (event: UIEvent<HTMLDivElement>) => void;
  onConversationSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onConversationKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  onInputChange: (value: string) => void;
  onOpenAssistantUrl: (url: string) => void;
  onOpenSource: (event: MouseEvent<HTMLButtonElement>, source: AssistantSource) => void;
  renderAssistantContent: (content: string, streaming?: boolean) => ReactNode;
}

export function SubtitleConversation({
  conversationListRef,
  conversationInputRef,
  conversationMessages,
  conversationBusy,
  conversationDraft,
  conversationError,
  conversationInput,
  assistantRequest,
  onCloseConversation,
  onConversationScroll,
  onConversationSubmit,
  onConversationKeyDown,
  onInputChange,
  onOpenAssistantUrl,
  onOpenSource,
  renderAssistantContent,
}: SubtitleConversationProps) {
  const { t } = useTranslation();

  const renderSources = (sources: AssistantSource[] | undefined) => {
    if (!sources?.length) return null;
    return (
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
    );
  };

  return (
    <section
      className="subtitle-conversation"
      role="dialog"
      aria-label={t("subtitle.conversation.title")}
    >
      <header className="subtitle-conversation-header">
        <div className="subtitle-conversation-title">
          <MessageCircle size={15} aria-hidden="true" />
          <span>{t("subtitle.conversation.title")}</span>
        </div>
        <button
          type="button"
          className="subtitle-conversation-close"
          onClick={onCloseConversation}
          aria-label={t("subtitle.conversation.close")}
          title={t("subtitle.conversation.close")}
        >
          <X size={15} aria-hidden="true" />
        </button>
      </header>

      <div
        ref={conversationListRef}
        className="subtitle-conversation-messages"
        aria-label={t("subtitle.conversation.history")}
        role="log"
        aria-live="polite"
        aria-busy={conversationBusy}
        tabIndex={0}
        onScroll={onConversationScroll}
      >
        {conversationMessages.map((message) => (
          <article
            key={message.id}
            className={`subtitle-conversation-message is-${message.role}`}
          >
            <div className="subtitle-conversation-row">
              {message.role === "assistant" && (
                <span className="subtitle-conversation-assistant-mark" aria-hidden="true">
                  <MessageCircle size={12} />
                </span>
              )}
              <div className="subtitle-conversation-bubble">
                {message.role === "assistant" ? renderAssistantContent(message.content) : message.content}
              </div>
            </div>
            {message.role === "assistant" && renderSources(message.sources)}
            {message.role === "assistant" && message.googleSearchEntryPoint && (
              <GoogleSearchEntryPoint
                html={message.googleSearchEntryPoint}
                label="Google Search"
                onOpen={onOpenAssistantUrl}
              />
            )}
          </article>
        ))}
        {conversationBusy && (
          <article className="subtitle-conversation-message is-assistant is-streaming">
            <div className="subtitle-conversation-row">
              <span className="subtitle-conversation-assistant-mark" aria-hidden="true">
                <MessageCircle size={12} />
              </span>
              <div className="subtitle-conversation-bubble">
                {conversationDraft ? renderAssistantContent(conversationDraft, true) : t("subtitle.conversation.thinking")}
              </div>
            </div>
          </article>
        )}
      </div>

      {conversationError && (
        <div className="subtitle-conversation-error" role="status">{conversationError}</div>
      )}

      <form className="subtitle-conversation-composer" onSubmit={onConversationSubmit}>
        <textarea
          ref={conversationInputRef}
          value={conversationInput}
          onChange={(event) => onInputChange(event.target.value)}
          onKeyDown={onConversationKeyDown}
          placeholder={t("subtitle.conversation.placeholder")}
          aria-label={t("subtitle.conversation.placeholder")}
          rows={1}
          maxLength={6000}
          disabled={conversationBusy || !assistantRequest.trim()}
        />
        <button
          type="submit"
          className="subtitle-conversation-send"
          disabled={conversationBusy || !assistantRequest.trim() || !conversationInput.trim()}
          aria-label={t("subtitle.conversation.send")}
          title={t("subtitle.conversation.send")}
        >
          <Send size={15} aria-hidden="true" />
        </button>
      </form>
    </section>
  );
}
