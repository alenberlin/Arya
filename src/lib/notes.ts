import { invoke } from "@tauri-apps/api/core";

/** A note as returned by the Rust shell. */
export interface Note {
  id: string;
  title: string;
  createdAt: string;
}

/** Creates a note and returns it. */
export function createNote(title: string): Promise<Note> {
  return invoke<Note>("create_note", { title });
}

/** Lists all notes, newest first. */
export function listNotes(): Promise<Note[]> {
  return invoke<Note[]>("list_notes");
}
