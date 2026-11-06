import { AudioLines, History, Settings, RefreshCw } from "lucide-react";
export type AppView = "main" | "history" | "settings" | "updates";

const destinations = [
  { view: "main", label: "Dictation", icon: AudioLines },
  { view: "history", label: "History", icon: History },
  { view: "settings", label: "Settings", icon: Settings },
  { view: "updates", label: "Updates", icon: RefreshCw },
] as const;

export default function AppNavigation({ view, onNavigate, updateAvailable = false, updateInstalling = false }: {
  view: AppView;
  onNavigate: (view: AppView) => void;
  updateAvailable?: boolean;
  updateInstalling?: boolean;
}) {
  const updateStatus = updateInstalling ? "Update in progress" : "New release available";

  return (
    <nav className="app-navigation" aria-label="Main navigation">
      {destinations.map(({ view: destination, label, icon: Icon }) => (
        <button
          key={destination}
          type="button"
          className="app-nav-item"
          data-destination={destination}
          aria-label={destination === "updates" && updateAvailable ? `${label}, ${updateStatus}` : label}
          aria-current={view === destination ? "page" : undefined}
          onClick={() => onNavigate(destination)}
        >
          <Icon size={12} strokeWidth={1.9} aria-hidden="true" className={destination === "updates" && updateInstalling ? "update-spinner" : undefined} />
          <span>{label}</span>
          {destination === "updates" && updateAvailable && (
            <span className="update-nav-dot" role="img" aria-label={updateStatus} />
          )}
        </button>
      ))}
    </nav>
  );
}
