import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AgentSection } from "../agent/AgentSection";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

// The sub-panels each fetch on mount; return empty/benign data so the hub can
// render any view without a live shell.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    switch (cmd) {
      case "agent_list_models":
        return ["ollama:llama3.2"];
      case "agent_list_sessions":
        return [];
      case "routine_list":
        return [];
      case "mcp_list_servers":
        return [];
      default:
        return null;
    }
  }),
}));

describe("AgentSection", () => {
  it("folds chat, routines, MCP, and security under one Agent hub", async () => {
    const user = userEvent.setup();
    render(<AgentSection />);

    // Chat is the default sub-view: the composer's New chat control is present.
    expect(await screen.findByRole("button", { name: "New chat" })).toBeInTheDocument();

    // Security surfaces the posture in plain language, without reading code.
    await user.click(screen.getByRole("tab", { name: /Security/ }));
    expect(await screen.findByText(/Sandboxed by default/)).toBeInTheDocument();
    expect(screen.getByText(/asks first/i)).toBeInTheDocument();
    expect(screen.getByText(/On-device by default/)).toBeInTheDocument();
    expect(screen.getByText(/MCP servers are opt-in/)).toBeInTheDocument();

    // Routines and MCP are reachable as sub-views, not top-level pillars.
    await user.click(screen.getByRole("tab", { name: /Routines/ }));
    expect(await screen.findByRole("button", { name: /Add routine/ })).toBeInTheDocument();

    await user.click(screen.getByRole("tab", { name: /MCP servers/ }));
    expect(await screen.findByText(/Connect external tools/)).toBeInTheDocument();

    // Back to chat.
    await user.click(screen.getByRole("tab", { name: /Chat/ }));
    expect(await screen.findByRole("button", { name: "New chat" })).toBeInTheDocument();
  });
});
