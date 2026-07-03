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
import { MeetingIcon, NotesIcon, RecordIcon, StopIcon } from "../ui/icons";

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
  const recording = recorder.state !== "idle";

  return (
    <div className="split">
      <aside className="stack">
        <div className="recorder-bar">
          {!recording ? (
            <>
              <button
                type="button"
                className="btn-primary"
                onClick={() => void onRecord()}
                aria-label="record"
              >
                <RecordIcon className="rec" /> Record
              </button>
              <button
                type="button"
                aria-label="record meeting"
                title="Record microphone plus system audio"
                onClick={() => void onRecord("microphone-and-system")}
              >
                <MeetingIcon /> Meeting
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                className="btn-danger"
                onClick={() => void onRecord()}
                aria-label="record"
              >
                <StopIcon /> Stop · {formatElapsed(recorder.elapsedMs)}
              </button>
              {recorder.state === "recording" ? (
                <button type="button" className="btn-sm" onClick={() => void pauseRecording()}>
                  Pause
                </button>
              ) : (
                <button type="button" className="btn-sm" onClick={() => void resumeRecording()}>
                  Resume
                </button>
              )}
            </>
          )}
        </div>

        {upcoming && !recording ? (
          <div className="banner banner-info" role="status">
            <span>
              <strong>{upcoming.title}</strong> starts in {Math.max(0, upcoming.startsInMin)} min.
            </span>
            <div className="hstack">
              <button
                type="button"
                className="btn-sm btn-primary"
                onClick={() => void onRecord("microphone-and-system")}
              >
                Record it
              </button>
              <button type="button" className="btn-sm" onClick={() => setUpcoming(null)}>
                Dismiss
              </button>
            </div>
          </div>
        ) : null}

        {meeting && !recording ? (
          <div className="banner banner-info" role="status">
            <span className="banner-title">Meeting detected in {meeting.appName}</span>
            <div className="hstack">
              <button
                type="button"
                className="btn-sm btn-primary"
                onClick={() => void onRecord("microphone-and-system")}
              >
                Record meeting
              </button>
              <button type="button" className="btn-sm" onClick={() => setMeeting(null)}>
                Dismiss
              </button>
            </div>
          </div>
        ) : null}

        {systemAudioWarning ? (
          <div className="banner banner-warning" role="alert">
            <span className="banner-title">System audio unavailable</span>
            <small>
              Recording microphone only. Grant "System audio recording" in System Settings for
              meeting capture.
            </small>
          </div>
        ) : null}

        {recoverables.length > 0 ? (
          <div className="banner banner-warning" role="alert">
            <span className="banner-title">Interrupted recording found</span>
            {recoverables.map((r) => (
              <div key={r.sessionId} className="hstack spread">
                <small>
                  {r.noteTitle} · {Math.round(r.sizeBytes / 1024)} KB
                </small>
                <div className="hstack">
                  <button
                    type="button"
                    className="btn-sm btn-primary"
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
                    className="btn-sm"
                    onClick={() =>
                      void discardRecording(r.sessionId).then(() => {
                        setRecoverables((list) => list.filter((x) => x.sessionId !== r.sessionId));
                      })
                    }
                  >
                    Discard
                  </button>
                </div>
              </div>
            ))}
          </div>
        ) : null}

        <nav aria-label="folders" className="hstack wrap">
          <button
            type="button"
            className={activeFolder ? "btn-sm btn-ghost" : "btn-sm btn-primary"}
            onClick={() => setActiveFolder(null)}
          >
            All notes
          </button>
          {folders.map((folder) => (
            <button
              key={folder.id}
              type="button"
              className={activeFolder === folder.id ? "btn-sm btn-primary" : "btn-sm btn-ghost"}
              onClick={() => setActiveFolder(folder.id)}
            >
              {folder.name}
            </button>
          ))}
          <button
            type="button"
            className="btn-sm btn-ghost"
            onClick={() => {
              const name = window.prompt("Folder name");
              if (name) void createFolder(name).then(refreshNotes);
            }}
          >
            + Folder
          </button>
        </nav>

        <ul aria-label="notes" className="plain stack" style={{ gap: 2 }}>
          {visibleNotes.map((note) => {
            const status = STATUS_LABEL[note.processingStatus] ?? note.processingStatus;
            return (
              <li key={note.id}>
                <button
                  type="button"
                  className="row"
                  aria-current={note.id === activeNoteId ? "true" : undefined}
                  onClick={() => void openNote(note.id)}
                >
                  <NotesIcon className="muted" />
                  <span
                    style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  >
                    {note.title}
                  </span>
                  {status ? (
                    <span
                      className={`badge note-list-status ${
                        note.processingStatus === "failed" ? "badge-danger" : ""
                      }`}
                    >
                      {status}
                    </span>
                  ) : null}
                </button>
              </li>
            );
          })}
          {visibleNotes.length === 0 ? (
            <li className="empty">No notes yet. Press Record to capture one.</li>
          ) : null}
        </ul>
      </aside>

      <section style={{ minWidth: 0 }}>
        {openTabs.length > 0 ? (
          <div role="tablist" className="tabstrip">
            {openTabs.map((id) => (
              <span
                key={id}
                role="tab"
                className="tab"
                aria-selected={id === activeNoteId}
                tabIndex={0}
              >
                <button type="button" className="tab" onClick={() => void openNote(id)}>
                  {tabTitle(id)}
                </button>
                <button
                  type="button"
                  className="tab-close"
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
          <article className="stack">
            <input
              aria-label="note title"
              className="note-title"
              value={detail.title}
              onChange={(e) => editDetail({ title: e.target.value })}
            />
            {detail.calendarContext
              ? (() => {
                  try {
                    const ctx = JSON.parse(detail.calendarContext) as {
                      title: string;
                      attendees: string[];
                    };
                    return ctx.attendees.length > 0 ? (
                      <small>Attendees: {ctx.attendees.join(", ")}</small>
                    ) : null;
                  } catch {
                    return null;
                  }
                })()
              : null}
            {detail.processingStatus === "failed" ? (
              <div className="banner banner-danger" role="alert">
                <span>Processing failed: {detail.processingError}</span>
                <button
                  type="button"
                  className="btn-sm"
                  onClick={() => void retryProcessing(detail.id)}
                >
                  Retry
                </button>
              </div>
            ) : null}
            {["transcribing", "generating", "recording"].includes(detail.processingStatus) ? (
              <span className="badge badge-accent badge-dot">
                {STATUS_LABEL[detail.processingStatus]}
              </span>
            ) : null}
            {recording && livePreview ? (
              <blockquote aria-label="live preview" className="live-preview">
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
              />
            </label>
            <label>
              Note
              <textarea
                aria-label="note body"
                value={detail.bodyMd}
                onChange={(e) => editDetail({ bodyMd: e.target.value })}
                rows={14}
              />
            </label>
            <div className="hstack spread">
              <select
                aria-label="note folder"
                value={detail.folderId ?? ""}
                style={{ width: "auto" }}
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
                className="btn-danger btn-sm"
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
            </div>
            {turns.length > 0 ? (
              <details>
                <summary>Transcript · {turns.length} turns</summary>
                <ul aria-label="transcript turns" className="transcript">
                  {turns.map((turn) => (
                    <li key={turn.turnIndex}>
                      <span className="ts">{formatElapsed(turn.startMs)}</span>
                      <span>
                        {turn.speaker ? <strong>{turn.speaker}: </strong> : null}
                        {turn.text}
                      </span>
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </article>
        ) : (
          <div className="empty">
            <NotesIcon className="muted" />
            <p style={{ marginTop: 8 }}>Select a note, or press Record to capture one.</p>
          </div>
        )}
      </section>
    </div>
  );
}
