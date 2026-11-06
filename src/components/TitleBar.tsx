import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { hideMainWindow } from "@/api/tauri";
import AppNavigation, { type AppView } from "./AppNavigation";

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
  view: AppView;
  onNavigate: (view: AppView) => void;
  updateAvailable?: boolean;
  updateInstalling?: boolean;
}

export default function TitleBar({
  title, view, onNavigate, updateAvailable = false, updateInstalling = false,
}: TitleBarProps) {
  const { t } = useTranslation();

  const startDrag = (e: React.MouseEvent) => {
    if (e.button !== 0) return;

    const target = e.target as HTMLElement;
    if (
      target.closest("button") ||
      target.closest("input") ||
      target.closest("select") ||
      target.closest("textarea") ||
      target.closest("a") ||
      target.closest("[data-no-drag]")
    ) {
      return;
    }

    e.preventDefault();
    getCurrentWindow().startDragging();
  };

  return (
    // Window dragging is a pointer-only desktop affordance; keyboard actions
    // remain on the real buttons rendered in the action slots.
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <header onMouseDown={startDrag} className="title-bar">
      <div className="title-bar-brand">
        <SpeakFlowBrandIcon height={14} className="title-bar-logo" />
        <span className="title-bar-name">SpeakFlow</span>
        <span className="title-bar-div" aria-hidden="true" />
        <span className="title-bar-doc">{title}</span>
      </div>

      <AppNavigation
        view={view}
        onNavigate={onNavigate}
        updateAvailable={updateAvailable}
        updateInstalling={updateInstalling}
      />

      <div className="title-bar-actions">
        <div className="window-controls">
          <button
            type="button"
            aria-label={t("common.minimize")}
            onClick={() => void getCurrentWindow().minimize()}
          >
            <Minus size={10} strokeWidth={1.1} />
          </button>
          <button
            type="button"
            aria-label={t("common.maximize")}
            onClick={() => { void getCurrentWindow().toggleMaximize?.().catch(() => undefined); }}
          >
            <Square size={9} strokeWidth={1.1} />
          </button>
          <button
            type="button"
            data-close
            aria-label={t("common.close")}
            onClick={() => void hideMainWindow()}
          >
            <X size={11} strokeWidth={1.1} />
          </button>
        </div>
      </div>
    </header>
  );
}
