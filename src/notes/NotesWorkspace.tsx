import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { type NodeKind, reconcileLinks } from "../lib/links";
import {
  type Attachment,
  assignNoteToFolder,
  attachFile,
  createFolder,
  createNote,
  deleteAllNotes,
  deleteNote,
  discardRecording,
  type Folder,
  finishRecording,
  getNote,
  getNoteTurns,
  importNotion,
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
  setNoteParent,
  startRecording,
  type TranscriptTurn,
  updateNote,
} from "../lib/notes";
import { aiTransform } from "../lib/transform";
import { ConfirmDialog, PromptDialog, TypeToConfirmDialog } from "../ui/dialogs";
import {
  MeetingIcon,
  MoreIcon,
  NotesIcon,
  PaperclipIcon,
  PlusIcon,
  RecordIcon,
  SearchIcon,
  StopIcon,
  TrashIcon,
} from "../ui/icons";
import { BacklinksPanel } from "./BacklinksPanel";
import { BlockEditor, type MentionItem } from "./BlockEditor";
import type { MentionTarget } from "./blockDocument";
import { NoteBanners } from "./NoteBanners";
import "./notes-chrome.css";

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

/** F16: turn a messy, multi-topic brain dump into coherent, grouped sections. */
const SORT_INSTRUCTION =
  "Reorganize this text into a few coherent, well-written sections, each focused " +
  "on a single topic. Group related points together and give each section a short " +
  "heading. Preserve every idea, but do not add any information that is not already " +
  "in the text.";

/** Auto-title: name an untitled note from its content once there's enough to
 * go on. Never invents facts — just names what's already there. */
const AUTO_TITLE_INSTRUCTION =
  "Read this note and reply with ONLY a short, specific title for it (3-7 " +
  "words) that captures what it's actually about. No quotes, no punctuation " +
  "at the end, no preamble, no explanation — just the title text itself.";
/** Below this many characters of body markdown, there isn't enough context
 * yet for a meaningful title — wait for more before asking the model. */
const AUTO_TITLE_MIN_CHARS = 24;
const AUTO_TITLE_DEBOUNCE_MS = 2500;

/** Strip quoting/trailing punctuation a model tends to add despite being
 * asked not to, and cap length so a rambling reply can't become the title. */
function sanitizeAutoTitle(raw: string): string {
  const cleaned = raw
    .trim()
    .replace(/^["'“”‘’]+|["'“”‘’]+$/g, "")
    .replace(/[.!?]+$/, "")
    .replace(/\s+/g, " ")
    .trim();
  return cleaned.length > 80 ? `${cleaned.slice(0, 79)}…` : cleaned;
}

/** Notes workspace: a list panel (recorder, banners, folders, notes) beside an
 * editor panel (title, live capture, note body, transcript). */
/** When another surface (e.g. Galaxy) asks to open a specific note, App passes
 * its id here; the workspace opens it on mount/change, then clears the request. */
export function NotesWorkspace({
  openNoteId,
  onOpenConsumed,
}: {
  openNoteId?: string | null;
  onOpenConsumed?: () => void;
} = {}) {
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
  const [clearAllOpen, setClearAllOpen] = useState(false);
  const [noteMenu, setNoteMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [dragOverFolder, setDragOverFolder] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [searchResults, setSearchResults] = useState<NoteSummary[]>([]);
  const [sortPreview, setSortPreview] = useState<string | null>(null);
  const [sorting, setSorting] = useState(false);
  const [editorEpoch, setEditorEpoch] = useState(0);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [notice, setNotice] = useState<string | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeNoteRef = useRef<string | null>(null);
  activeNoteRef.current = activeNoteId;
  const savedDetailRef = useRef<NoteDetail | null>(null);
  const editRevisionRef = useRef(0);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const mentionsRef = useRef<MentionTarget[]>([]);
  // Mirrors `detail` so the debounced auto-title callback below can read the
  // truly-current title/id when it fires, not a stale render-time closure.
  const detailRef = useRef<NoteDetail | null>(null);
  detailRef.current = detail;
  const autoTitleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

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
        if (savedDetailRef.current?.id === noteId) {
          savedDetailRef.current = { ...savedDetailRef.current, folderId };
        }
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
    setSaveState("idle");
    setOpenTabs((tabs) => (tabs.includes(id) ? tabs : [...tabs, id]));
    try {
      const [nextDetail, nextTurns, nextAttachments] = await Promise.all([
        getNote(id),
        getNoteTurns(id),
        listAttachments(id),
      ]);
      if (openReqRef.current !== req) return; // superseded by a newer open
      savedDetailRef.current = nextDetail;
      setDetail(nextDetail);
      setTurns(nextTurns);
      setAttachments(nextAttachments);
    } catch (e) {
      if (openReqRef.current === req) setError(String(e));
    }
  }, []);

  // Honour an external open request (e.g. Galaxy's "Open"), then clear it.
  useEffect(() => {
    if (!openNoteId) return;
    void openNote(openNoteId);
    onOpenConsumed?.();
  }, [openNoteId, openNote, onOpenConsumed]);

  /** F15: resolve a mentioned node's text and apply the instruction to it. Only
   * notes are resolvable today; returns "" (insert nothing) on any failure. */
  const runInlineCommand = useCallback(
    async (mention: { kind: string; id: string; label: string }, instruction: string) => {
      try {
        if (mention.kind !== "note") return "";
        const target = await getNote(mention.id);
        const source = target.bodyMd.trim();
        if (!source) {
          setError(`"${mention.label || "That note"}" has no text to use yet.`);
          return "";
        }
        return await aiTransform(source, instruction);
      } catch (e) {
        setError(String(e));
        return "";
      }
    },
    [],
  );

  /** Create a new page nested under `parentId`, open it, and expand the parent. */
  const addSubPage = useCallback(
    async (parentId: string) => {
      try {
        const note = await createNote("New note", parentId);
        await refreshNotes();
        setExpanded((s) => new Set(s).add(parentId));
        await openNote(note.id);
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshNotes, openNote],
  );

  /** Move a nested note back to the top level (F3). */
  const moveToTopLevel = useCallback(
    async (noteId: string) => {
      try {
        await setNoteParent(noteId, null);
        await refreshNotes();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshNotes],
  );

  /** Import an unzipped Notion export folder as a page tree (F4). */
  const importFromNotion = useCallback(async () => {
    try {
      const picked = await openDialog({
        directory: true,
        title: "Choose your unzipped Notion export folder",
      });
      if (!picked || Array.isArray(picked)) return;
      setNotice("Importing from Notion…");
      const report = await importNotion(picked);
      await refreshNotes();
      const pages = `${report.pagesCreated} page${report.pagesCreated === 1 ? "" : "s"}`;
      const links = report.linksResolved
        ? `, ${report.linksResolved} link${report.linksResolved === 1 ? "" : "s"}`
        : "";
      const skipped = report.skipped ? ` (${report.skipped} skipped)` : "";
      setNotice(`Imported ${pages} from Notion${links}${skipped}.`);
    } catch (e) {
      setNotice(null);
      setError(String(e));
    }
  }, [refreshNotes]);

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
      savedDetailRef.current = null;
      setTurns([]);
    }
  };

  const editDetail = (
    fields: Partial<Pick<NoteDetail, "title" | "bodyMd" | "manualNotes" | "documentJson">>,
  ) => {
    if (!detail) return;
    const next = { ...detail, ...fields };
    const revision = ++editRevisionRef.current;
    setDetail(next);
    setSaveState("saving");
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      void updateNote(next.id, {
        title: next.title,
        bodyMd: next.bodyMd,
        manualNotes: next.manualNotes,
        documentJson: next.documentJson,
      })
        .then(async () => {
          if (revision === editRevisionRef.current) setSaveState("saved");
          // Reconcile the note's mention edges only when the body changed. A
          // link-graph failure must never roll back the saved note, so it's
          // best-effort (matches the fault-isolated reconcile on the Rust side).
          if (fields.documentJson !== undefined) {
            await reconcileLinks(
              "note",
              next.id,
              mentionsRef.current.map((m) => ({ kind: m.kind as NodeKind, id: m.id })),
            ).catch(() => {});
          }
          if (activeNoteRef.current === next.id) {
            savedDetailRef.current = next;
            if (revision === editRevisionRef.current) setError(null);
          }
          await refreshNotes();
        })
        .catch((e) => {
          const saved = savedDetailRef.current;
          if (
            revision === editRevisionRef.current &&
            activeNoteRef.current === next.id &&
            saved?.id === next.id
          ) {
            setDetail(saved);
          }
          if (revision === editRevisionRef.current) setSaveState("idle");
          setError(String(e));
        });
    }, 600);
  };
  // Mirrors `editDetail` so a callback that was frozen (via useCallback with
  // stable deps) long before this render can still reach the CURRENT
  // implementation at fire time, instead of a stale one whose own closed-over
  // `detail` is permanently the value from whenever that callback was first
  // created (e.g. null, on the component's very first render).
  const editDetailRef = useRef(editDetail);
  editDetailRef.current = editDetail;

  /** F-auto-title: once an untitled note has enough body content, ask the
   * (local by default) model for a short title — debounced so it fires once
   * typing pauses, not on every keystroke, and only while the title is still
   * exactly the unedited "New note" default. Re-checks both the note id and
   * the title at fire time (via `detailRef`) so a note switch, a manual
   * rename, or a prior auto-title landing first never gets clobbered by a
   * late response. Applies the result through `editDetailRef.current` rather
   * than `editDetail` directly: this callback is created once (stable `[]`
   * deps, so the debounce timer survives re-renders) and would otherwise
   * permanently hold the `editDetail` closure from whichever render first
   * created it — on first mount that closure's own `detail` is `null`
   * forever, so every apply would silently no-op via editDetail's own early
   * `if (!detail) return`. Reading through the ref always gets the current
   * implementation instead. */
  const scheduleAutoTitle = useCallback((noteId: string, bodyMd: string) => {
    if (autoTitleTimer.current) clearTimeout(autoTitleTimer.current);
    if (bodyMd.trim().length < AUTO_TITLE_MIN_CHARS) return;
    autoTitleTimer.current = setTimeout(() => {
      void aiTransform(bodyMd, AUTO_TITLE_INSTRUCTION)
        .then((result) => {
          const title = sanitizeAutoTitle(result);
          const current = detailRef.current;
          if (title && current?.id === noteId && current.title === "New note") {
            editDetailRef.current({ title });
          }
        })
        .catch(() => {
          // Silent: this is a background nicety, not a user-initiated action.
          // Ollama being unavailable should never surface as a note error.
        });
    }, AUTO_TITLE_DEBOUNCE_MS);
  }, []);

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

  // Nodes offered in the editor's `@`-mention menu (notes for now; other node
  // kinds join as those surfaces land). The open note can't mention itself.
  const mentionItems: MentionItem[] = notes
    .filter((n) => n.id !== detail?.id)
    .map((n) => ({ kind: "note" as const, id: n.id, label: n.title }));

  const baseNotes = filter.trim() ? searchResults : notes;
  // "All notes" is the unfiled bucket: a note lives in exactly one place —
  // there, or in a single folder. Moving it to a folder removes it from here.
  const visibleNotes = baseNotes
    .filter((n) => (activeFolder ? n.folderId === activeFolder : !n.folderId))
    .slice()
    // Guarantee newest-first regardless of source ordering.
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt));

  // F3 nesting: a page tree for browse mode (no filter). Roots are the
  // folder-visible notes with no visible parent; children nest beneath them
  // (from the full list, so a child shows even if its folder differs).
  const treeMode = !filter.trim();
  const noteIds = new Set(notes.map((n) => n.id));
  const childrenByParent = new Map<string, NoteSummary[]>();
  for (const n of notes) {
    if (n.parentNoteId && noteIds.has(n.parentNoteId)) {
      const siblings = childrenByParent.get(n.parentNoteId) ?? [];
      siblings.push(n);
      childrenByParent.set(n.parentNoteId, siblings);
    }
  }
  const rootNotes = visibleNotes.filter((n) => !n.parentNoteId || !noteIds.has(n.parentNoteId));

  const toggleExpand = (id: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const renderNoteRow = (note: NoteSummary, depth: number) => {
    const kids = childrenByParent.get(note.id) ?? [];
    const isOpen = expanded.has(note.id);
    return (
      <li
        key={note.id}
        className="note-row"
        style={treeMode ? { paddingLeft: 8 + depth * 14 } : undefined}
      >
        {treeMode ? (
          kids.length > 0 ? (
            <button
              type="button"
              className="note-twisty"
              aria-label={`${isOpen ? "Collapse" : "Expand"} ${note.title}`}
              aria-expanded={isOpen}
              onClick={() => toggleExpand(note.id)}
            >
              {isOpen ? "▾" : "▸"}
            </button>
          ) : (
            <span className="note-twisty note-twisty-empty" aria-hidden="true" />
          )
        ) : null}
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
        <button
          type="button"
          className="note-del"
          aria-label={`delete ${note.title}`}
          title="Delete note"
          onClick={(e) => {
            e.stopPropagation();
            setDeleteTargetId(note.id);
          }}
        >
          <TrashIcon />
        </button>
      </li>
    );
  };

  const renderTree = (items: NoteSummary[], depth: number): React.JSX.Element[] =>
    items.flatMap((note) => {
      const rows = [renderNoteRow(note, depth)];
      if (expanded.has(note.id) && (childrenByParent.get(note.id)?.length ?? 0) > 0) {
        rows.push(...renderTree(childrenByParent.get(note.id) ?? [], depth + 1));
      }
      return rows;
    });

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
            <div className="hstack" style={{ gap: 2 }}>
              {notes.length > 0 ? (
                <button
                  type="button"
                  className="btn-icon bare note-del-all"
                  aria-label="delete all notes"
                  title="Delete all notes"
                  onClick={() => setClearAllOpen(true)}
                >
                  <TrashIcon />
                </button>
              ) : null}
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
          <NoteBanners
            upcoming={upcoming}
            meeting={meeting}
            systemAudioWarning={systemAudioWarning}
            recoverables={recoverables}
            recording={recording}
            onRecordMeeting={() => void onRecord("microphone-and-system")}
            onDismissUpcoming={() => setUpcoming(null)}
            onDismissMeeting={() => setMeeting(null)}
            onRecover={(sessionId) => {
              void recoverRecording(sessionId).then(() => {
                setRecoverables((list) => list.filter((x) => x.sessionId !== sessionId));
                return refreshNotes();
              });
            }}
            onDiscard={(sessionId) => {
              void discardRecording(sessionId).then(() => {
                setRecoverables((list) => list.filter((x) => x.sessionId !== sessionId));
              });
            }}
          />

          {notice ? (
            <div
              className="banner"
              style={{
                margin: "0 8px 8px",
                background: "var(--accent-ghost)",
                color: "var(--accent)",
              }}
            >
              <span style={{ flex: 1 }}>{notice}</span>
              <button
                type="button"
                className="tab-close bare"
                aria-label="dismiss"
                onClick={() => setNotice(null)}
              >
                ×
              </button>
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
            <button
              type="button"
              className="btn-sm btn-ghost"
              title="Import an unzipped Notion export folder"
              style={{ marginLeft: "auto" }}
              onClick={() => void importFromNotion()}
            >
              Import
            </button>
          </nav>

          <ul aria-label="notes" className="plain">
            {treeMode
              ? renderTree(rootNotes, 0)
              : visibleNotes.map((note) => renderNoteRow(note, 0))}
            {(treeMode ? rootNotes : visibleNotes).length === 0 ? (
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
          <div
            className="panel-body note-doc"
            style={{ padding: "22px 28px 28px", maxWidth: 760, width: "100%", margin: "0 auto" }}
          >
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
            <div className="note-detail-head">
              <input
                aria-label="note title"
                className="note-title"
                value={detail.title}
                onChange={(e) => editDetail({ title: e.target.value })}
              />
              {saveState !== "idle" ? (
                <span className="note-savechip" data-state={saveState}>
                  {saveState === "saving" ? "Saving…" : "Saved"}
                </span>
              ) : null}
              <button
                type="button"
                className="btn-icon bare note-more-btn"
                aria-label="Note options"
                title="Folder, delete, and more"
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  setNoteMenu({ id: detail.id, x: r.right - 210, y: r.bottom + 4 });
                }}
              >
                <MoreIcon />
              </button>
            </div>
            <div className="note-meta">
              {[
                detail.folderId ? folders.find((f) => f.id === detail.folderId)?.name : null,
                formatWhen(detail.createdAt),
              ]
                .filter(Boolean)
                .join(" · ") || "Draft"}
            </div>
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

            {/* The note is one document. The manual-notes capture only appears
                while recording — that's the only time you jot alongside a live
                transcript; afterwards it's folded into the note during
                processing, so a permanent second field just confused things. */}
            {recording ? (
              <label style={{ marginTop: 18 }}>
                Notes while recording
                <textarea
                  aria-label="manual notes"
                  value={detail.manualNotes}
                  onChange={(e) => editDetail({ manualNotes: e.target.value })}
                  rows={3}
                  style={{ marginTop: 6 }}
                />
              </label>
            ) : null}
            <div className="note-editor-field" style={{ marginTop: 14 }}>
              {detail.bodyMd.trim() ? (
                <div className="hstack" style={{ justifyContent: "flex-end", marginBottom: 6 }}>
                  <button
                    type="button"
                    className="btn-sm btn-ghost"
                    disabled={sorting}
                    title="Reorganize this note into coherent sections"
                    onClick={() => {
                      if (sorting) return;
                      setSorting(true);
                      void aiTransform(detail.bodyMd, SORT_INSTRUCTION)
                        .then(setSortPreview)
                        .catch((e) => setError(String(e)))
                        .finally(() => setSorting(false));
                    }}
                  >
                    {sorting ? "Sorting…" : "Sort"}
                  </button>
                </div>
              ) : null}
              <BlockEditor
                key={`${detail.id}:${editorEpoch}`}
                initialDocumentJson={detail.documentJson}
                initialBodyMd={detail.bodyMd}
                mentionItems={mentionItems}
                onChange={(documentJson, bodyMd, mentions) => {
                  mentionsRef.current = mentions;
                  editDetail({ documentJson, bodyMd });
                  if (detail.title === "New note") scheduleAutoTitle(detail.id, bodyMd);
                }}
                onOpenNode={(kind, id) => {
                  if (kind === "note") void openNote(id);
                }}
                onInlineCommand={runInlineCommand}
              />
            </div>
            <BacklinksPanel
              noteId={detail.id}
              notes={notes}
              onOpen={(kind, id) => {
                if (kind === "note") void openNote(id);
              }}
            />

            {attachments.length > 0 ? (
              <ul aria-label="attachments" className="plain" style={{ marginTop: 18 }}>
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
            ) : null}
            <div className="note-attach-row">
              <button
                type="button"
                className="btn-icon bare note-attach-btn"
                aria-label="Attach a file"
                title="Attach a file (or drop one onto this note)"
                onClick={() => void pickAttachments()}
              >
                <PaperclipIcon />
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
            <div className="empty note-empty">
              <span className="note-empty-icon">
                <NotesIcon />
              </span>
              <div className="note-empty-title">Your notes live here</div>
              <p>Select a note from the list, press Record to capture one, or start typing.</p>
              <button
                type="button"
                className="btn-sm btn-primary"
                onClick={() => void createNewNote()}
              >
                New note
              </button>
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
      <TypeToConfirmDialog
        open={clearAllOpen}
        title="Delete all notes?"
        message="This permanently deletes every note, along with its transcript and attachments. This cannot be undone."
        phrase="confirm"
        confirmLabel="Delete all notes"
        onConfirm={() => {
          setClearAllOpen(false);
          void deleteAllNotes()
            .then(() => {
              // Everything the editor and list referenced is gone — reset it,
              // and drop any active filter so the list can't show stale hits.
              setOpenTabs([]);
              setActiveNoteId(null);
              setDetail(null);
              savedDetailRef.current = null;
              setTurns([]);
              setAttachments([]);
              setFilter("");
              setSearchResults([]);
              return refreshNotes();
            })
            .catch((e) => setError(String(e)));
        }}
        onCancel={() => setClearAllOpen(false)}
      />

      {sortPreview !== null ? (
        <div role="dialog" aria-label="sorted note preview" className="modal-overlay">
          <div className="panel modal-card">
            <div className="panel-title" style={{ marginBottom: 4 }}>
              Sorted note
            </div>
            <p className="muted" style={{ margin: "0 0 12px", fontSize: 13 }}>
              A reorganized version of your notes, grouped into coherent sections. Accept to replace
              the note, or discard.
            </p>
            <pre className="sort-preview">{sortPreview}</pre>
            <div className="hstack spread" style={{ marginTop: 14 }}>
              <button
                type="button"
                className="btn-sm btn-ghost"
                onClick={() => setSortPreview(null)}
              >
                Discard
              </button>
              <button
                type="button"
                className="btn-primary"
                onClick={() => {
                  const sorted = sortPreview;
                  setDetail((d) => (d ? { ...d, documentJson: "", bodyMd: sorted } : d));
                  setEditorEpoch((e) => e + 1);
                  setSortPreview(null);
                }}
              >
                Accept &amp; replace
              </button>
            </div>
          </div>
        </div>
      ) : null}

      {noteMenu ? (
        <div
          ref={menuRef}
          className="context-menu"
          style={{ top: noteMenu.y, left: noteMenu.x }}
          role="menu"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              const id = noteMenu.id;
              setNoteMenu(null);
              void addSubPage(id);
            }}
          >
            Add sub-page
          </button>
          {notes.find((n) => n.id === noteMenu.id)?.parentNoteId ? (
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                const id = noteMenu.id;
                setNoteMenu(null);
                void moveToTopLevel(id);
              }}
            >
              Move to top level
            </button>
          ) : null}
          <div className="context-menu-sep" />
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
