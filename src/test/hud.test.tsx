import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HudApp } from "../hud/HudApp";

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    handlers.set(name, handler);
    return () => handlers.delete(name);
  }),
}));

describe("dictation HUD", () => {
  it("shows recording state and live label from events", async () => {
    render(<HudApp />);
    // Wait for listeners to attach.
    await act(async () => {});
    expect(screen.getByText("Done")).toBeInTheDocument();

    act(() => {
      handlers.get("dictation:state")?.({
        payload: { state: "recording", message: null, text: null },
      });
    });
    expect(screen.getByText("Listening")).toBeInTheDocument();

    act(() => {
      handlers.get("dictation:state")?.({
        payload: { state: "error", message: "no microphone", text: null },
      });
    });
    expect(screen.getByText("no microphone")).toBeInTheDocument();
  });
});
