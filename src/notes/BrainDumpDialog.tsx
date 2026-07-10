import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import {
  createFolder,
  createNotesFromSplit,
  deleteNote,
  type Folder,
  type ProposedNote,
  readTextFiles,
  splitBraindumpIntoNotes,
} from "../lib/notes";

/** Text formats that can be added to a dump via the picker. */
const TEXT_EXTENSIONS = ["txt", "md", "markdown", "csv", "json", "log", "text"];
/** Minimum characters worth organizing (matches the backend guard). */
const MIN_CHARS = 20;

/**
 * Brain-dump → coherent notes (suggest-then-confirm). Phase one collects a
 * jumble of ideas (typed, pasted, dictated, or read from text files); phase two
 * reviews the local model's proposed single-topic notes before any are created.
 * Nothing is written until the user confirms.
 */
export function BrainDumpDialog({
  open,
  onClose,
  folders,
  seedText,
  sourceNoteId,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  folders: Folder[];
  seedText?: string;
  sourceNoteId?: string | null;
  onCreated: (count: number) => void;
}) {
  const [input, setInput] = useState("");
  const [proposed, setProposed] = useState<ProposedNote[] | null>(null);
  const [titles, setTitles] = useState<string[]>([]);
  const [skipped, setSkipped] = useState<Set<number>>(() => new Set());
  const [folderChoice, setFolderChoice] = useState("");
  const [newFolderName, setNewFolderName] = useState("");
  const [deleteSource, setDeleteSource] = useState(false);
  const [busy, setBusy] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Reset the flow whenever the dialog (re)opens.
  useEffect(() => {
    if (!open) return;
    setInput(seedText ?? "");
    setProposed(null);
    setTitles([]);
    setSkipped(new Set());
    setFolderChoice("");
    setNewFolderName("");
    setDeleteSource(false);
    setError(null);
    setBusy(false);
  }, [open, seedText]);

  // Tick an elapsed-seconds counter while the model works, so a long organize on
  // a large dump reads as "working", not frozen.
  useEffect(() => {
    if (!busy) return;
    setElapsed(0);
    const t = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(t);
  }, [busy]);

  if (!open) return null;

  const addFiles = async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        title: "Add text files",
        filters: [{ name: "Text", extensions: TEXT_EXTENSIONS }],
      });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (paths.length === 0) return;
      const text = await readTextFiles(paths);
      setInput((prev) => (prev.trim() ? `${prev}\n\n${text}` : text));
    } catch (e) {
      setError(String(e));
    }
  };

  const organize = async () => {
    if (input.trim().length < MIN_CHARS || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await splitBraindumpIntoNotes(input);
      setProposed(result);
      setTitles(result.map((r) => r.title));
      setSkipped(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const create = async () => {
    if (!proposed || busy) return;
    const accepted = proposed.flatMap((p, i) =>
      skipped.has(i) ? [] : [{ title: titles[i]?.trim() || p.title, body: p.body }],
    );
    if (accepted.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      let folderId: string | null = null;
      if (folderChoice === "__new__" && newFolderName.trim()) {
        folderId = (await createFolder(newFolderName.trim())).id;
      } else if (folderChoice && folderChoice !== "__new__") {
        folderId = folderChoice;
      }
      await createNotesFromSplit(accepted, folderId);
      if (deleteSource && sourceNoteId) await deleteNote(sourceNoteId);
      onCreated(accepted.length);
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const acceptedCount = proposed ? proposed.length - skipped.size : 0;

  return (
    <div role="dialog" aria-label="Organize into notes" className="modal-overlay">
      <div className="panel modal-card brain-card">
        {proposed === null ? (
          <>
            <div className="panel-title" style={{ marginBottom: 4 }}>
              Organize into notes
            </div>
            <p className="muted" style={{ margin: "0 0 12px", fontSize: 13 }}>
              Dump any jumble of ideas — unrelated is fine. Arya groups them by topic into separate,
              coherent notes. You can type, paste, dictate, or add text files.
            </p>
            <textarea
              className="brain-input"
              aria-label="Brain dump"
              placeholder="Paste or type your brain dump here…"
              value={input}
              // biome-ignore lint/a11y/noAutofocus: focusing the input in this dialog is expected
              autoFocus
              onChange={(e) => setInput(e.target.value)}
            />
            {error ? (
              <p role="alert" className="brain-error">
                {error}
              </p>
            ) : null}
            {busy ? (
              <p className="brain-status muted" role="status">
                <span className="brain-spinner" aria-hidden="true" />
                Organizing your dump… {elapsed}s. A large dump can take a minute or two on the local
                model — hang tight.
              </p>
            ) : null}
            <div className="hstack spread" style={{ marginTop: 14 }}>
              <button
                type="button"
                className="btn-sm btn-ghost"
                disabled={busy}
                onClick={() => void addFiles()}
              >
                Add text files
              </button>
              <div className="hstack" style={{ gap: 8 }}>
                <button type="button" className="btn-sm btn-ghost" onClick={onClose}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="btn-sm btn-primary"
                  disabled={busy || input.trim().length < MIN_CHARS}
                  onClick={() => void organize()}
                >
                  {busy ? "Organizing…" : "Organize into notes"}
                </button>
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="panel-title" style={{ marginBottom: 4 }}>
              Review notes
            </div>
            <p className="muted" style={{ margin: "0 0 12px", fontSize: 13 }}>
              Arya split your dump into {proposed.length} note{proposed.length === 1 ? "" : "s"}.
              Edit titles, skip any you don't want, pick a folder, then create — nothing is saved
              until you do.
            </p>
            <div className="brain-review-list">
              {proposed.map((p, i) => {
                const isSkipped = skipped.has(i);
                return (
                  <div
                    // biome-ignore lint/suspicious/noArrayIndexKey: proposals are a stable, non-reordered list for this render
                    key={i}
                    className={`brain-note${isSkipped ? " skipped" : ""}`}
                  >
                    <div className="brain-note-head">
                      <input
                        className="brain-note-title"
                        aria-label={`Title for note ${i + 1}`}
                        value={titles[i] ?? ""}
                        disabled={isSkipped}
                        onChange={(e) =>
                          setTitles((prev) => {
                            const next = [...prev];
                            next[i] = e.target.value;
                            return next;
                          })
                        }
                      />
                      <button
                        type="button"
                        className="btn-sm btn-ghost"
                        onClick={() =>
                          setSkipped((prev) => {
                            const next = new Set(prev);
                            if (next.has(i)) next.delete(i);
                            else next.add(i);
                            return next;
                          })
                        }
                      >
                        {isSkipped ? "Keep" : "Skip"}
                      </button>
                    </div>
                    <div className="brain-note-body">{p.body.slice(0, 280)}</div>
                  </div>
                );
              })}
            </div>

            <div className="brain-options">
              <label className="brain-folder">
                Add to
                <select value={folderChoice} onChange={(e) => setFolderChoice(e.target.value)}>
                  <option value="">No folder</option>
                  {folders.map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.name}
                    </option>
                  ))}
                  <option value="__new__">New folder…</option>
                </select>
              </label>
              {folderChoice === "__new__" ? (
                <input
                  className="brain-newfolder"
                  aria-label="New folder name"
                  placeholder="Folder name"
                  value={newFolderName}
                  onChange={(e) => setNewFolderName(e.target.value)}
                />
              ) : null}
              {sourceNoteId ? (
                <label className="brain-delete-source">
                  <input
                    type="checkbox"
                    checked={deleteSource}
                    onChange={(e) => setDeleteSource(e.target.checked)}
                  />
                  Delete the original note
                </label>
              ) : null}
            </div>

            {error ? (
              <p role="alert" className="brain-error">
                {error}
              </p>
            ) : null}

            <div className="hstack spread" style={{ marginTop: 14 }}>
              <button type="button" className="btn-sm btn-ghost" onClick={() => setProposed(null)}>
                Back
              </button>
              <div className="hstack" style={{ gap: 8 }}>
                <button type="button" className="btn-sm btn-ghost" onClick={onClose}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="btn-sm btn-primary"
                  disabled={busy || acceptedCount === 0}
                  onClick={() => void create()}
                >
                  {busy
                    ? "Creating…"
                    : `Create ${acceptedCount} note${acceptedCount === 1 ? "" : "s"}`}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
