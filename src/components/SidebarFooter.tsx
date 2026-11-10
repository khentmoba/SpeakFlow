import { useSidebarStatus } from "@/hooks/useSidebarStatus";
import Kbd from "./Kbd";
import type { AppView } from "./AppNavigation";

interface SidebarFooterProps {
  onNavigate: (view: AppView) => void;
  updateAvailable?: boolean;
  latestVersion?: string | null;
}

export default function SidebarFooter({
  onNavigate,
  updateAvailable = false,
  latestVersion = null,
}: SidebarFooterProps) {
  const { version, state, label, hotkeyDisplay, updateLabel, statusBarLabel } =
    useSidebarStatus(updateAvailable, latestVersion);

  return (
    <footer className="app-sidebar-footer" aria-label={statusBarLabel}>
      <div className="app-sidebar-status">
        <span className="status-dot" data-state={state} aria-hidden="true" />
        <span className="app-sidebar-status-text">{label}</span>
        {hotkeyDisplay && <Kbd combo={hotkeyDisplay} className="app-sidebar-key" />}
      </div>
      <div className="app-sidebar-meta">
        {version && <span className="app-sidebar-version">v{version}</span>}
        <button
          type="button"
          className="app-sidebar-link"
          onClick={() => onNavigate("updates")}
        >
          {updateLabel}
        </button>
      </div>
    </footer>
  );
}
