import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { jsonSchema, type Tool, tool } from "ai";
import type { ApprovalBroker } from "./approvals.js";
import type { AgentEvent } from "./protocol.js";

export interface McpServerSpec {
  name: string;
  command: string;
  args?: string[];
  env?: Record<string, string>;
}

interface ConnectedServer {
  name: string;
  client: Client;
  transport: StdioClientTransport;
  toolNames: string[];
}

/**
 * Manages external MCP servers and exposes their tools to the agent.
 * MCP tool calls are gated through the same approval broker as built-ins:
 * an external process acting on the user's behalf always asks first (unless
 * pre-approved for the session).
 */
export class McpManager {
  private servers = new Map<string, ConnectedServer>();

  async connect(spec: McpServerSpec): Promise<string[]> {
    await this.disconnect(spec.name);
    const transport = new StdioClientTransport({
      command: spec.command,
      args: spec.args ?? [],
      env: { ...(process.env as Record<string, string>), ...(spec.env ?? {}) },
    });
    const client = new Client({ name: "arya", version: "0.1.0" });
    await client.connect(transport);
    const { tools } = await client.listTools();
    const toolNames = tools.map((t) => t.name);
    this.servers.set(spec.name, { name: spec.name, client, transport, toolNames });
    return toolNames;
  }

  async disconnect(name: string): Promise<void> {
    const server = this.servers.get(name);
    if (!server) return;
    this.servers.delete(name);
    try {
      await server.client.close();
    } catch {
      // best effort
    }
  }

  async closeAll(): Promise<void> {
    for (const name of [...this.servers.keys()]) {
      await this.disconnect(name);
    }
  }

  /**
   * AI-SDK tools for every connected server's tools, namespaced as
   * `mcp__<server>__<tool>`, each gated for approval.
   */
  async buildTools(
    broker: ApprovalBroker,
    emit: (event: AgentEvent) => void,
    nextCallId: () => string,
  ): Promise<Record<string, Tool>> {
    const result: Record<string, Tool> = {};
    for (const server of this.servers.values()) {
      const { tools } = await server.client.listTools();
      for (const definition of tools) {
        const toolName = `mcp__${server.name}__${definition.name}`;
        result[toolName] = tool({
          description: definition.description ?? `MCP tool ${definition.name}`,
          inputSchema: jsonSchema(definition.inputSchema as Record<string, unknown>),
          execute: async (args: unknown) => {
            if (!broker.isPreApproved(toolName)) {
              const callId = nextCallId();
              emit({
                kind: "tool-approval-required",
                callId,
                name: toolName,
                args,
                description: `MCP ${server.name}: ${definition.name}(${JSON.stringify(args)})`,
              });
              const approved = await broker.wait(callId, toolName);
              if (!approved) return "denied by user";
            }
            const response = await server.client.callTool({
              name: definition.name,
              arguments: args as Record<string, unknown>,
            });
            return summarizeMcpResult(response);
          },
        });
      }
    }
    return result;
  }
}

function summarizeMcpResult(response: unknown): string {
  const content = (response as { content?: Array<{ type: string; text?: string }> }).content;
  if (!Array.isArray(content)) return JSON.stringify(response).slice(0, 8000);
  return content
    .map((part) => (part.type === "text" ? (part.text ?? "") : `[${part.type}]`))
    .join("\n")
    .slice(0, 8000);
}
