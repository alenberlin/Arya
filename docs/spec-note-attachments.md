# Spec — Note attachments

Status: **LOCKED (2026-07-05).** Build order: first (before dictation translation).

## 1. Goal

Attach arbitrary files to a note — CSV, PDF, image, Word, spreadsheet,
PowerPoint, anything. Files are **copied into the app's workspace** so the note
stays self-contained, and you can open them (in the OS default app) or remove
them.

Your words: *"when I manually create a new note … have the option to attach a
file — CSV, PDF, image, Word document, spreadsheet, PowerPoint."*

## 2. Non-goals (v1)

- **Text extraction / RAG indexing of attachment contents** (searching inside a
  PDF/docx/xlsx). High-value, but per-format extraction is a real scope — a
  fast-follow (§9).
- **In-app preview/rendering** of attachments (v1 opens them in the OS default
  app).
- Editing attachments, versioning, or thumbnails.
- Attaching to dictations or agent sessions (notes only for v1).

## 3. UX

- In the **note editor** (`NotesWorkspace`), an **"Attach"** button reveals a
  native file picker (multi-select).
- Picked files are copied into the workspace and shown as a list of rows: a
  **type icon** (by extension), **filename**, and **size**, each with **open**
  and **remove**.
- Clicking a row **opens** the file in the OS default app.
- Works on **any** note (manually created or recorded).
- **Drag-and-drop** files onto the note also attaches them (in v1).

## 4. Architecture

**New dependency:** `tauri-plugin-dialog` (v2) for the native open dialog (the
`tauri-plugin-opener` already in the tree handles opening). Add its capability
permission to `capabilities/default.json`.

**Storage:** copy each picked file to
```
<app_data>/attachments/<note_id>/<uuid>-<original_name>
```
so filenames never collide and the note owns its files. This mirrors how
recordings are stored and cleaned up today.

**Data model** — migration `000N_note_attachments.sql`:
```
CREATE TABLE note_attachments (
    id          TEXT PRIMARY KEY,
    note_id     TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,          -- original filename (with extension)
    path        TEXT NOT NULL,          -- absolute stored path in the workspace
    size_bytes  INTEGER NOT NULL,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_note_attachments_note ON note_attachments(note_id);
```
Foreign-key cascade removes the *rows* on note delete; the *files* are removed
explicitly (see below), mirroring `delete_note`'s existing audio-artifact
cleanup.

**Backend commands** (`notes.rs` or a new `attachments.rs`):
- `attach_file(note_id, source_path) -> Attachment` — copy into the workspace,
  insert the row, return it. (The frontend picks paths via the dialog plugin and
  calls this per file.)
- `list_attachments(note_id) -> Vec<Attachment>`.
- `remove_attachment(id)` — delete the row and the file.
- `open_attachment(id)` — resolve the stored path and open via `opener` (keeps
  path handling in Rust).
- Extend `delete_note` to also remove the note's attachment files from disk.

**Frontend:**
- `lib/notes.ts`: an `Attachment` type + wrappers; picks files with
  `@tauri-apps/plugin-dialog`'s `open({ multiple: true })`.
- `NotesWorkspace`: an **Attachments** section in the editor — the Attach button
  and the list (icon by extension → open / remove).

## 5. Flow

```
Attach button → dialog.open({multiple:true}) → [paths]
  → for each path: attach_file(noteId, path)   (Rust copies + inserts)
  → refresh list
row click     → open_attachment(id)            (OS default app)
remove        → remove_attachment(id) → refresh
note delete   → cascade rows + delete files
```

## 6. Decisions to lock (my recommendation in **bold**)

1. **Copy into the workspace** (self-contained, survives moving/deleting the
   original) vs. reference the original path. → **Lock: copy.**
2. **Location** `<app_data>/attachments/<note_id>/`. → **Lock.**
3. **Types & size:** allow **any** file type; **no hard size limit** v1 (maybe a
   soft warning above ~100 MB later). → **Lock: any type, no hard cap.**
4. **Open** in the OS default app via `opener` (no in-app preview v1). → **Lock.**
5. **RAG indexing of contents = out of v1**, fast-follow. → **Lock: out of v1.**
6. **LOCKED — drag-and-drop is in v1** (in addition to the Attach button), via
   Tauri's file-drop events.
7. **Scope of "which notes":** **any** note, not only manual ones. → **Lock: any
   note.**

## 7. Edge cases & failure handling

- Duplicate filenames → the `<uuid>-` prefix prevents collisions.
- Source file unreadable/missing at attach time → command returns an error; the
  UI surfaces it; nothing is inserted.
- Note deleted → attachment rows cascade and files are removed.
- File deleted from disk out-of-band → `open_attachment` surfaces a clear error;
  `remove_attachment` still clears the row.

## 8. Privacy / security

Files live in the app's local data directory; nothing leaves the machine.
Opening delegates to the OS. Single-user, local — consistent with the product.

## 9. Fast-follow: make attachments searchable (out of v1)

The payoff extension: extract text from PDF/docx/xlsx/pptx and feed it into the
existing local **RAG** index, so "search everything you've captured" covers
attachment contents too. This needs per-format extractors and is its own scoped
effort — deliberately **not** in v1.

## 10. Verification plan

- **Unit (backend, `test_pool` + temp files):** attach copies + inserts; list
  returns it; remove deletes row + file; **note delete removes the files**.
- **Frontend:** the attachments list renders; type icons by extension.
- **Manual:** pick a real CSV/PDF/image, confirm it opens and persists across
  restarts.

## 11. Implementation slices (each verified)

1. **Backend** — add `tauri-plugin-dialog` + capability; `note_attachments`
   migration; `attach_file` / `list_attachments` / `remove_attachment` /
   `open_attachment`; extend `delete_note` cleanup; register commands. Tests.
2. **Frontend** — `lib/notes.ts` wrappers + `Attachment` type; the Attachments
   section in `NotesWorkspace` (Attach button, list, open, remove).
3. **Drag-and-drop** — Tauri file-drop onto the note attaches files.
4. **Fast-follows (not v1):** RAG indexing of contents; inline preview/thumbnails.
