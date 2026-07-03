/**
 * Shell <-> sidecar protocol: JSON-RPC 2.0, newline-delimited over stdio.
 *
 * Shell -> sidecar requests:
 *   session.start   { sessionId, model, mode, workspace, system? }
 *   session.message { sessionId, text }
 *   session.steer   { sessionId, text }
 *   session.cancel  { sessionId }
 *   approval.resolve{ sessionId, callId, decision: "once"|"session"|"always"|"deny" }
 *   models.list     {}
 *   runtime.ping    {}
 *
 * Sidecar -> shell notifications (method "event"):
 *   { sessionId, event: AgentEvent }
 */

export interface RpcRequest {
  jsonrpc: "2.0";
  id?: number | string;
  method: string;
  params?: Record<string, unknown>;
}

/** Reverse call: sidecar -> shell request for workspace context. */
export interface ContextSearchRequest {
  jsonrpc: "2.0";
  id: number;
  method: "context.search";
  params: { query: string; limit: number };
}

export interface RpcResponse {
  jsonrpc: "2.0";
  id: number | string;
  result?: unknown;
  error?: { code: number; message: string };
}

export type ApprovalDecision = "once" | "session" | "always" | "deny";

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
  | {
      kind: "turn-finished";
      inputTokens: number;
      outputTokens: number;
      finishReason: string;
    }
  | { kind: "steered"; text: string }
  | { kind: "error"; message: string };

export interface SessionConfig {
  sessionId: string;
  /** Provider-qualified model id, e.g. "anthropic:claude-sonnet-5". */
  model: string;
  mode: "sandboxed" | "unrestricted";
  workspace: string;
  system?: string;
  /** Restore point: prior conversation as (role, text) pairs. */
  history?: Array<{ role: "user" | "assistant"; text: string }>;
}
