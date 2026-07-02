import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";

// The Rust side has its own tests; here the mock stands in for the shell so
// the test proves the UI wiring: command out, state update, render.
const backend: { notes: Array<{ id: string; title: string; createdAt: string }> } = { notes: [] };

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "list_notes") return [...backend.notes].reverse();
    if (cmd === "create_note") {
      const note = {
        id: `id-${backend.notes.length + 1}`,
        title: String(args?.title),
        createdAt: "2026-07-03T00:00:00Z",
      };
      backend.notes.push(note);
      return note;
    }
    throw new Error(`unexpected command ${cmd}`);
  }),
}));

describe("walking skeleton", () => {
  beforeEach(() => {
    backend.notes = [];
  });

  it("shows the brand name", async () => {
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Arya" })).toBeInTheDocument();
  });

  it("creates a note via the shell and renders it", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: "New note" }));
    await waitFor(() => {
      expect(screen.getByRole("list", { name: "notes" })).toHaveTextContent("New note");
    });
  });
});
