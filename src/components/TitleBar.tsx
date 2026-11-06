import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { hideMainWindow } from "@/api/tauri";
import AppNavigation, { type AppView } from "./AppNavigation";

export function SpeakFlowBrandIcon({ size = 15, className }: { size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 512 512"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
    >
      <g transform="translate(-10 -20)">
        <path
          transform="translate(512 0) scale(-1 1)"
          d="M 166 220 C 156 176, 194 144, 256 144 C 324 144, 360 178, 352 220 C 344 252, 310 266, 256 276 C 202 286, 168 300, 160 332 C 152 374, 188 408, 256 408 C 310 408, 344 382, 350 344"
          fill="none"
          stroke="currentColor"
          strokeWidth="56"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <path
          transform="translate(512 0) scale(-1 1)"
          d="M 164 216 Q 140 236 116 244 Q 142 246 170 230 Z"
          fill="currentColor"
        />
        <rect x="414" y="296" width="22" height="64" rx="11" fill="currentColor" />
      </g>
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
        <SpeakFlowBrandIcon size={14} className="title-bar-logo" />
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
