import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import brand from "../brand.json";
import { AccountGate } from "./account/AccountGate";
import { AccountPanel } from "./account/AccountPanel";
import { AgentPanel } from "./agent/AgentPanel";
import { McpPanel } from "./agent/McpPanel";
import { RoutinesPanel } from "./agent/RoutinesPanel";
import { DictationPanel } from "./dictation/DictationPanel";
import { NotesWorkspace } from "./notes/NotesWorkspace";
import { SearchPanel } from "./search/SearchPanel";

type Tab = "notes" | "agent" | "search" | "routines" | "mcp" | "dictation" | "account";

/**
 * Main-window shell: Notes workspace (M4) and Dictation panel (M3).
 * The polished app chrome lands in M13.
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");

  useEffect(() => {
    const unlisten = listen("tray:new-session", () => setTab("agent"));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <AccountGate>
      <main
        style={{ fontFamily: "system-ui", padding: "1.5rem", maxWidth: 1000, margin: "0 auto" }}
      >
        <header style={{ display: "flex", alignItems: "baseline", gap: 16, marginBottom: 12 }}>
          <h1 style={{ margin: 0 }}>{brand.name}</h1>
          <nav style={{ display: "flex", gap: 8 }}>
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
