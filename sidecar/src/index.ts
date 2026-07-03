/**
 * Arya agent sidecar: reads JSON-RPC requests from stdin (one per line),
 * writes responses and event notifications to stdout. Spawned and sandboxed
 * by the Rust shell; one process per write-mode.
 */
import { createInterface } from "node:readline";
import type { AgentEvent, ApprovalDecision, RpcRequest, SessionConfig } from "./protocol.js";
import { McpManager, type McpServerSpec } from "./mcp.js";
import { listOllamaModels } from "./providers.js";
import { Session } from "./session.js";

const sessions = new Map<string, Session>();
const mcp = new McpManager();

function send(payload: unknown): void {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

function notifyEvent(sessionId: string, event: AgentEvent): void {
  send({ jsonrpc: "2.0", method: "event", params: { sessionId, event } });
}

function ok(id: number | string, result: unknown): void {
  send({ jsonrpc: "2.0", id, result });
}

function fail(id: number | string, message: string): void {
  send({ jsonrpc: "2.0", id, error: { code: -32000, message } });
}

async function dispatch(request: RpcRequest): Promise<void> {
  const id = request.id ?? null;
  const params = (request.params ?? {}) as Record<string, unknown>;
  try {
    switch (request.method) {
      case "runtime.ping": {
        if (id !== null) ok(id, { pid: process.pid, version: "0.1.0" });
        return;
      }
      case "models.list": {
        const local = await listOllamaModels();
        const cloud: string[] = [];
        if (process.env.ANTHROPIC_API_KEY) {
          cloud.push("anthropic:claude-sonnet-5", "anthropic:claude-opus-4-8");
        }
        if (process.env.OPENAI_API_KEY) {
          cloud.push("openai:gpt-5.2", "openai:gpt-5-mini");
        }
        if (id !== null) ok(id, { models: [...local, ...cloud] });
        return;
      }
      case "session.start": {
        const config = params as unknown as SessionConfig;
        if (!config.sessionId || !config.model || !config.workspace) {
          throw new Error("sessionId, model, workspace are required");
        }
        sessions.set(
          config.sessionId,
          new Session(config, (event) => notifyEvent(config.sessionId, event), mcp),
        );
        if (id !== null) ok(id, { started: true });
        return;
      }
      case "session.message": {
        const session = sessions.get(String(params.sessionId));
        if (!session) throw new Error("unknown session");
        if (id !== null) ok(id, { accepted: true });
        // Fire and stream; completion arrives as turn-finished.
        void session.run(String(params.text));
        return;
      }
      case "session.steer": {
        const session = sessions.get(String(params.sessionId));
        if (!session) throw new Error("unknown session");
        session.steer(String(params.text));
        if (id !== null) ok(id, { steered: session.running });
        return;
      }
      case "session.cancel": {
        const session = sessions.get(String(params.sessionId));
        session?.cancel();
        if (id !== null) ok(id, { cancelled: true });
        return;
      }
      case "approval.resolve": {
        const session = sessions.get(String(params.sessionId));
        if (!session) throw new Error("unknown session");
        const resolved = session.broker.resolve(
          String(params.callId),
          String(params.decision) as ApprovalDecision,
        );
        if (id !== null) ok(id, { resolved });
        return;
      }
      case "mcp.connect": {
        const spec = params as unknown as McpServerSpec;
        const toolNames = await mcp.connect(spec);
        if (id !== null) ok(id, { tools: toolNames });
        return;
      }
      case "mcp.disconnect": {
        await mcp.disconnect(String(params.name));
        if (id !== null) ok(id, { disconnected: true });
        return;
      }
      case "runtime.shutdown": {
        await mcp.closeAll();
        for (const session of sessions.values()) session.cancel();
        if (id !== null) ok(id, { bye: true });
        process.exit(0);
        return;
      }
      default:
        if (id !== null) fail(id, `unknown method ${request.method}`);
    }
  } catch (error) {
    if (id !== null) fail(id, String(error));
  }
}

const reader = createInterface({ input: process.stdin });
reader.on("line", (line) => {
  const trimmed = line.trim();
  if (!trimmed) return;
  let request: RpcRequest;
  try {
    request = JSON.parse(trimmed) as RpcRequest;
  } catch {
    return;
  }
  void dispatch(request);
});
reader.on("close", () => process.exit(0));

send({ jsonrpc: "2.0", method: "ready", params: { pid: process.pid } });
