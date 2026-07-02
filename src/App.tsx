import { useCallback, useEffect, useState } from "react";
import brand from "../brand.json";
import { DictationPanel } from "./dictation/DictationPanel";
import { createNote, listNotes, type Note } from "./lib/notes";

type Tab = "notes" | "dictation";

/**
 * Main-window shell. Minimal two-tab layout until the workspace shell
 * arrives (M4/M13): Notes (walking-skeleton slice) and Dictation (M3).
 */
export function App() {
  const [tab, setTab] = useState<Tab>("notes");
  const [notes, setNotes] = useState<Note[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setNotes(await listNotes());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const onNewNote = async () => {
    try {
      await createNote("New note");
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <main style={{ fontFamily: "system-ui", padding: "2rem", maxWidth: 720, margin: "0 auto" }}>
      <h1>{brand.name}</h1>
      <nav style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <button type="button" onClick={() => setTab("notes")} disabled={tab === "notes"}>
          Notes
        </button>
        <button type="button" onClick={() => setTab("dictation")} disabled={tab === "dictation"}>
          Dictation
        </button>
      </nav>
      {error ? <p role="alert">{error}</p> : null}
      {tab === "notes" ? (
        <section>
          <button type="button" onClick={onNewNote}>
            New note
          </button>
          <ul aria-label="notes">
            {notes.map((note) => (
              <li key={note.id}>
                {note.title} <small>{note.createdAt}</small>
              </li>
            ))}
          </ul>
        </section>
      ) : (
        <DictationPanel />
      )}
    </main>
  );
}
