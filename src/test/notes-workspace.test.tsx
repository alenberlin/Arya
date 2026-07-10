import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { NotesWorkspace } from "../notes/NotesWorkspace";

const noteDetail = {
  id: "note-1",
  title: "Saved note",
  bodyMd: "Persisted body",
  documentJson: "",
  manualNotes: "Persisted manual notes",
  processingStatus: "ready",
  processingError: null,
  folderId: null,
  calendarContext: null,
  createdAt: "2026-07-03T00:00:00Z",
};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn(async () => () => {}),
  }),
}));

// The block editor is a heavy third-party component with its own coverage
// (BlockEditor's parseInitialContent test + build). These tests exercise the
// workspace's list/open/save logic, so stub it out to keep them focused.
vi.mock("../notes/BlockEditor", () => ({ BlockEditor: () => null }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    switch (cmd) {
      case "list_notes":
        return [
          {
            id: noteDetail.id,
            title: noteDetail.title,
            processingStatus: noteDetail.processingStatus,
            processingError: null,
            folderId: null,
            createdAt: noteDetail.createdAt,
          },
        ];
      case "list_folders":
      case "get_note_turns":
      case "list_attachments":
      case "list_links_to":
      case "scan_recoverable_recordings":
        return [];
      case "recording_status":
        return { state: "idle", elapsedMs: 0, level: 0, sessionId: null, noteId: null };
      case "get_note":
        return { ...noteDetail };
      case "update_note":
        throw new Error("disk full");
      default:
        return null;
    }
  }),
}));

describe("NotesWorkspace", () => {
  it("rolls back optimistic note edits when the save fails", async () => {
    const user = userEvent.setup();
    render(<NotesWorkspace />);

    const notesList = await screen.findByRole("list", { name: "notes" });
    await user.click(within(notesList).getAllByRole("button", { name: /Saved note/ })[0]);
    // Rollback is shared across every edited field (editDetail); assert it via
    // the title, which is always present (the manual-notes capture now only
    // shows while recording, and the body moved to the block editor).
    const title = await screen.findByLabelText("note title");
    expect(title).toHaveValue("Saved note");

    await user.clear(title);
    await user.type(title, "Unsaved title");
    expect(title).toHaveValue("Unsaved title");

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("disk full"));
    expect(title).toHaveValue("Saved note");
  });
});
