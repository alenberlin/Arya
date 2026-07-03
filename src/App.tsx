import { useState } from "react";
import brand from "../brand.json";
import { AgentPanel } from "./agent/AgentPanel";
import { DictationPanel } from "./dictation/DictationPanel";
import { NotesWorkspace } from "./notes/NotesWorkspace";

type Tab = "notes" | "agent" | "dictation";

/**
 * Main-window shell: Notes workspace (M4) and Dictation panel (M3).
 * The polished app chrome lands in M13.
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");

  return (
    <main style={{ fontFamily: "system-ui", padding: "1.5rem", maxWidth: 1000, margin: "0 auto" }}>
      <header style={{ display: "flex", alignItems: "baseline", gap: 16, marginBottom: 12 }}>
        <h1 style={{ margin: 0 }}>{brand.name}</h1>
        <nav style={{ display: "flex", gap: 8 }}>
          <button type="button" onClick={() => setTab("notes")} disabled={tab === "notes"}>
            Notes
          </button>
          <button type="button" onClick={() => setTab("agent")} disabled={tab === "agent"}>
            Agent
          </button>
          <button type="button" onClick={() => setTab("dictation")} disabled={tab === "dictation"}>
            Dictation
          </button>
        </nav>
      </header>
      {tab === "notes" ? <NotesWorkspace /> : tab === "agent" ? <AgentPanel /> : <DictationPanel />}
    </main>
  );
}
