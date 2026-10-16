import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import brand from "../brand.json";
import { AccountPanel } from "./account/AccountPanel";
import { AgentSection } from "./agent/AgentSection";
import { DictationPanel } from "./dictation/DictationPanel";
import { GalaxyPanel } from "./galaxy/GalaxyPanel";
import { KnowledgeBasePanel } from "./knowledge/KnowledgeBasePanel";
import { disableAutostart, enableAutostart, isAutostartEnabled } from "./lib/autostart";
import { useFormFieldDictation } from "./lib/dictationInsert";
import { loadTheme, saveTheme, type Theme } from "./lib/theme";
import { MindMapPanel } from "./mindmap/MindMapPanel";
import { NotesWorkspace } from "./notes/NotesWorkspace";
import { Onboarding, onboardingComplete } from "./onboarding/Onboarding";
import { SearchPanel } from "./search/SearchPanel";
import { ConfirmDialog, Modal } from "./ui/dialogs";
import { PaneErrorBoundary } from "./ui/PaneErrorBoundary";
import {
  AccountIcon,
  AgentIcon,
  ChevronDownIcon,
  DictationIcon,
  GalaxyIcon,
  KnowledgeIcon,
  LabsIcon,
  LockIcon,
  MindMapIcon,
  NotesIcon,
  SearchIcon,
  ThemeIcon,
} from "./ui/icons";
import { Logo } from "./ui/Logo";

type Tab =
  | "notes"
  | "agent"
  | "search"
  | "galaxy"
  | "knowledge"
  | "mindmap"
  | "dictation"
  | "account";

type NavItem = { id: Tab; label: string; icon: (p: { className?: string }) => React.JSX.Element };

// The default surface is the five everyday pillars; the experimental visual
// surfaces (Galaxy, Mind Map) live behind "Labs" so a first-time user sees a
// short, legible sidebar.
const MAIN_NAV: NavItem[] = [
  { id: "notes", label: "Notes", icon: NotesIcon },
  { id: "agent", label: "Agent", icon: AgentIcon },
  { id: "dictation", label: "Dictation", icon: DictationIcon },
  { id: "search", label: "Search", icon: SearchIcon },
  { id: "knowledge", label: "Knowledge", icon: KnowledgeIcon },
];

const LABS_NAV: NavItem[] = [
  { id: "galaxy", label: "Galaxy", icon: GalaxyIcon },
  { id: "mindmap", label: "Mind Map", icon: MindMapIcon },
];

/**
 * Main-window shell: a tinted sidebar of pillars over a warm ground, with each
 * screen laid out as its own inset panel in the padded main region.
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");
  const [onboarded, setOnboarded] = useState(onboardingComplete);
  const [theme, setThemeState] = useState<Theme>(loadTheme);
  const [autostart, setAutostart] = useState(false);
  const [autostartPrompt, setAutostartPrompt] = useState(false);
  const [pendingNote, setPendingNote] = useState<string | null>(null);
  const [labsOpen, setLabsOpen] = useState(() => localStorage.getItem("arya-labs-open") === "true");
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [cheatSheetOpen, setCheatSheetOpen] = useState(false);
  const gTimer = useRef<number | null>(null);

  const clearPendingNote = useCallback(() => setPendingNote(null), []);

  const toggleLabs = () => {
    const next = !labsOpen;
    setLabsOpen(next);
    localStorage.setItem("arya-labs-open", String(next));
  };

  // Keyboard map: `?` opens the cheat sheet; a `g` prefix then a letter jumps
  // straight to a surface (g n = Notes, g d = Dictation, …). Ignored while
  // typing in a field so normal input is never swallowed.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const target = e.target as HTMLElement | null;
      const typing =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.tagName === "SELECT" ||
        target?.isContentEditable;
      if (typing) return;

      if (e.key === "?") {
        e.preventDefault();
        setCheatSheetOpen((v) => !v);
        return;
      }
      if (e.key.toLowerCase() === "g") {
        if (gTimer.current !== null) window.clearTimeout(gTimer.current);
        gTimer.current = window.setTimeout(() => {
          gTimer.current = null;
        }, 800);
        return;
      }
      if (gTimer.current !== null) {
        window.clearTimeout(gTimer.current);
        gTimer.current = null;
        const jump: Record<string, Tab> = {
          n: "notes",
          d: "dictation",
          a: "agent",
          s: "search",
          k: "knowledge",
          g: "galaxy",
          m: "mindmap",
        };
        const to = jump[e.key.toLowerCase()];
        if (to) {
          e.preventDefault();
          setTab(to);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // In-app dictation lands in whatever form field is focused (title, composer,
  // search); the note body is handled by BlockEditor itself.
  useFormFieldDictation();

  const setTheme = (next: Theme) => {
    saveTheme(next);
    setThemeState(next);
  };

  const toggleAutostart = async (next: boolean) => {
    // Reflect the real state; if the OS call fails, a later read corrects it.
    try {
      await (next ? enableAutostart() : disableAutostart());
      setAutostart(next);
    } catch {
      void isAutostartEnabled()
        .then(setAutostart)
        .catch(() => {});
    }
    localStorage.setItem("arya-autostart-decided", "true");
  };

  // Cross-surface deep-links (e.g. Galaxy's "Open"): switch to the owning tab,
  // and hand a note request to the workspace so it opens even on fresh mount.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<{ kind: string; id: string }>).detail;
      if (!detail) return;
      if (detail.kind === "note") {
        setPendingNote(detail.id);
        setTab("notes");
      } else if (detail.kind === "dictation") {
        setTab("dictation");
      } else if (detail.kind === "mindmap") {
        setTab("mindmap");
      } else if (detail.kind === "agent") {
        setTab("agent");
      }
    };
    window.addEventListener("arya:open-node", handler);
    return () => window.removeEventListener("arya:open-node", handler);
  }, []);

  useEffect(() => {
    const unlisten = listen("tray:new-session", () => setTab("agent"));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    // Reflect the current launch-at-login state, and offer it once on first run.
    void isAutostartEnabled()
      .then((on) => {
        setAutostart(on);
        if (!on && localStorage.getItem("arya-autostart-decided") !== "true") {
          setAutostartPrompt(true);
        }
      })
      .catch(() => {});
  }, []);

  // Tabs are lazily mounted on first visit and kept mounted (hidden) after, so
  // switching tabs preserves each workspace's state without paying for every
  // panel's data fetch + render on startup.
  const [mountedTabs, setMountedTabs] = useState<Partial<Record<Tab, boolean>>>({ notes: true });
  useEffect(() => {
    setMountedTabs((m) => (m[tab] ? m : { ...m, [tab]: true }));
  }, [tab]);

  if (!onboarded) {
    return <Onboarding onFinish={() => setOnboarded(true)} />;
  }

  const panels: { id: Tab; name: string; node: React.JSX.Element }[] = [
    {
      id: "notes",
      name: "Notes",
      node: <NotesWorkspace openNoteId={pendingNote} onOpenConsumed={clearPendingNote} />,
    },
    { id: "agent", name: "Agent", node: <AgentSection /> },
    { id: "search", name: "Search", node: <SearchPanel /> },
    { id: "galaxy", name: "Galaxy", node: <GalaxyPanel /> },
    { id: "knowledge", name: "Knowledge", node: <KnowledgeBasePanel /> },
    { id: "mindmap", name: "Mind Map", node: <MindMapPanel /> },
    { id: "dictation", name: "Dictation", node: <DictationPanel /> },
    { id: "account", name: "Account", node: <AccountPanel /> },
  ];

  return (
    <>
      <div className="app">
        <aside className="sidebar">
          <div className="sidebar-brand">
            <span className="brand-logo" aria-hidden="true">
              <Logo size={24} />
            </span>
            <span className="brand-name">{brand.name}</span>
          </div>

          {MAIN_NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              className="nav-item"
              aria-current={tab === id ? "page" : undefined}
              onClick={() => setTab(id)}
            >
              <Icon />
              <span className="nav-label">{label}</span>
            </button>
          ))}

          <button
            type="button"
            className="nav-item labs-toggle"
            aria-expanded={labsOpen}
            onClick={toggleLabs}
          >
            <LabsIcon />
            <span className="nav-label">Labs</span>
            <ChevronDownIcon className={`labs-chevron${labsOpen ? " open" : ""}`} />
          </button>
          {labsOpen
            ? LABS_NAV.map(({ id, label, icon: Icon }) => (
                <button
                  key={id}
                  type="button"
                  className="nav-item nav-item-nested"
                  aria-current={tab === id ? "page" : undefined}
                  onClick={() => setTab(id)}
                >
                  <Icon />
                  <span className="nav-label">{label}</span>
                </button>
              ))
            : null}

          <div className="sidebar-spacer" />

          <div className="sidebar-privacy">
            <LockIcon />
            <span>On-device · private</span>
          </div>

          <div className="sidebar-account-wrap">
            <button
              type="button"
              className="sidebar-account"
              aria-current={tab === "account" ? "page" : undefined}
              aria-expanded={userMenuOpen}
              onClick={() => setUserMenuOpen((v) => !v)}
            >
              <span className="avatar">
                <AccountIcon />
              </span>
              <span style={{ flex: 1, minWidth: 0 }}>
                <span className="account-name">Account</span>
                <span className="account-sub">Local-first · your keys</span>
              </span>
            </button>

            {userMenuOpen ? (
              <>
                <button
                  type="button"
                  className="menu-backdrop"
                  aria-label="Close menu"
                  onClick={() => setUserMenuOpen(false)}
                />
                <div className="context-menu user-menu">
                  <button
                    type="button"
                    onClick={() => {
                      setTab("account");
                      setUserMenuOpen(false);
                    }}
                  >
                    <AccountIcon />
                    Account &amp; models
                  </button>
                  <div className="context-menu-sep" />
                  <div className="context-menu-label">Theme</div>
                  <div className="user-menu-row">
                    <ThemeIcon />
                    <select
                      aria-label="theme"
                      value={theme}
                      onChange={(e) => setTheme(e.target.value as Theme)}
                    >
                      <option value="system">System</option>
                      <option value="light">Light</option>
                      <option value="dark">Dark</option>
                    </select>
                  </div>
                  <div className="context-menu-sep" />
                  <button type="button" onClick={() => void toggleAutostart(!autostart)}>
                    <StartupIcon />
                    <span style={{ flex: 1 }}>Start at login</span>
                    <span className={`menu-check${autostart ? " on" : ""}`}>
                      {autostart ? "On" : "Off"}
                    </span>
                  </button>
                </div>
              </>
            ) : null}
          </div>
        </aside>
        <main className="content">
          {panels
            .filter(({ id }) => mountedTabs[id])
            .map(({ id, name, node }) => (
              <div key={id} style={{ display: tab === id ? "contents" : "none" }}>
                <PaneErrorBoundary paneKey={id} paneName={name}>
                  {node}
                </PaneErrorBoundary>
              </div>
            ))}
        </main>
      </div>
      <ConfirmDialog
        open={autostartPrompt}
        title="Launch Arya at login?"
        message="Arya will start automatically when you log in and wait in your menu bar, ready for dictation. You can change this anytime from the sidebar."
        confirmLabel="Start at login"
        cancelLabel="Not now"
        onConfirm={() => {
          setAutostartPrompt(false);
          void toggleAutostart(true);
        }}
        onCancel={() => {
          setAutostartPrompt(false);
          localStorage.setItem("arya-autostart-decided", "true");
        }}
      />
      <Modal
        open={cheatSheetOpen}
        onClose={() => setCheatSheetOpen(false)}
        labelledBy="cheat-title"
      >
        <h3 id="cheat-title" className="modal-title">
          Keyboard shortcuts
        </h3>
        <div className="cheat-grid">
          <Shortcut keys="g n" label="Notes" />
          <Shortcut keys="g d" label="Dictation" />
          <Shortcut keys="g a" label="Agent" />
          <Shortcut keys="g s" label="Search" />
          <Shortcut keys="g k" label="Knowledge" />
          <Shortcut keys="g g" label="Galaxy" />
          <Shortcut keys="g m" label="Mind Map" />
          <Shortcut keys="?" label="Toggle this sheet" />
        </div>
      </Modal>
    </>
  );
}

function Shortcut({ keys, label }: { keys: string; label: string }) {
  return (
    <div className="cheat-row">
      <span className="cheat-keys">{keys}</span>
      <span className="cheat-label">{label}</span>
    </div>
  );
}

/** A "power" glyph for the launch-at-login toggle. */
function StartupIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 3v8" />
      <path d="M7.5 6.6a7 7 0 1 0 9 0" />
    </svg>
  );
}
