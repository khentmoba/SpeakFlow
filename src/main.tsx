import React, { Suspense, lazy, useState, useRef, useCallback, useEffect } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "sonner";
import { CircleCheck, CircleAlert, Info, TriangleAlert, LoaderCircle } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RecordingProvider } from "./contexts/RecordingContext";
import { prefersReducedMotion } from "./lib/motion";
import MainPage from "./pages/MainPage";
import TitleBar, { SpeakFlowBrandIcon } from "./components/TitleBar";
import AppNavigation from "./components/AppNavigation";
import SidebarFooter from "./components/SidebarFooter";
import { handleWindowDrag } from "./hooks/useWindowDrag";
import { AppUpdateProvider, useAppUpdates } from "./contexts/AppUpdateContext";
import { useTheme } from "./hooks/useTheme";
import { useProfileSaveNotifications } from "./hooks/useProfileSaveNotifications";
import i18n from "./i18n";
import "./styles/theme.css";
import "./styles/pages.css";
import "./styles/workspace.css";
import "./styles/signal.css";

type View = "main" | "settings" | "history" | "updates" | "meetings";
const loadUpdatesPage = () => import("./pages/UpdatesPage");
const UpdatesPage = lazy(loadUpdatesPage);
const loadSettingsPage = () => import("./pages/SettingsPage");
const loadHistoryPage = () => import("./pages/HistoryPage");
const loadMeetingsPage = () => import("./pages/MeetingsPage");
const SettingsPage = lazy(loadSettingsPage);
const HistoryPage = lazy(loadHistoryPage);
const MeetingsPage = lazy(loadMeetingsPage);

class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; errorMessage: string | null }
> {
  constructor(props: { children: React.ReactNode }) {
    super(props);
    this.state = { hasError: false, errorMessage: null };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, errorMessage: error?.message || null };
  }
  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("Uncaught render error:", error, info);
  }
  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: 24, textAlign: "center", color: "var(--color-text-secondary)" }}>
          <p>{i18n.t("app.errorRestart")}</p>
          {this.state.errorMessage && (
            <p style={{ fontSize: 12, color: "var(--color-text-tertiary)", marginTop: 8 }}>
              {this.state.errorMessage}
            </p>
          )}
          <button onClick={() => this.setState({ hasError: false, errorMessage: null })} style={{ marginTop: 8, padding: "6px 16px", cursor: "pointer" }}>
            {i18n.t("common.retry")}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

export function App() {
  return <AppUpdateProvider><AppWorkspace /></AppUpdateProvider>;
}

function AppWorkspace() {
  const { latestAvailableVersion, updateInstalling } = useAppUpdates();
  const [view, setView] = useState<View>("main");
  const [animClass, setAnimClass] = useState("");
  const isTransitioning = useRef(false);
  useTheme();
  useProfileSaveNotifications();

  useEffect(() => {
    const preloadTimer = window.setTimeout(() => {
      void loadSettingsPage().catch((error) => {
        console.error("Settings page preload failed:", error);
      });
      void loadHistoryPage().catch((error) => {
        console.error("History page preload failed:", error);
      });
    }, 400);

    return () => window.clearTimeout(preloadTimer);
  }, []);

  // Page navigation with directional slide while only mounting the active page
  const navigateTo = useCallback((target: View) => {
    if (isTransitioning.current || target === view) return;
    // Commit an active transcript edit before any navigation source unmounts it.
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
    isTransitioning.current = true;

    const startTransition = () => {
      if (prefersReducedMotion()) {
        setAnimClass("");
        setView(target);
        isTransitioning.current = false;
        return;
      }

      const movingAwayFromMain = view === "main" && target !== "main";
      const exitClass = movingAwayFromMain ? "page-exit-left" : "page-exit-right";
      const enterClass = movingAwayFromMain ? "page-enter-right" : "page-enter-left";

      setAnimClass(exitClass);
      setTimeout(() => {
        setView(target);
        setAnimClass(enterClass);
        setTimeout(() => {
          setAnimClass("");
          isTransitioning.current = false;
        }, 180);
      }, 140);
    };

    if (target !== "main") {
      const loadTarget = target === "settings"
        ? loadSettingsPage
        : target === "updates"
          ? loadUpdatesPage
          : target === "meetings"
            ? loadMeetingsPage
            : loadHistoryPage;
      void loadTarget().then(startTransition).catch((error) => {
        isTransitioning.current = false;
        console.error(`${target} page load failed:`, error);
      });
      return;
    }

    startTransition();
  }, [view]);

  return (
    <RecordingProvider>
      <div className="app-shell">
        <a className="skip-link" href="#main-content">{i18n.t("app.skipToContent")}</a>
        <aside className="app-sidebar">
          {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
          <div className="app-sidebar-header" onMouseDown={handleWindowDrag}>
            <SpeakFlowBrandIcon height={16} className="app-sidebar-logo" />
            <span className="app-sidebar-name">SpeakFlow</span>
          </div>
          <AppNavigation
            view={view}
            onNavigate={navigateTo}
            updateAvailable={Boolean(latestAvailableVersion)}
            updateInstalling={updateInstalling}
          />
          <SidebarFooter
            onNavigate={navigateTo}
            updateAvailable={Boolean(latestAvailableVersion)}
            latestVersion={latestAvailableVersion}
          />
        </aside>
        <div className="app-workspace" id="main-content" tabIndex={-1}>
          <TitleBar title={i18n.t(`viewTitle.${view}`)} />
          <Suspense fallback={<div className="workspace-loading" role="status">Loading workspace…</div>}>
            {view === "main" ? (
              <MainPage onNavigate={navigateTo} animClass={animClass} />
            ) : view === "settings" ? (
              <SettingsPage onNavigate={navigateTo} active animClass={animClass} />
            ) : view === "updates" ? (
              <UpdatesPage animClass={animClass} />
            ) : view === "meetings" ? (
              <MeetingsPage animClass={animClass} />
            ) : (
              <HistoryPage onNavigate={navigateTo} animClass={animClass} />
            )}
          </Suspense>
        </div>
      </div>
      <Toaster
        className="app-toaster"
        position="bottom-right"
        offset={14}
        mobileOffset={14}
        icons={{
          success: <CircleCheck size={18} aria-hidden="true" />,
          error: <CircleAlert size={18} aria-hidden="true" />,
          info: <Info size={18} aria-hidden="true" />,
          warning: <TriangleAlert size={18} aria-hidden="true" />,
          loading: <LoaderCircle size={18} className="animate-spin" aria-hidden="true" />,
        }}
        toastOptions={{
          className: "app-toast",
          duration: 1800,
        }}
      />
    </RecordingProvider>
  );
}

const windowLabel = getCurrentWindow().label;

if (windowLabel === "subtitle") {
  import("./pages/SubtitleOverlay")
    .then(({ default: SubtitleOverlay }) => {
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <SubtitleOverlay />
        </React.StrictMode>
      );
    })
    .catch((error) => {
      console.error("Subtitle window load failed:", error);
    });
} else if (windowLabel === "selection-toolbar") {
  import("./pages/SelectionOverlay")
    .then(({ default: SelectionOverlay }) => {
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <SelectionOverlay />
        </React.StrictMode>
      );
    })
    .catch((error) => {
      console.error("Selection window load failed:", error);
    });
} else if (windowLabel === "recording-pill") {
  import("./pages/RecordingPill")
    .then(({ default: RecordingPill }) => {
      ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
          <RecordingPill />
        </React.StrictMode>
      );
    })
    .catch((error) => {
      console.error("Recording pill load failed:", error);
    });
} else {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </React.StrictMode>
  );
}
