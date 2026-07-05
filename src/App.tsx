import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import brand from "../brand.json";
import { AccountGate } from "./account/AccountGate";
import { AccountPanel } from "./account/AccountPanel";
import { AgentPanel } from "./agent/AgentPanel";
import { McpPanel } from "./agent/McpPanel";
import { RoutinesPanel } from "./agent/RoutinesPanel";
import { DictationPanel } from "./dictation/DictationPanel";
import { type AccountSnapshot, accountSnapshot } from "./lib/account";
import { disableAutostart, enableAutostart, isAutostartEnabled } from "./lib/autostart";
import { loadTheme, saveTheme, type Theme } from "./lib/theme";
import { NotesWorkspace } from "./notes/NotesWorkspace";
import { Onboarding, onboardingComplete } from "./onboarding/Onboarding";
import { SearchPanel } from "./search/SearchPanel";
import { ConfirmDialog } from "./ui/dialogs";
import {
  AccountIcon,
  AgentIcon,
  DictationIcon,
  LockIcon,
  McpIcon,
  NotesIcon,
  RoutinesIcon,
  SearchIcon,
  ThemeIcon,
} from "./ui/icons";

type Tab = "notes" | "agent" | "search" | "routines" | "mcp" | "dictation" | "account";

const NAV: { id: Tab; label: string; icon: (p: { className?: string }) => React.JSX.Element }[] = [
  { id: "notes", label: "Notes", icon: NotesIcon },
  { id: "agent", label: "Agent", icon: AgentIcon },
  { id: "search", label: "Search", icon: SearchIcon },
  { id: "routines", label: "Routines", icon: RoutinesIcon },
  { id: "mcp", label: "MCP servers", icon: McpIcon },
  { id: "dictation", label: "Dictation", icon: DictationIcon },
];

const cap = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : s);

/**
 * Main-window shell: a tinted sidebar of pillars over a warm ground, with each
 * screen laid out as its own inset panel in the padded main region.
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");
  const [onboarded, setOnboarded] = useState(onboardingComplete);
  const [theme, setThemeState] = useState<Theme>(loadTheme);
  const [account, setAccount] = useState<AccountSnapshot | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [autostartPrompt, setAutostartPrompt] = useState(false);

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

  useEffect(() => {
    const unlisten = listen("tray:new-session", () => setTab("agent"));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    // Best-effort: the sidebar shows plan + credits when a backend answers;
    // in local mode it stays quiet rather than erroring. Re-fetch whenever auth
    // changes anywhere (sign-in via the loopback callback, sign-out from the
    // account panel) so the sidebar never shows a stale tier until reload.
    const refresh = () => {
      void accountSnapshot()
        .then(setAccount)
        .catch(() => setAccount(null));
    };
    refresh();
    const unlistenIn = listen("account:signed-in", refresh);
    const unlistenOut = listen("account:signed-out", refresh);
    return () => {
      void unlistenIn.then((fn) => fn());
      void unlistenOut.then((fn) => fn());
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

  if (!onboarded) {
    return <Onboarding onFinish={() => setOnboarded(true)} />;
  }

  const panel = {
    notes: <NotesWorkspace />,
    agent: <AgentPanel />,
    search: <SearchPanel />,
    routines: <RoutinesPanel />,
    mcp: <McpPanel />,
    dictation: <DictationPanel />,
    account: <AccountPanel />,
  }[tab];

  return (
    <AccountGate>
      <div className="app">
        <aside className="sidebar">
          <div className="sidebar-brand">
            <span className="brand-logo" aria-hidden="true">
              <span className="core" />
              <span className="pt n" />
              <span className="pt s" />
              <span className="pt w" />
              <span className="pt e" />
            </span>
            <span className="brand-name">{brand.name}</span>
          </div>

          {NAV.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              className="nav-item"
              aria-current={tab === id}
              onClick={() => setTab(id)}
            >
              <Icon />
              <span className="nav-label">{label}</span>
            </button>
          ))}

          <div className="sidebar-spacer" />

          <div className="sidebar-privacy">
            <LockIcon />
            <span>On-device · private</span>
          </div>

          <button
            type="button"
            className="sidebar-account"
            aria-current={tab === "account"}
            onClick={() => setTab("account")}
          >
            <span className="avatar">
              <AccountIcon />
            </span>
            <span style={{ flex: 1, minWidth: 0 }}>
              <span className="account-name">
                {account ? `${cap(account.tier)} plan` : "Account"}
              </span>
              <span className="account-sub">
                {account ? `${account.remainingCredits.toLocaleString()} credits` : "Local mode"}
              </span>
            </span>
          </button>

          <label className="sidebar-theme">
            <span className="hstack">
              <ThemeIcon />
              Theme
            </span>
            <select
              aria-label="theme"
              value={theme}
              onChange={(e) => setTheme(e.target.value as Theme)}
            >
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>

          <div className="sidebar-theme" style={{ cursor: "default" }}>
            <span className="hstack">
              <StartupIcon />
              Start at login
            </span>
            <button
              type="button"
              role="switch"
              aria-checked={autostart}
              aria-label="Start Arya at login"
              className="switch bare"
              onClick={() => void toggleAutostart(!autostart)}
            />
          </div>
        </aside>
        <main className="content">{panel}</main>
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
    </AccountGate>
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
