import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  assignNoteToFolder,
  createFolder,
  deleteNote,
  discardRecording,
  type Folder,
  finishRecording,
  getNote,
  getNoteTurns,
  listFolders,
  listNotes,
  type NoteDetail,
  type NoteSummary,
  pauseRecording,
  type RecoverableRecording,
  recordingStatus,
  recoverRecording,
  resumeRecording,
  retryProcessing,
  scanRecoverableRecordings,
  startRecording,
  type TranscriptTurn,
  updateNote,
} from "../lib/notes";

const STATUS_LABEL: Record<string, string> = {
  idle: "",
  recording: "Recording",
  transcribing: "Transcribing…",
  generating: "Generating…",
  ready: "",
  failed: "Failed",
};

function formatElapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

/** Notes workspace: folder sidebar, note list, editor, recorder bar. */
export function NotesWorkspace() {
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [activeFolder, setActiveFolder] = useState<string | null>(null);
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  const [detail, setDetail] = useState<NoteDetail | null>(null);
  const [turns, setTurns] = useState<TranscriptTurn[]>([]);
  const [recorder, setRecorder] = useState<{ state: string; elapsedMs: number }>({
    state: "idle",
    elapsedMs: 0,
  });
  const [recoverables, setRecoverables] = useState<RecoverableRecording[]>([]);
  const [meeting, setMeeting] = useState<{ appName: string } | null>(null);
  const [upcoming, setUpcoming] = useState<{ title: string; startsInMin: number } | null>(null);
  const [livePreview, setLivePreview] = useState<string | null>(null);
  const [systemAudioWarning, setSystemAudioWarning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeNoteRef = useRef<string | null>(null);
  activeNoteRef.current = activeNoteId;

  const refreshNotes = useCallback(async () => {
    try {
      const [n, f] = await Promise.all([listNotes(), listFolders()]);
      setNotes(n);
      setFolders(f);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const openNote = useCallback(async (id: string) => {
    setActiveNoteId(id);
    setOpenTabs((tabs) => (tabs.includes(id) ? tabs : [...tabs, id]));
    try {
      setDetail(await getNote(id));
      setTurns(await getNoteTurns(id));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshNotes();
    void scanRecoverableRecordings()
      .then(setRecoverables)
      .catch(() => {});
    const unlisten = listen<{ noteId: string; status: string }>("note:processing", (event) => {
      void refreshNotes();
      if (event.payload.status === "ready") {
        setLivePreview(null);
        // Don't clobber the note the user is actively editing; only auto-open
        // a freshly-ready note if it isn't the current buffer.
        if (activeNoteRef.current !== event.payload.noteId) {
          void openNote(event.payload.noteId);
        } else {
          void getNoteTurns(event.payload.noteId)
            .then(setTurns)
            .catch(() => {});
        }
      }
    });
    const unlistenMeeting = listen<{ appName: string }>("meeting:detected", (event) => {
      setMeeting(event.payload);
    });
    const unlistenCleared = listen("meeting:cleared", () => setMeeting(null));
    const unlistenUpcoming = listen<{ title: string; startsInMin: number }>(
      "calendar:upcoming",
      (event) => setUpcoming(event.payload),
    );
    const unlistenPreview = listen<{ noteId: string; text: string }>("note:live-preview", (event) =>
      setLivePreview(event.payload.text),
    );
    const unlistenSystemWarn = listen<string>("recording:system-audio-unavailable", (event) =>
      setSystemAudioWarning(event.payload),
    );
    const poll = setInterval(() => {
      void recordingStatus()
        .then((s) => setRecorder({ state: s.state, elapsedMs: s.elapsedMs }))
        .catch(() => {});
    }, 500);
    return () => {
      void unlisten.then((fn) => fn());
      void unlistenMeeting.then((fn) => fn());
      void unlistenCleared.then((fn) => fn());
      void unlistenUpcoming.then((fn) => fn());
      void unlistenPreview.then((fn) => fn());
      void unlistenSystemWarn.then((fn) => fn());
      clearInterval(poll);
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, [refreshNotes, openNote]);

  const closeTab = (id: string) => {
    setOpenTabs((tabs) => tabs.filter((t) => t !== id));
    if (activeNoteId === id) {
      setActiveNoteId(null);
      setDetail(null);
      setTurns([]);
    }
  };

  const editDetail = (fields: Partial<Pick<NoteDetail, "title" | "bodyMd" | "manualNotes">>) => {
    if (!detail) return;
    const next = { ...detail, ...fields };
    setDetail(next);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      void updateNote(next.id, {
        title: next.title,
        bodyMd: next.bodyMd,
        manualNotes: next.manualNotes,
      })
        .then(refreshNotes)
        .catch((e) => setError(String(e)));
    }, 600);
  };

  const onRecord = async (sourceMode?: "microphone-only" | "microphone-and-system") => {
    try {
      if (recorder.state === "idle") {
        setSystemAudioWarning(null);
        setLivePreview(null);
        const noteId = await startRecording(activeNoteId ?? undefined, sourceMode);
        setMeeting(null);
        await refreshNotes();
        await openNote(noteId);
      } else {
        await finishRecording();
      }
    } catch (e) {
      setError(String(e));
    }
  };

  const visibleNotes = activeFolder ? notes.filter((n) => n.folderId === activeFolder) : notes;
  const tabTitle = (id: string) => notes.find((n) => n.id === id)?.title ?? "Note";

  return (
    <div style={{ display: "flex", gap: 16, alignItems: "flex-start" }}>
      <aside style={{ width: 220, flexShrink: 0 }}>
        <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
          <button type="button" onClick={() => void onRecord()} aria-label="record">
            {recorder.state === "idle" ? "● Record" : `■ Stop ${formatElapsed(recorder.elapsedMs)}`}
          </button>
          {recorder.state === "idle" ? (
            <button
              type="button"
              aria-label="record meeting"
              title="Record microphone plus system audio"
              onClick={() => void onRecord("microphone-and-system")}
            >
              ● Meeting
            </button>
          ) : null}
          {recorder.state === "recording" ? (
            <button type="button" onClick={() => void pauseRecording()}>
              Pause
            </button>
          ) : null}
          {recorder.state === "paused" ? (
            <button type="button" onClick={() => void resumeRecording()}>
              Resume
            </button>
          ) : null}
        </div>

        {upcoming && recorder.state === "idle" ? (
          <div role="status" style={{ margin: "8px 0", padding: 8, background: "#ede9fe" }}>
            <strong>{upcoming.title}</strong> starts in {Math.max(0, upcoming.startsInMin)} min.{" "}
            <button type="button" onClick={() => void onRecord("microphone-and-system")}>
              Record it
            </button>
            <button type="button" onClick={() => setUpcoming(null)}>
              Dismiss
            </button>
          </div>
        ) : null}

        {meeting && recorder.state === "idle" ? (
          <div role="status" style={{ margin: "8px 0", padding: 8, background: "#dbeafe" }}>
            <strong>Meeting detected in {meeting.appName}.</strong>{" "}
            <button type="button" onClick={() => void onRecord("microphone-and-system")}>
              Record meeting
            </button>
            <button type="button" onClick={() => setMeeting(null)}>
              Dismiss
            </button>
          </div>
        ) : null}

        {systemAudioWarning ? (
          <div role="alert" style={{ margin: "8px 0", padding: 8, background: "#fee2e2" }}>
            System audio unavailable ({systemAudioWarning}); recording microphone only. Grant
            "System Audio Recording" in System Settings for meeting capture.
          </div>
        ) : null}

        {recoverables.length > 0 ? (
          <div role="alert" style={{ margin: "8px 0", padding: 8, background: "#fef3c7" }}>
            <strong>Interrupted recording found.</strong>
            {recoverables.map((r) => (
              <div key={r.sessionId}>
                {r.noteTitle} ({Math.round(r.sizeBytes / 1024)} KB)
                <button
                  type="button"
                  onClick={() =>
                    void recoverRecording(r.sessionId).then(() => {
                      setRecoverables((list) => list.filter((x) => x.sessionId !== r.sessionId));
                      return refreshNotes();
                    })
                  }
                >
                  Recover
                </button>
                <button
                  type="button"
                  onClick={() =>
                    void discardRecording(r.sessionId).then(() => {
                      setRecoverables((list) => list.filter((x) => x.sessionId !== r.sessionId));
                    })
                  }
                >
                  Discard
                </button>
              </div>
            ))}
          </div>
        ) : null}

        <nav aria-label="folders" style={{ margin: "12px 0" }}>
          <button type="button" onClick={() => setActiveFolder(null)} disabled={!activeFolder}>
            All notes
          </button>
          {folders.map((folder) => (
            <button
              key={folder.id}
              type="button"
              onClick={() => setActiveFolder(folder.id)}
              disabled={activeFolder === folder.id}
            >
              {folder.name}
            </button>
          ))}
          <button
            type="button"
            onClick={() => {
              const name = window.prompt("Folder name");
              if (name) void createFolder(name).then(refreshNotes);
            }}
          >
            + Folder
          </button>
        </nav>

        <ul aria-label="notes" style={{ listStyle: "none", padding: 0 }}>
          {visibleNotes.map((note) => (
            <li key={note.id} style={{ marginBottom: 4 }}>
              <button type="button" onClick={() => void openNote(note.id)}>
                {note.title}
              </button>{" "}
              <small>{STATUS_LABEL[note.processingStatus] ?? note.processingStatus}</small>
            </li>
          ))}
        </ul>
      </aside>

      <section style={{ flex: 1, minWidth: 0 }}>
        {openTabs.length > 0 ? (
          <div role="tablist" style={{ display: "flex", gap: 4, marginBottom: 8 }}>
            {openTabs.map((id) => (
              <span key={id} role="tab" aria-selected={id === activeNoteId} tabIndex={0}>
                <button type="button" onClick={() => void openNote(id)}>
                  {tabTitle(id)}
                </button>
                <button
                  type="button"
                  aria-label={`close ${tabTitle(id)}`}
                  onClick={() => closeTab(id)}
                >
                  ×
                </button>
              </span>
            ))}
          </div>
        ) : null}

        {error ? <p role="alert">{error}</p> : null}

        {detail ? (
          <article>
            <input
              aria-label="note title"
              value={detail.title}
              onChange={(e) => editDetail({ title: e.target.value })}
              style={{ fontSize: 20, width: "100%" }}
            />
            {detail.calendarContext
              ? (() => {
                  try {
                    const ctx = JSON.parse(detail.calendarContext) as {
                      title: string;
                      attendees: string[];
                    };
                    return ctx.attendees.length > 0 ? (
                      <p>
                        <small>Attendees: {ctx.attendees.join(", ")}</small>
                      </p>
                    ) : null;
                  } catch {
                    return null;
                  }
                })()
              : null}
            {detail.processingStatus === "failed" ? (
              <p role="alert">
                Processing failed: {detail.processingError}{" "}
                <button type="button" onClick={() => void retryProcessing(detail.id)}>
                  Retry
                </button>
              </p>
            ) : null}
            {["transcribing", "generating", "recording"].includes(detail.processingStatus) ? (
              <p>{STATUS_LABEL[detail.processingStatus]}</p>
            ) : null}
            {recorder.state !== "idle" && livePreview ? (
              <blockquote aria-label="live preview" style={{ color: "#6b7280" }}>
                {livePreview}
              </blockquote>
            ) : null}
            <label>
              Manual notes
              <textarea
                aria-label="manual notes"
                value={detail.manualNotes}
                onChange={(e) => editDetail({ manualNotes: e.target.value })}
                rows={3}
                style={{ width: "100%" }}
              />
            </label>
            <label>
              Note
              <textarea
                aria-label="note body"
                value={detail.bodyMd}
                onChange={(e) => editDetail({ bodyMd: e.target.value })}
                rows={14}
                style={{ width: "100%" }}
              />
            </label>
            <select
              aria-label="note folder"
              value={detail.folderId ?? ""}
              onChange={(e) => {
                const folderId = e.target.value || null;
                void assignNoteToFolder(detail.id, folderId).then(refreshNotes);
                setDetail({ ...detail, folderId });
              }}
            >
              <option value="">No folder</option>
              {folders.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => {
                if (window.confirm("Delete this note?")) {
                  void deleteNote(detail.id).then(() => {
                    closeTab(detail.id);
                    return refreshNotes();
                  });
                }
              }}
            >
              Delete note
            </button>
            {turns.length > 0 ? (
              <details>
                <summary>Transcript ({turns.length} turns)</summary>
                <ul aria-label="transcript turns">
                  {turns.map((turn) => (
                    <li key={turn.turnIndex}>
                      <small>{formatElapsed(turn.startMs)}</small>{" "}
                      {turn.speaker ? <strong>{turn.speaker}: </strong> : null}
                      {turn.text}
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </article>
        ) : (
          <p>Select a note, or press Record to capture one.</p>
        )}
      </section>
    </div>
  );
}
