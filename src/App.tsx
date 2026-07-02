import { useCallback, useEffect, useState } from "react";
import brand from "../brand.json";
import { createNote, listNotes, type Note } from "./lib/notes";

/**
 * Walking-skeleton shell: proves UI -> Tauri command -> SQLite -> UI round
 * trips end to end. Replaced by the real workspace shell in later milestones.
 */
export function App() {
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
    <main style={{ fontFamily: "system-ui", padding: "2rem", maxWidth: 640, margin: "0 auto" }}>
      <h1>{brand.name}</h1>
      <button type="button" onClick={onNewNote}>
        New note
      </button>
      {error ? <p role="alert">{error}</p> : null}
      <ul aria-label="notes">
        {notes.map((note) => (
          <li key={note.id}>
            {note.title} <small>{note.createdAt}</small>
          </li>
        ))}
      </ul>
    </main>
  );
}
