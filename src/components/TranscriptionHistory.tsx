import i18n from "@/i18n";
import type { HistoryItem } from "@/types";

interface TranscriptionHistoryProps {
  history: HistoryItem[];
  currentResult: string | null;
  copiedId: string | null;
  onCopy: (text: string, id: string) => void;
}

const clockSvg = Object.freeze(
  <span className="history-item-clock lucide lucide-clock" aria-hidden="true" />,
);

const checkSvg = Object.freeze(
  <span className="animate-check-draw lucide lucide-check" aria-hidden="true" />,
);

const copySvg = Object.freeze(
  <span className="lucide lucide-copy" aria-hidden="true" />,
);

const dotSpan = Object.freeze(<span className="history-item-dot" aria-hidden="true" />);

const ANIMATION_DELAYS = Array.from({ length: 64 }, (_, i) =>
  Object.freeze({ animationDelay: `${i * 50}ms` }));

function getAnimationDelay(idx: number): { animationDelay: string } {
  return idx < ANIMATION_DELAYS.length
    ? ANIMATION_DELAYS[idx]
    : { animationDelay: `${idx * 50}ms` };
}

let currentCopyLabel = i18n.t("common.copy");
if (typeof i18n.on === "function") {
  i18n.on("languageChanged", () => {
    currentCopyLabel = i18n.t("common.copy");
  });
}

export default function TranscriptionHistory({
  history,
  currentResult,
  copiedId,
  onCopy,
}: TranscriptionHistoryProps) {
  if (history.length === 0) return null;

  const items = currentResult && history.length > 0 && history[0].text === currentResult
    ? history.slice(1)
    : history;

  if (items.length === 0) return null;

  return (
    <div className="history-list">
      {items.map((item, idx) => {
        const isCopied = copiedId === item.id;
        return (
          <div
            key={item.id}
            className="history-item"
            data-history-id={item.id}
            style={getAnimationDelay(idx)}
          >
            {dotSpan}
            <div className="history-item-body">
              <p className="history-item-text">{item.text}</p>
              <span className="history-item-time">
                {clockSvg}
                {item.timeDisplay}
              </span>
            </div>
            <button
              aria-label={currentCopyLabel}
              className="icon-btn icon-btn-sm"
              onClick={() => onCopy(item.text, item.id)}
            >
              {isCopied ? checkSvg : copySvg}
            </button>
          </div>
        );
      })}
    </div>
  );
}
