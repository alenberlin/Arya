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

const invoked: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    invoked.push({ cmd, args });
    return undefined;
  }),
}));

const targetPayload = (over: Record<string, unknown> = {}) => ({
  name: "Mail",
  bundleId: "com.apple.mail",
  polish: "clean",
  style: "standard",
  pinned: false,
  ...over,
});

describe("dictation HUD", () => {
  it("shows recording state, destination, and error label from events", async () => {
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

    // The destination chip shows where the text will land.
    act(() => {
      handlers.get("dictation:target")?.({ payload: targetPayload() });
    });
    expect(screen.getByText("Mail")).toBeInTheDocument();

    act(() => {
      handlers.get("dictation:state")?.({
        payload: { state: "error", message: "no microphone", text: null },
      });
    });
    expect(screen.getByText("no microphone")).toBeInTheDocument();
  });

  it("cycles AI polish as a one-off session override and reveals the levels", async () => {
    invoked.length = 0;
    render(<HudApp />);
    await act(async () => {});

    act(() => {
      handlers.get("dictation:state")?.({
        payload: { state: "recording", message: null, text: null },
      });
    });
    const chip = screen.getByLabelText("AI polish: Clean");

    // One tap cycles to the next level and reveals the whole set.
    act(() => {
      chip.click();
    });
    expect(screen.getByRole("group", { name: "AI polish level" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Polished" })).toBeInTheDocument();
    expect(invoked.at(-1)).toMatchObject({
      cmd: "dictation_set_session_polish",
      args: { polish: "polished" },
    });

    // Picking a segment jumps straight to it and collapses the panel.
    act(() => {
      screen.getByRole("button", { name: "Raw" }).click();
    });
    expect(invoked.at(-1)).toMatchObject({
      cmd: "dictation_set_session_polish",
      args: { polish: "raw" },
    });
    expect(screen.queryByRole("group", { name: "AI polish level" })).not.toBeInTheDocument();
  });

  it("pins the current polish as the app default", async () => {
    invoked.length = 0;
    render(<HudApp />);
    await act(async () => {});

    act(() => {
      handlers.get("dictation:state")?.({
        payload: { state: "recording", message: null, text: null },
      });
      handlers.get("dictation:target")?.({
        payload: targetPayload({ name: "Slack", bundleId: "com.tinyspeck.slackmacgap" }),
      });
    });

    // Open the panel (cycles once), then pin.
    act(() => {
      screen.getByLabelText("AI polish: Clean").click();
    });
    act(() => {
      screen.getByRole("button", { name: /Pin for Slack/ }).click();
    });
    expect(invoked.some((i) => i.cmd === "dictation_pin_app")).toBe(true);
  });
});
