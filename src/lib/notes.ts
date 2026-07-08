import { invoke } from "@tauri-apps/api/core";

/** A note as returned by create_note. */
export interface Note {
  id: string;
  title: string;
  createdAt: string;
}

export interface NoteSummary {
  id: string;
  title: string;
  processingStatus: string;
  processingError: string | null;
  folderId: string | null;
  /** Parent page, or null for a top-level note (F3 nesting). */
  parentNoteId: string | null;
  createdAt: string;
}

export interface NoteDetail {
  id: string;
  title: string;
  bodyMd: string;
  /** BlockNote block-JSON (the editor's source of truth); empty for legacy notes. */
  documentJson: string;
  manualNotes: string;
  processingStatus: string;
  processingError: string | null;
  folderId: string | null;
  calendarContext: string | null;
  createdAt: string;
}

export interface TranscriptTurn {
  turnIndex: number;
  source: string;
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
}

export interface Folder {
  id: string;
  name: string;
  createdAt: string;
}

export interface RecorderStatus {
  state: "idle" | "recording" | "paused";
  elapsedMs: number;
  level: number;
  sessionId: string | null;
  noteId: string | null;
}

export interface RecoverableRecording {
  sessionId: string;
  noteId: string;
  noteTitle: string;
  partialPath: string;
  sizeBytes: number;
  startedAt: string;
}

export const createNote = (title: string, parentId?: string) =>
  invoke<Note>("create_note", { title, parentId: parentId ?? null });
/** Re-parent a note, or move it to top level with `parentId = null` (F3). */
export const setNoteParent = (noteId: string, parentId: string | null) =>
  invoke<void>("set_note_parent", { noteId, parentId });
export const listNotes = () => invoke<NoteSummary[]>("list_notes");
export const searchNotes = (query: string) => invoke<NoteSummary[]>("search_notes", { query });
export const getNote = (id: string) => invoke<NoteDetail>("get_note", { id });
export const getNoteTurns = (id: string) => invoke<TranscriptTurn[]>("get_note_turns", { id });
export const updateNote = (
  id: string,
  fields: { title?: string; bodyMd?: string; manualNotes?: string; documentJson?: string },
) =>
  invoke<void>("update_note", {
    id,
    title: fields.title ?? null,
    bodyMd: fields.bodyMd ?? null,
    manualNotes: fields.manualNotes ?? null,
    documentJson: fields.documentJson ?? null,
  });
export const deleteNote = (id: string) => invoke<void>("delete_note", { id });
/** Permanently deletes every note and its recordings/attachments. */
export const deleteAllNotes = () => invoke<void>("delete_all_notes");

/** A file attached to a note, copied into the app workspace. */
export interface Attachment {
  id: string;
  noteId: string;
  name: string;
  path: string;
  sizeBytes: number;
  createdAt: string;
}
export const attachFile = (noteId: string, sourcePath: string) =>
  invoke<Attachment>("attach_file", { noteId, sourcePath });
export const listAttachments = (noteId: string) =>
  invoke<Attachment[]>("list_attachments", { noteId });
export const removeAttachment = (id: string) => invoke<void>("remove_attachment", { id });
export const openAttachment = (id: string) => invoke<void>("open_attachment", { id });

/** Result of importing a Notion export folder (F4). */
export interface ImportReport {
  pagesCreated: number;
  linksResolved: number;
  skipped: number;
}
/** Import an unzipped Notion "Markdown & CSV" export folder as a page tree. */
export const importNotion = (dirPath: string) => invoke<ImportReport>("import_notion", { dirPath });

export const createFolder = (name: string) => invoke<Folder>("create_folder", { name });
export const listFolders = () => invoke<Folder[]>("list_folders");
export const deleteFolder = (id: string) => invoke<void>("delete_folder", { id });
export const assignNoteToFolder = (noteId: string, folderId: string | null) =>
  invoke<void>("assign_note_to_folder", { noteId, folderId });

export type SourceMode = "microphone-only" | "microphone-and-system";
export const startRecording = (noteId?: string, sourceMode?: SourceMode) =>
  invoke<string>("start_recording", {
    noteId: noteId ?? null,
    sourceMode: sourceMode ?? null,
  });
export const pauseRecording = () => invoke<void>("pause_recording");
export const resumeRecording = () => invoke<void>("resume_recording");
export const finishRecording = () => invoke<string>("finish_recording");
export const recordingStatus = () => invoke<RecorderStatus>("recording_status");
export const retryProcessing = (noteId: string) => invoke<void>("retry_processing", { noteId });
export const scanRecoverableRecordings = () =>
  invoke<RecoverableRecording[]>("scan_recoverable_recordings");
export const recoverRecording = (sessionId: string) =>
  invoke<string>("recover_recording", { sessionId });
export const discardRecording = (sessionId: string) =>
  invoke<void>("discard_recording", { sessionId });
