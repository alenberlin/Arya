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

type Tab = "notes" | "agent" | "search" | "routines" | "mcp" | "dictation" | "account";

/**
 * Main-window shell: Notes workspace (M4) and Dictation panel (M3).
 * The polished app chrome lands in M13.
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

  return (
    <AccountGate>
      <main
        style={{ fontFamily: "system-ui", padding: "1.5rem", maxWidth: 1000, margin: "0 auto" }}
      >
        <header style={{ display: "flex", alignItems: "baseline", gap: 16, marginBottom: 12 }}>
          <h1 style={{ margin: 0 }}>{brand.name}</h1>
          <nav className="app-nav" style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button type="button" onClick={() => setTab("notes")} disabled={tab === "notes"}>
              Notes
            </button>
            <button type="button" onClick={() => setTab("agent")} disabled={tab === "agent"}>
              Agent
            </button>
            <button type="button" onClick={() => setTab("search")} disabled={tab === "search"}>
              Search
            </button>
            <button type="button" onClick={() => setTab("routines")} disabled={tab === "routines"}>
              Routines
            </button>
            <button type="button" onClick={() => setTab("mcp")} disabled={tab === "mcp"}>
              MCP
            </button>
            <button
              type="button"
              onClick={() => setTab("dictation")}
              disabled={tab === "dictation"}
            >
              Dictation
            </button>
            <button type="button" onClick={() => setTab("account")} disabled={tab === "account"}>
              Account
            </button>
            <select
              aria-label="theme"
              value={theme}
              onChange={(e) => setTheme(e.target.value as Theme)}
              style={{ marginLeft: "auto" }}
            >
              <option value="system">System theme</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </nav>
        </header>
        {tab === "notes" ? (
          <NotesWorkspace />
        ) : tab === "agent" ? (
          <AgentPanel />
        ) : tab === "search" ? (
          <SearchPanel />
        ) : tab === "routines" ? (
          <RoutinesPanel />
        ) : tab === "mcp" ? (
          <McpPanel />
        ) : tab === "dictation" ? (
          <DictationPanel />
        ) : (
          <AccountPanel />
        )}
      </main>
    </AccountGate>
  );
}
