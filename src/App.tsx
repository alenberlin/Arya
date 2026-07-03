import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import brand from "../brand.json";
import { AccountGate } from "./account/AccountGate";
import { AccountPanel } from "./account/AccountPanel";
import { AgentPanel } from "./agent/AgentPanel";
import { McpPanel } from "./agent/McpPanel";
import { RoutinesPanel } from "./agent/RoutinesPanel";
import { DictationPanel } from "./dictation/DictationPanel";
import { loadTheme, saveTheme, type Theme } from "./lib/theme";
import { NotesWorkspace } from "./notes/NotesWorkspace";
import { Onboarding, onboardingComplete } from "./onboarding/Onboarding";
import { SearchPanel } from "./search/SearchPanel";
import {
  AccountIcon,
  AgentIcon,
  DictationIcon,
  McpIcon,
  NotesIcon,
  RoutinesIcon,
  SearchIcon,
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

/**
 * Main-window shell: a left sidebar of pillars and an inset content card.
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");
  const [onboarded, setOnboarded] = useState(onboardingComplete);
  const [theme, setThemeState] = useState<Theme>(loadTheme);

  const setTheme = (next: Theme) => {
    saveTheme(next);
    setThemeState(next);
  };

  useEffect(() => {
    const unlisten = listen("tray:new-session", () => setTab("agent"));
    return () => {
      void unlisten.then((fn) => fn());
    };
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
            <span className="dot" />
            {brand.name}
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
              {label}
            </button>
          ))}
          <div className="sidebar-spacer" />
          <div className="sidebar-footer">
            <button
              type="button"
              className="nav-item"
              aria-current={tab === "account"}
              onClick={() => setTab("account")}
            >
              <AccountIcon />
              Account
            </button>
            <label className="nav-item" style={{ cursor: "default" }}>
              Theme
              <select
                aria-label="theme"
                value={theme}
                onChange={(e) => setTheme(e.target.value as Theme)}
                style={{ width: "auto", marginLeft: "auto" }}
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
          </div>
        </aside>
        <main className="content">
          <div className="content-inner">{panel}</div>
        </main>
      </div>
    </AccountGate>
  );
}
