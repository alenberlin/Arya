import { type ModelMessage, stepCountIs, streamText, type Tool } from "ai";
import { ApprovalBroker } from "./approvals.js";
import type { McpManager } from "./mcp.js";
import type { AgentEvent, SessionConfig } from "./protocol.js";
import { resolveModel } from "./providers.js";
import { buildTools } from "./tools.js";

const DEFAULT_SYSTEM = `You are Arya, a careful personal AI agent running on the user's Mac.
You work inside a workspace directory; file tools operate there.
Be concise. Use tools when they genuinely help. Never fabricate file contents
or command output. If a tool is denied by the user, adapt and continue.`;

const MAX_STEPS = 24;

/** One agent conversation bound to a model, workspace, and approval broker. */
export class Session {
  readonly broker = new ApprovalBroker();
  private messages: ModelMessage[] = [];
  private abort: AbortController | null = null;
  private steerQueue: string[] = [];
  private callCounter = 0;

  constructor(
    private config: SessionConfig,
    private emit: (event: AgentEvent) => void,
    private searchWorkspace: (query: string, limit: number) => Promise<string>,
    private mcp?: McpManager,
  ) {
    for (const item of config.history ?? []) {
      this.messages.push({ role: item.role, content: item.text });
    }
  }

  get running(): boolean {
    return this.abort !== null;
  }

  /** Queues a mid-run instruction; delivered at the next step boundary. */
  steer(text: string): void {
    if (!this.running) return;
    this.steerQueue.push(text);
    this.emit({ kind: "steered", text });
  }

  cancel(): void {
    this.abort?.abort();
    this.broker.denyAll();
  }

  async run(userText: string): Promise<void> {
    if (this.running) {
      throw new Error("session is already running");
    }
    this.messages.push({ role: "user", content: userText });
    this.abort = new AbortController();
    this.emit({ kind: "turn-started" });

    const nextCallId = () => `approval-${++this.callCounter}`;
    const builtins = buildTools({
      workspace: this.config.workspace,
      mode: this.config.mode,
      broker: this.broker,
      emit: this.emit,
      nextCallId,
      searchWorkspace: this.searchWorkspace,
    });
    let tools: Record<string, Tool> = builtins;
    if (this.mcp) {
      const mcpTools = await this.mcp.buildTools(this.broker, this.emit, nextCallId);
      tools = { ...builtins, ...mcpTools };
    }

    try {
      const result = streamText({
        model: resolveModel(this.config.model),
        system: this.config.system ?? DEFAULT_SYSTEM,
        messages: this.messages,
        tools,
        stopWhen: stepCountIs(MAX_STEPS),
        abortSignal: this.abort.signal,
        prepareStep: () => {
          // Deliver queued steering as fresh user guidance mid-run.
          if (this.steerQueue.length > 0) {
            const steer = this.steerQueue.splice(0).join("\n");
            this.messages.push({
              role: "user",
              content: `[steering] ${steer}`,
            });
            return { messages: this.messages };
          }
          return undefined;
        },
      });

      for await (const part of result.fullStream) {
        switch (part.type) {
          case "text-delta":
            this.emit({ kind: "text-delta", delta: part.text });
            break;
          case "reasoning-delta":
            this.emit({ kind: "reasoning-delta", delta: part.text });
            break;
          case "tool-call":
            this.emit({
              kind: "tool-call",
              callId: part.toolCallId,
              name: part.toolName,
              args: part.input,
            });
            break;
          case "tool-result":
            this.emit({
              kind: "tool-result",
              callId: part.toolCallId,
              name: part.toolName,
              result: typeof part.output === "string" ? part.output : JSON.stringify(part.output),
            });
            break;
          case "error":
            this.emit({ kind: "error", message: String(part.error) });
            break;
          default:
            break;
        }
      }

      const [finishReason, usage, responseMessages] = await Promise.all([
        result.finishReason,
        result.totalUsage,
        result.response.then((r) => r.messages),
      ]);
      this.messages.push(...responseMessages);
      this.emit({
        kind: "turn-finished",
        inputTokens: usage.inputTokens ?? 0,
        outputTokens: usage.outputTokens ?? 0,
        finishReason,
      });
    } catch (error) {
      if (this.abort?.signal.aborted) {
        this.emit({
          kind: "turn-finished",
          inputTokens: 0,
          outputTokens: 0,
          finishReason: "aborted",
        });
      } else {
        this.emit({ kind: "error", message: String(error) });
        this.emit({
          kind: "turn-finished",
          inputTokens: 0,
          outputTokens: 0,
          finishReason: "error",
        });
      }
    } finally {
      this.abort = null;
    }
  }
}
