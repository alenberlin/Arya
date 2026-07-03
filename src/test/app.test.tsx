import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";

// The Rust side has its own tests; the mock stands in for the shell so these
// tests prove UI wiring: commands out, state update, render.
interface MockNote {
  id: string;
  title: string;
  processingStatus: string;
  processingError: string | null;
  folderId: string | null;
  createdAt: string;
}

const backend: {
  notes: MockNote[];
  recorder: { state: string; elapsedMs: number };
  recovered: string[];
} = {
  notes: [],
  recorder: { state: "idle", elapsedMs: 0 },
  recovered: [],
};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "account_signin_state":
        return { signedIn: true, hostedAuth: false };
      case "list_notes":
        return backend.notes;
      case "list_folders":
        return [];
      case "recording_status":
        return { ...backend.recorder, level: 0, sessionId: null, noteId: null };
      case "scan_recoverable_recordings":
        return [
          {
            sessionId: "s1",
            noteId: "n1",
            noteTitle: "Crashed meeting",
            partialPath: "/tmp/x.partial.wav",
            sizeBytes: 250_000,
            startedAt: "2026-07-03T00:00:00Z",
          },
        ];
      case "start_recording": {
        backend.recorder = { state: "recording", elapsedMs: 0 };
        const note: MockNote = {
          id: "rec-note",
          title: "New recording",
          processingStatus: "recording",
          processingError: null,
          folderId: null,
          createdAt: "2026-07-03T00:00:00Z",
        };
        backend.notes = [note, ...backend.notes];
        return note.id;
      }
      case "get_note":
        return {
          id: String(args?.id),
          title: "New recording",
          bodyMd: "",
          manualNotes: "",
          processingStatus: "recording",
          processingError: null,
          folderId: null,
          createdAt: "2026-07-03T00:00:00Z",
        };
      case "get_note_turns":
        return [];
      case "recover_recording":
        backend.recovered.push(String(args?.sessionId));
        return "n1";
      default:
        return null;
    }
  }),
}));

describe("app shell", () => {
  beforeEach(() => {
    backend.notes = [];
    backend.recorder = { state: "idle", elapsedMs: 0 };
    backend.recovered = [];
    // Skip first-run onboarding for the shell tests.
    localStorage.setItem("arya-onboarded", "true");
  });

  it("shows the brand name and the notes workspace", async () => {
    render(<App />);
    // Brand appears in the sidebar; the Notes pillar is the active nav item.
    expect(await screen.findByRole("button", { name: "record" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Notes" })).toBeInTheDocument();
  });

  it("starts a recording and opens its note", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "record" }));
    await waitFor(() => {
      expect(screen.getByLabelText("note title")).toHaveValue("New recording");
    });
  });

  it("offers recovery for interrupted recordings and recovers on click", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByText(/Interrupted recording found/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Recover" }));
    await waitFor(() => {
      expect(backend.recovered).toEqual(["s1"]);
    });
  });
});
