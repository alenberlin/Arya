import { exec } from "node:child_process";
import { readdir, readFile, stat, writeFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { promisify } from "node:util";
import { tool } from "ai";
import { z } from "zod";
import type { ApprovalBroker } from "./approvals.js";
import type { AgentEvent } from "./protocol.js";
import { resolveInWorkspace, resolveReadable } from "./paths.js";

const execAsync = promisify(exec);

export interface ToolContext {
  workspace: string;
  mode: "sandboxed" | "unrestricted";
  broker: ApprovalBroker;
  emit: (event: AgentEvent) => void;
  nextCallId: () => string;
}

/**
 * Gate helper: risky tools pause for user approval unless pre-approved.
 * A denial returns a normal tool result so the model can adjust course.
 */
async function gate(
  ctx: ToolContext,
  toolName: string,
  description: string,
  args: unknown,
): Promise<boolean> {
  if (ctx.broker.isPreApproved(toolName)) return true;
  const callId = ctx.nextCallId();
  ctx.emit({
    kind: "tool-approval-required",
    callId,
    name: toolName,
    args,
    description,
  });
  return ctx.broker.wait(callId, toolName);
}

const MAX_READ_BYTES = 512 * 1024;
const MAX_RESULT_CHARS = 32_000;

function clip(text: string): string {
  return text.length > MAX_RESULT_CHARS
    ? `${text.slice(0, MAX_RESULT_CHARS)}\n…[truncated]`
    : text;
}

export function buildTools(ctx: ToolContext) {
  return {
    read_file: tool({
      description: "Read a text file. Paths are relative to the workspace.",
      inputSchema: z.object({ path: z.string() }),
      execute: async ({ path }) => {
        const target = resolveReadable(ctx.workspace, path);
        const info = await stat(target);
        if (info.size > MAX_READ_BYTES) {
          return `file is ${info.size} bytes; too large to read whole`;
        }
        return clip(await readFile(target, "utf8"));
      },
    }),

    write_file: tool({
      description:
        "Write a text file inside the workspace, creating parent directories.",
      inputSchema: z.object({ path: z.string(), content: z.string() }),
      execute: async ({ path, content }) => {
        // In unrestricted mode writes anywhere require approval; in
        // sandboxed mode the jail confines writes, and workspace writes
        // are the agent's normal work - no prompt.
        if (ctx.mode === "unrestricted") {
          const approved = await gate(
            ctx,
            "write_file",
            `Write ${content.length} chars to ${path}`,
            { path },
          );
          if (!approved) return "denied by user";
          const target = resolveReadable(ctx.workspace, path);
          await mkdir(dirname(target), { recursive: true });
          await writeFile(target, content, "utf8");
          return `wrote ${content.length} chars to ${path}`;
        }
        const target = resolveInWorkspace(ctx.workspace, path);
        await mkdir(dirname(target), { recursive: true });
        await writeFile(target, content, "utf8");
        return `wrote ${content.length} chars to ${path}`;
      },
    }),

    list_dir: tool({
      description: "List directory entries (name + kind).",
      inputSchema: z.object({ path: z.string().default(".") }),
      execute: async ({ path }) => {
        const target = resolveReadable(ctx.workspace, path);
        const entries = await readdir(target, { withFileTypes: true });
        return clip(
          entries
            .slice(0, 500)
            .map((e) => `${e.isDirectory() ? "dir " : "file"} ${e.name}`)
            .join("\n") || "(empty)",
        );
      },
    }),

    run_command: tool({
      description:
        "Run a shell command in the workspace. Requires user approval.",
      inputSchema: z.object({ command: z.string() }),
      execute: async ({ command }) => {
        const approved = await gate(
          ctx,
          "run_command",
          `Run: ${command}`,
          { command },
        );
        if (!approved) return "denied by user";
        try {
          const { stdout, stderr } = await execAsync(command, {
            cwd: ctx.workspace,
            timeout: 120_000,
            maxBuffer: 4 * 1024 * 1024,
          });
          return clip(`${stdout}${stderr ? `\nstderr:\n${stderr}` : ""}` || "(no output)");
        } catch (error) {
          return clip(`command failed: ${String(error)}`);
        }
      },
    }),
  };
}
