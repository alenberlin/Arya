import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  type Attachment,
  assignNoteToFolder,
  attachFile,
  createFolder,
  createNote,
  deleteNote,
  discardRecording,
  type Folder,
  finishRecording,
  getNote,
  getNoteTurns,
  listAttachments,
  listFolders,
  listNotes,
  type NoteDetail,
  type NoteSummary,
  openAttachment,
  pauseRecording,
  type RecoverableRecording,
  recordingStatus,
  recoverRecording,
  removeAttachment,
  resumeRecording,
  retryProcessing,
  scanRecoverableRecordings,
  searchNotes,
  startRecording,
  type TranscriptTurn,
  updateNote,
} from "../lib/notes";
import { ConfirmDialog, PromptDialog } from "../ui/dialogs";
import { MeetingIcon, NotesIcon, PlusIcon, RecordIcon, SearchIcon, StopIcon } from "../ui/icons";

const STATUS_LABEL: Record<string, string> = {
  idle: "",
  recording: "Recording",
  transcribing: "Transcribing…",
  generating: "Generating…",
  ready: "",
  failed: "Failed",
};

/** Short type badge from a filename (e.g. "PDF"). */
function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0
    ? name
        .slice(dot + 1)
        .toUpperCase()
        .slice(0, 4)
    : "FILE";
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

// A static set of bar heights/timings for the live recording waveform.
const WAVE = [
  { h: "70%", d: "0s", t: "0.9s" },
  { h: "100%", d: "0.1s", t: "1.1s" },
  { h: "50%", d: "0.05s", t: "0.8s" },
  { h: "85%", d: "0.15s", t: "1s" },
  { h: "40%", d: "0.2s", t: "0.95s" },
  { h: "65%", d: "0.08s", t: "1.05s" },
  { h: "90%", d: "0.12s", t: "0.85s" },
  { h: "55%", d: "0.04s", t: "1.15s" },
];

function formatElapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

function formatWhen(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function statusChip(status: string) {
  if (status === "recording") {
    return (
      <span className="status-chip status--rec note-list-status">
        <span className="dot-pulse" />
        REC
      </span>
    );
  }
  if (status === "transcribing" || status === "generating") {
    return (
      <span className="status-chip status--processing note-list-status">
        {STATUS_LABEL[status]}
      </span>
    );
  }
  if (status === "failed") {
    return <span className="status-chip status--failed note-list-status">Failed</span>;
  }
  return null;
}

/** Notes workspace: a list panel (recorder, banners, folders, notes) beside an
 * editor panel (title, live capture, note body, transcript). */
export function NotesWorkspace() {
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [activeFolder, setActiveFolder] = useState<string | null>(null);
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  const [detail, setDetail] = useState<NoteDetail | null>(null);
  const [turns, setTurns] = useState<TranscriptTurn[]>([]);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [dragActive, setDragActive] = useState(false);
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
  const [folderDialogOpen, setFolderDialogOpen] = useState(false);
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null);
  const [noteMenu, setNoteMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [dragOverFolder, setDragOverFolder] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [searchResults, setSearchResults] = useState<NoteSummary[]>([]);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeNoteRef = useRef<string | null>(null);
  activeNoteRef.current = activeNoteId;
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Close the note context menu on any click outside it, or on Escape.
  useEffect(() => {
    if (!noteMenu) return;
    const onDown = (e: MouseEvent) => {
      if (menuRef.current && e.target instanceof Node && menuRef.current.contains(e.target)) {
        return;
      }
      setNoteMenu(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setNoteMenu(null);
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [noteMenu]);

  const refreshNotes = useCallback(async () => {
    try {
      const [n, f] = await Promise.all([listNotes(), listFolders()]);
      setNotes(n);
      setFolders(f);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  /** Move a note to a folder (or out of one). Used by both drag-drop and the
   * right-click menu. Keeps an open note's detail in sync. */
  const moveNote = useCallback(
    async (noteId: string, folderId: string | null) => {
      try {
        await assignNoteToFolder(noteId, folderId);
        await refreshNotes();
        setDetail((d) => (d && d.id === noteId ? { ...d, folderId } : d));
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshNotes],
  );

  const openReqRef = useRef(0);
  const openNote = useCallback(async (id: string) => {
    // Monotonic request token: a rapid A→B click must not let A's slower
    // response overwrite B's editor.
    const req = ++openReqRef.current;
    setActiveNoteId(id);
    setOpenTabs((tabs) => (tabs.includes(id) ? tabs : [...tabs, id]));
    try {
      const [nextDetail, nextTurns, nextAttachments] = await Promise.all([
        getNote(id),
        getNoteTurns(id),
        listAttachments(id),
      ]);
      if (openReqRef.current !== req) return; // superseded by a newer open
      setDetail(nextDetail);
      setTurns(nextTurns);
      setAttachments(nextAttachments);
    } catch (e) {
      if (openReqRef.current === req) setError(String(e));
    }
  }, []);

  /** Create a blank note and open it, ready to type or dictate into (hold Right
   * Shift). Lands in "All notes" unless a folder is active. */
  const createNewNote = useCallback(async () => {
    try {
      const note = await createNote("New note");
      if (activeFolder) await assignNoteToFolder(note.id, activeFolder);
      await refreshNotes();
      await openNote(note.id);
    } catch (e) {
      setError(String(e));
    }
  }, [activeFolder, refreshNotes, openNote]);

  const refreshAttachments = useCallback(async (noteId: string) => {
    try {
      setAttachments(await listAttachments(noteId));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const attachFiles = useCallback(
    async (noteId: string, paths: string[]) => {
      for (const path of paths) {
        try {
          await attachFile(noteId, path);
        } catch (e) {
          setError(String(e));
        }
      }
      if (activeNoteRef.current === noteId) await refreshAttachments(noteId);
    },
    [refreshAttachments],
  );

  const pickAttachments = useCallback(async () => {
    const noteId = activeNoteRef.current;
    if (!noteId) return;
    try {
      const picked = await openDialog({ multiple: true, title: "Attach files" });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (paths.length > 0) await attachFiles(noteId, paths);
    } catch (e) {
      setError(String(e));
    }
  }, [attachFiles]);

  // Attach files dropped anywhere on the window to the open note.
  useEffect(() => {
    const promise = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as { type: string; paths?: string[] };
      if (payload.type === "enter" || payload.type === "over") {
        if (activeNoteRef.current) setDragActive(true);
      } else if (payload.type === "leave") {
        setDragActive(false);
      } else if (payload.type === "drop") {
        setDragActive(false);
        const noteId = activeNoteRef.current;
        if (noteId && payload.paths?.length) void attachFiles(noteId, payload.paths);
      }
    });
    return () => {
      void promise.then((unlisten) => unlisten());
    };
  }, [attachFiles]);

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

  // Debounced content filter: searches titles + bodies + transcripts server-side.
  useEffect(() => {
    const q = filter.trim();
    if (!q) {
      setSearchResults([]);
      return;
    }
    const t = setTimeout(() => {
      void searchNotes(q)
        .then(setSearchResults)
        .catch(() => {});
    }, 180);
    return () => clearTimeout(t);
  }, [filter]);

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

  const baseNotes = filter.trim() ? searchResults : notes;
  // "All notes" is the unfiled bucket: a note lives in exactly one place —
  // there, or in a single folder. Moving it to a folder removes it from here.
  const visibleNotes = baseNotes
    .filter((n) => (activeFolder ? n.folderId === activeFolder : !n.folderId))
    .slice()
    // Guarantee newest-first regardless of source ordering.
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  const tabTitle = (id: string) => notes.find((n) => n.id === id)?.title ?? "Note";
  const recording = recorder.state !== "idle";
  const source = detail && recording ? "mic + system audio" : "";

  return (
    <div className="screen">
      {/* LIST PANEL */}
      <div className="panel" style={{ width: 314, flex: "0 0 314px" }}>
        <div className="panel-head">
          <div className="spread hstack" style={{ marginBottom: 12 }}>
            <div className="panel-title">Notes</div>
            <button
              type="button"
              className="btn-icon bare"
              aria-label="new note"
              title="New note"
              onClick={() => void createNewNote()}
            >
              <PlusIcon />
            </button>
          </div>
          <div className="filter-field" style={{ marginBottom: 12 }}>
            <SearchIcon className="filter-icon" />
            <input
              aria-label="filter notes"
              placeholder="Filter notes by title or content…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
          <div className="recorder-bar">
            {!recording ? (
              <>
                <button
                  type="button"
                  className="btn-primary"
                  onClick={() => void onRecord()}
                  aria-label="record"
                >
                  <RecordIcon /> Record
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
                  className="btn-primary"
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
        </div>

        <div className="panel-body">
          {upcoming && !recording ? (
            <div className="banner banner-accent" role="status" style={{ margin: "6px 6px 12px" }}>
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
            <div className="banner banner-accent" role="status" style={{ margin: "6px 6px 12px" }}>
              <div className="hstack">
                <span className="dot-pulse" />
                <span className="banner-title">Meeting detected in {meeting.appName}</span>
              </div>
              <div className="hstack">
                <button
                  type="button"
                  className="btn-sm btn-primary"
                  onClick={() => void onRecord("microphone-and-system")}
                >
                  Record
                </button>
                <button type="button" className="btn-sm" onClick={() => setMeeting(null)}>
                  Dismiss
                </button>
              </div>
            </div>
          ) : null}

          {systemAudioWarning ? (
            <div className="banner banner-warning" role="alert" style={{ margin: "6px 6px 12px" }}>
              <span className="banner-title">System audio unavailable</span>
              <small>
                Recording microphone only. Grant "System audio recording" in System Settings for
                meeting capture.
              </small>
            </div>
          ) : null}

          {recoverables.length > 0 ? (
            <div className="banner banner-warning" role="alert" style={{ margin: "6px 6px 12px" }}>
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
                          setRecoverables((list) =>
                            list.filter((x) => x.sessionId !== r.sessionId),
                          );
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
                          setRecoverables((list) =>
                            list.filter((x) => x.sessionId !== r.sessionId),
                          );
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

          <nav aria-label="folders" className="hstack wrap" style={{ padding: "4px 8px 8px" }}>
            <button
              type="button"
              className={`${activeFolder ? "btn-sm btn-ghost" : "btn-sm btn-primary"}${
                dragOverFolder === "__all__" ? " drop-hot" : ""
              }`}
              onClick={() => setActiveFolder(null)}
              onDragOver={(e) => {
                e.preventDefault();
                setDragOverFolder("__all__");
              }}
              onDragLeave={() => setDragOverFolder((f) => (f === "__all__" ? null : f))}
              onDrop={(e) => {
                e.preventDefault();
                const id = e.dataTransfer.getData("text/plain");
                setDragOverFolder(null);
                if (id) void moveNote(id, null);
              }}
            >
              All notes
            </button>
            {folders.map((folder) => (
              <button
                key={folder.id}
                type="button"
                className={`${
                  activeFolder === folder.id ? "btn-sm btn-primary" : "btn-sm btn-ghost"
                }${dragOverFolder === folder.id ? " drop-hot" : ""}`}
                onClick={() => setActiveFolder(folder.id)}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDragOverFolder(folder.id);
                }}
                onDragLeave={() => setDragOverFolder((f) => (f === folder.id ? null : f))}
                onDrop={(e) => {
                  e.preventDefault();
                  const id = e.dataTransfer.getData("text/plain");
                  setDragOverFolder(null);
                  if (id) void moveNote(id, folder.id);
                }}
              >
                {folder.name}
              </button>
            ))}
            <button
              type="button"
              className="btn-sm btn-ghost"
              aria-label="new folder"
              title="New folder"
              style={{ paddingInline: 9 }}
              onClick={() => setFolderDialogOpen(true)}
            >
              <PlusIcon />
            </button>
          </nav>

          <ul aria-label="notes" className="plain">
            {visibleNotes.map((note) => (
              <li key={note.id}>
                <button
                  type="button"
                  className="row"
                  draggable
                  aria-current={note.id === activeNoteId ? "true" : undefined}
                  onClick={() => void openNote(note.id)}
                  onDragStart={(e) => {
                    e.dataTransfer.setData("text/plain", note.id);
                    e.dataTransfer.effectAllowed = "move";
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setNoteMenu({ id: note.id, x: e.clientX, y: e.clientY });
                  }}
                >
                  <div className="spread hstack" style={{ marginBottom: 4 }}>
                    <span className="truncate" style={{ fontSize: 13.5, fontWeight: 500 }}>
                      {note.title}
                    </span>
                    {statusChip(note.processingStatus)}
                  </div>
                  <div className="mono" style={{ fontSize: 10.5, color: "var(--text-muted)" }}>
                    {formatWhen(note.createdAt)}
                  </div>
                </button>
              </li>
            ))}
            {visibleNotes.length === 0 ? (
              <li className="empty">
                {filter.trim()
                  ? "No notes match your filter."
                  : "No notes yet. Press Record to capture one."}
              </li>
            ) : null}
          </ul>
        </div>
      </div>

      {/* EDITOR PANEL */}
      <div className="panel panel-grow">
        {openTabs.length > 0 ? (
          <div className="panel-head" style={{ paddingBottom: 8 }}>
            <div
              role="tablist"
              className="tabstrip"
              style={{ margin: 0, border: "none", padding: 0 }}
            >
              {openTabs.map((id) => (
                <span
                  key={id}
                  role="tab"
                  className="tab"
                  aria-selected={id === activeNoteId}
                  tabIndex={0}
                >
                  <button type="button" className="tab bare" onClick={() => void openNote(id)}>
                    {tabTitle(id)}
                  </button>
                  <button
                    type="button"
                    className="tab-close bare"
                    aria-label={`close ${tabTitle(id)}`}
                    onClick={() => closeTab(id)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
          </div>
        ) : null}

        {detail ? (
          <div className="panel-body" style={{ padding: "22px 28px 28px" }}>
            {error ? (
              <p role="alert" style={{ marginBottom: 12 }}>
                {error}
              </p>
            ) : null}
            {dragActive ? (
              <div
                className="banner"
                style={{
                  marginBottom: 12,
                  background: "var(--accent-ghost)",
                  color: "var(--accent)",
                }}
              >
                Drop files to attach to this note
              </div>
            ) : null}
            {recording ? (
              <div className="hstack" style={{ marginBottom: 6 }}>
                <span
                  className="status-chip status--rec"
                  style={{ background: "var(--accent-ghost)" }}
                >
                  <span className="dot-pulse" />
                  Recording
                </span>
                <span className="mono muted" style={{ fontSize: 12 }}>
                  {formatElapsed(recorder.elapsedMs)}
                  {source ? ` · ${source}` : ""}
                </span>
              </div>
            ) : null}
            <input
              aria-label="note title"
              className="note-title"
              value={detail.title}
              onChange={(e) => editDetail({ title: e.target.value })}
            />
            {recording ? (
              <div className="waveform" style={{ margin: "12px 0" }} aria-hidden="true">
                {WAVE.map((b, i) => (
                  <span
                    // biome-ignore lint/suspicious/noArrayIndexKey: fixed decorative bar set
                    key={i}
                    style={{ height: b.h, animationDelay: b.d, animationDuration: b.t }}
                  />
                ))}
              </div>
            ) : null}
            {detail.calendarContext
              ? (() => {
                  try {
                    const ctx = JSON.parse(detail.calendarContext) as {
                      title: string;
                      attendees: string[];
                    };
                    return ctx.attendees.length > 0 ? (
                      <small style={{ display: "block", marginTop: 4 }}>
                        Attendees: {ctx.attendees.join(", ")}
                      </small>
                    ) : null;
                  } catch {
                    return null;
                  }
                })()
              : null}
            {detail.processingStatus === "failed" ? (
              <div className="banner banner-danger" role="alert" style={{ marginTop: 14 }}>
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
            {recording && livePreview ? (
              <blockquote
                aria-label="live preview"
                className="live-preview"
                style={{ marginTop: 14 }}
              >
                {livePreview}
              </blockquote>
            ) : null}

            <label style={{ marginTop: 18 }}>
              Manual notes
              <textarea
                aria-label="manual notes"
                value={detail.manualNotes}
                onChange={(e) => editDetail({ manualNotes: e.target.value })}
                rows={3}
                style={{ marginTop: 6 }}
              />
            </label>
            <label style={{ marginTop: 14 }}>
              Note
              <textarea
                aria-label="note body"
                value={detail.bodyMd}
                onChange={(e) => editDetail({ bodyMd: e.target.value })}
                rows={12}
                style={{ marginTop: 6 }}
              />
            </label>

            <div style={{ marginTop: 18 }}>
              <div className="hstack spread" style={{ marginBottom: 8 }}>
                <span className="section-label">Attachments</span>
                <button type="button" className="btn-sm" onClick={() => void pickAttachments()}>
                  Attach
                </button>
              </div>
              {attachments.length > 0 ? (
                <ul aria-label="attachments" className="plain">
                  {attachments.map((a) => (
                    <li
                      key={a.id}
                      className="card-sunken hstack spread"
                      style={{ padding: "7px 10px", margin: "5px 0" }}
                    >
                      <button
                        type="button"
                        className="hstack"
                        style={{
                          gap: 8,
                          minWidth: 0,
                          background: "none",
                          border: "none",
                          cursor: "pointer",
                          padding: 0,
                          textAlign: "left",
                        }}
                        title={`Open ${a.name}`}
                        onClick={() => void openAttachment(a.id).catch((e) => setError(String(e)))}
                      >
                        <span
                          className="mono"
                          style={{
                            fontSize: 10,
                            fontWeight: 600,
                            padding: "2px 5px",
                            borderRadius: 4,
                            background: "var(--surface-sunken)",
                            color: "var(--text-secondary)",
                            flexShrink: 0,
                          }}
                        >
                          {extOf(a.name)}
                        </span>
                        <span
                          style={{
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {a.name}
                        </span>
                        <span className="mono muted" style={{ fontSize: 11, flexShrink: 0 }}>
                          {formatBytes(a.sizeBytes)}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="tab-close bare"
                        aria-label={`remove ${a.name}`}
                        onClick={() =>
                          void removeAttachment(a.id)
                            .then(() => refreshAttachments(a.noteId))
                            .catch((e) => setError(String(e)))
                        }
                      >
                        ×
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted" style={{ fontSize: 12.5, margin: 0 }}>
                  No files attached. Click Attach, or drop files onto this note.
                </p>
              )}
            </div>

            <div className="hstack spread" style={{ marginTop: 14 }}>
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
                onClick={() => setDeleteTargetId(detail.id)}
              >
                Delete note
              </button>
            </div>

            {turns.length > 0 ? (
              <details style={{ marginTop: 22 }} open>
                <summary className="section-label" style={{ cursor: "pointer", marginBottom: 14 }}>
                  Transcript · {turns.length} turns
                </summary>
                <ul aria-label="transcript turns" className="transcript">
                  {turns.map((turn) => (
                    <li key={turn.turnIndex}>
                      <span className="ts">{formatElapsed(turn.startMs)}</span>
                      <div style={{ minWidth: 0 }}>
                        {turn.speaker ? <div className="speaker">{turn.speaker}</div> : null}
                        <div className="line">{turn.text}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </div>
        ) : (
          <div className="panel-body">
            <div className="empty">
              <NotesIcon className="muted" />
              <p style={{ marginTop: 8 }}>Select a note, or press Record to capture one.</p>
            </div>
          </div>
        )}
      </div>

      <PromptDialog
        open={folderDialogOpen}
        title="New folder"
        label="Folder name"
        placeholder="e.g. Work"
        submitLabel="Create folder"
        onSubmit={(name) => {
          setFolderDialogOpen(false);
          void createFolder(name).then(refreshNotes);
        }}
        onCancel={() => setFolderDialogOpen(false)}
      />
      <ConfirmDialog
        open={deleteTargetId !== null}
        title="Delete note?"
        message="This permanently removes the note and its transcript."
        confirmLabel="Delete note"
        danger
        onConfirm={() => {
          const id = deleteTargetId;
          setDeleteTargetId(null);
          if (id) {
            void deleteNote(id).then(() => {
              closeTab(id);
              return refreshNotes();
            });
          }
        }}
        onCancel={() => setDeleteTargetId(null)}
      />

      {noteMenu ? (
        <div
          ref={menuRef}
          className="context-menu"
          style={{ top: noteMenu.y, left: noteMenu.x }}
          role="menu"
        >
          <div className="context-menu-label">Move to</div>
          <button
            type="button"
            role="menuitem"
            disabled={!notes.find((n) => n.id === noteMenu.id)?.folderId}
            onClick={() => {
              void moveNote(noteMenu.id, null);
              setNoteMenu(null);
            }}
          >
            All notes
          </button>
          {folders.map((f) => (
            <button
              key={f.id}
              type="button"
              role="menuitem"
              disabled={notes.find((n) => n.id === noteMenu.id)?.folderId === f.id}
              onClick={() => {
                void moveNote(noteMenu.id, f.id);
                setNoteMenu(null);
              }}
            >
              {f.name}
            </button>
          ))}
          {folders.length === 0 ? (
            <div className="context-menu-empty">No folders yet — add one with +</div>
          ) : null}
          <div className="context-menu-sep" />
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={() => {
              const id = noteMenu.id;
              setNoteMenu(null);
              setDeleteTargetId(id);
            }}
          >
            Delete note
          </button>
        </div>
      ) : null}
    </div>
  );
}
