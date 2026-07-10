import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPanel } from "../agent/AgentPanel";
import type { AgentSession } from "../lib/agent";

const backend: {
  sessions: AgentSession[];
  sent: string[];
} = {
  sessions: [],
  sent: [],
};

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case "agent_list_models":
        return ["ollama:llama3.2", "openai:gpt-5-mini"];
      case "agent_list_sessions":
        return backend.sessions;
      case "agent_create_session": {
        const session: AgentSession = {
          id: `session-${backend.sessions.length + 1}`,
          title: "New chat",
          model: String(args?.model),
          mode: "default",
          createdAt: "2026-07-06T00:00:00Z",
          updatedAt: "2026-07-06T00:00:00Z",
        };
        backend.sessions = [session, ...backend.sessions];
        return session;
      }
      case "agent_get_messages":
        return [];
      case "agent_send":
        backend.sent.push(String(args?.text));
        return null;
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  }),
}));

describe("AgentPanel", () => {
  beforeEach(() => {
    backend.sessions = [];
    backend.sent = [];
  });

  it("shows local privacy copy only for local models", async () => {
    const user = userEvent.setup();
    render(<AgentPanel />);

    await screen.findByLabelText("agent model");
    await user.click(screen.getByRole("button", { name: "New chat" }));

    expect(await screen.findByText(/Running locally/)).toBeInTheDocument();
    expect(screen.queryByText(/Cloud model/)).not.toBeInTheDocument();
    expect(document.body.textContent?.match(/On-device · free · private/g)).toHaveLength(1);
  });

  it("switches the footer to cloud privacy copy for cloud models", async () => {
    const user = userEvent.setup();
    render(<AgentPanel />);

    await screen.findByLabelText("agent model");
    await user.click(screen.getByRole("button", { name: "New chat" }));
    await user.selectOptions(screen.getByLabelText("agent model"), "openai:gpt-5-mini");

    await waitFor(() => {
      expect(screen.getByText(/Cloud model/)).toBeInTheDocument();
    });
    expect(screen.queryByText(/Running locally/)).not.toBeInTheDocument();
  });

  it("has a multi-line composer: Shift+Enter adds a newline, Enter sends", async () => {
    const user = userEvent.setup();
    render(<AgentPanel />);

    await screen.findByLabelText("agent model");
    await user.click(screen.getByRole("button", { name: "New chat" }));

    const composer = await screen.findByLabelText<HTMLTextAreaElement>("agent composer");
    // The field shows at least five lines so longer drafts are readable.
    expect(composer.rows).toBeGreaterThanOrEqual(5);

    // Shift+Enter inserts a newline without sending.
    await user.type(composer, "line one");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await user.type(composer, "line two");
    expect(composer.value).toBe("line one\nline two");
    expect(backend.sent).toEqual([]);

    // A bare Enter submits the whole multi-line draft and clears the field.
    await user.keyboard("{Enter}");
    await waitFor(() => expect(backend.sent).toEqual(["line one\nline two"]));
    expect(composer.value).toBe("");
  });

  it("sends the draft on Cmd/Ctrl+Enter as well", async () => {
    const user = userEvent.setup();
    render(<AgentPanel />);

    await screen.findByLabelText("agent model");
    await user.click(screen.getByRole("button", { name: "New chat" }));

    const composer = await screen.findByLabelText<HTMLTextAreaElement>("agent composer");
    await user.type(composer, "ship it");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(backend.sent).toEqual(["ship it"]));
  });
});
