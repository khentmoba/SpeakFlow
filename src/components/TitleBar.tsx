import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { hideMainWindow } from "@/api/tauri";
import { handleWindowDrag } from "@/hooks/useWindowDrag";

export function SpeakFlowBrandIcon({ height = 14, className }: { height?: number; className?: string }) {
  const width = Math.round(((height * 394) / 228) * 100) / 100;
  return (
    <svg
      width={width}
      height={height}
      viewBox="60 152 394 228"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
    >
      <path
        d="M 84 256 H 132 C 156 256 156 176 180 176 C 204 176 220 356 244 356 C 268 356 284 196 308 196 C 324 196 324 256 340 256"
        fill="none"
        stroke="currentColor"
        strokeWidth="48"
        strokeLinecap="square"
      />
      <rect x="410" y="156" width="44" height="200" fill="currentColor" />
    </svg>
  );
}

interface TitleBarProps {
  title: string;
}

export default function TitleBar({ title }: TitleBarProps) {
  const { t } = useTranslation();

  return (
    // Window dragging is a pointer-only desktop affordance; keyboard actions
    // remain on the real buttons rendered in the action slots.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <header onMouseDown={handleWindowDrag} className="title-bar">
      <div className="title-bar-brand">
        <h1 className="title-bar-doc">{title}</h1>
      </div>

      <div className="title-bar-actions">
        <div className="window-controls">
          <button
            type="button"
            aria-label={t("common.minimize")}
            onClick={() => {
              try {
                void getCurrentWindow().minimize();
              } catch {
                // non-Tauri preview fallback
              }
            }}
          >
            <Minus size={11} strokeWidth={1.2} />
          </button>
          <button
            type="button"
            aria-label={t("common.maximize")}
            onClick={() => {
              try {
                void getCurrentWindow().toggleMaximize?.().catch(() => undefined);
              } catch {
                // non-Tauri preview fallback
              }
            }}
          >
            <Square size={10} strokeWidth={1.2} />
          </button>
          <button
            type="button"
            data-close
            aria-label={t("common.close")}
            onClick={() => void hideMainWindow()}
          >
            <X size={12} strokeWidth={1.2} />
          </button>
        </div>
      </div>
    </header>
  );
}
