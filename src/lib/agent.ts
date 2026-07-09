import { invoke } from "@tauri-apps/api/core";

export interface AgentSession {
  id: string;
  title: string;
  model: string;
  mode: string;
  createdAt: string;
  updatedAt: string;
}

export interface AgentMessage {
  id: string;
  role: "user" | "assistant";
  contentJson: string;
  createdAt: string;
}

export interface ToolInfo {
  callId: string;
  name: string;
  args: unknown;
  result?: string;
}

export type AgentEvent =
  | { kind: "turn-started" }
  | { kind: "text-delta"; delta: string }
  | { kind: "reasoning-delta"; delta: string }
  | { kind: "tool-call"; callId: string; name: string; args: unknown }
  | {
      kind: "tool-approval-required";
      callId: string;
      name: string;
      args: unknown;
      description: string;
    }
  | { kind: "tool-result"; callId: string; name: string; result: string }
  | { kind: "turn-finished"; inputTokens: number; outputTokens: number; finishReason: string }
  | { kind: "steered"; text: string }
  | { kind: "error"; message: string };

export const agentListModels = () => invoke<string[]>("agent_list_models");
export const agentCreateSession = (model: string, mode?: string) =>
  invoke<AgentSession>("agent_create_session", { model, mode: mode ?? null });
export const agentListSessions = () => invoke<AgentSession[]>("agent_list_sessions");
export const agentGetMessages = (sessionId: string) =>
  invoke<AgentMessage[]>("agent_get_messages", { sessionId });
export const agentSend = (sessionId: string, text: string) =>
  invoke<void>("agent_send", { sessionId, text });
export const agentSteer = (sessionId: string, text: string) =>
  invoke<void>("agent_steer", { sessionId, text });
export const agentCancel = (sessionId: string) => invoke<void>("agent_cancel", { sessionId });
export const agentResolveApproval = (sessionId: string, callId: string, decision: string) =>
  invoke<void>("agent_resolve_approval", { sessionId, callId, decision });
export const agentDeleteSession = (sessionId: string) =>
  invoke<void>("agent_delete_session", { sessionId });
/** Converts an agent chat into a note (markdown transcript); returns the note id. */
export const convertSessionToNote = (sessionId: string) =>
  invoke<string>("convert_session_to_note", { sessionId });
/** Generates an image into the agent workspace; returns its saved path. */
export const agentGenerateImage = (prompt: string, size?: string | null) =>
  invoke<{ path: string }>("agent_generate_image", { prompt, size: size ?? null });
/** Reads a workspace file as base64 (used to render generated images). */
export const agentWorkspaceReadB64 = (path: string) =>
  invoke<string>("agent_workspace_read_b64", { path });

/** Privacy tier for the model picker: local models never leave the Mac. */
export function modelPrivacy(model: string): "local" | "cloud" {
  return model.startsWith("ollama:") ? "local" : "cloud";
}
