import { execFile } from "node:child_process";
import { mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { promisify } from "node:util";
import { tool } from "ai";
import { z } from "zod";
import type { ApprovalBroker } from "./approvals.js";
import { generateImageToWorkspace, imageGenerationAvailable } from "./images.js";
import { safeMcpEnv } from "./mcp.js";
import { classifyReadable, resolveInWorkspace } from "./paths.js";
import type { AgentEvent } from "./protocol.js";

const execFileAsync = promisify(execFile);

export interface ToolContext {
  workspace: string;
  mode: "sandboxed";
  broker: ApprovalBroker;
  emit: (event: AgentEvent) => void;
  nextCallId: () => string;
  /** Reverse-RPC into the shell for workspace RAG. */
  searchWorkspace: (query: string, limit: number) => Promise<string>;
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
  return text.length > MAX_RESULT_CHARS ? `${text.slice(0, MAX_RESULT_CHARS)}\n…[truncated]` : text;
}

/**
 * Strips control and bidi/format codepoints so a tool argument cannot spoof the
 * approval prompt (Trojan Source), and caps length for display.
 */
export function sanitizeForDisplay(text: string): string {
  let out = "";
  for (const ch of text.slice(0, 2000)) {
    const c = ch.codePointAt(0) ?? 0;
    const control = c < 0x20 || (c >= 0x7f && c <= 0x9f);
    const bidiOrFormat =
      (c >= 0x200b && c <= 0x200f) ||
      (c >= 0x202a && c <= 0x202e) ||
      (c >= 0x2060 && c <= 0x206f) ||
      c === 0xfeff;
    if (!control && !bidiOrFormat) out += ch;
  }
  return out.slice(0, 500);
}

/** Keys whose values are secrets and must never reach the approval prompt. */
const SENSITIVE_KEY =
  /(api[_-]?key|secret|token|passw(or)?d|authorization|auth|private[_-]?key|access[_-]?token|refresh[_-]?token|cookie|credential)/i;

/**
 * Redacts secret-looking values and sanitizes the rest so a tool argument can't
 * leak credentials or spoof the approval prompt. Used for anything rendered to
 * the user (MCP tool args in particular carry arbitrary third-party JSON).
 */
export function sanitizeArgsForDisplay(args: unknown): unknown {
  if (typeof args === "string") return sanitizeForDisplay(args);
  if (Array.isArray(args)) return args.map(sanitizeArgsForDisplay);
  if (args && typeof args === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(args)) {
      out[key] = SENSITIVE_KEY.test(key) ? "[redacted]" : sanitizeArgsForDisplay(value);
    }
    return out;
  }
  return args;
}

export function buildTools(ctx: ToolContext) {
  return {
    read_file: tool({
      description:
        "Read a text file. Paths are workspace-relative; reading a path outside " +
        "the workspace requires explicit user approval.",
      inputSchema: z.object({ path: z.string() }),
      execute: async ({ path }) => {
        const { target, inside } = classifyReadable(ctx.workspace, path);
        if (!inside) {
          // Scope the session approval to this exact path, so "allow for
          // session" on one file doesn't grant whole-filesystem reads.
          const approved = await gate(
            ctx,
            `read_outside:${target}`,
            `Read file outside the workspace: ${sanitizeForDisplay(target)}`,
            { path: target },
          );
          if (!approved) return "denied by user";
        }
        const info = await stat(target);
        if (info.size > MAX_READ_BYTES) {
          return `file is ${info.size} bytes; too large to read whole`;
        }
        return clip(await readFile(target, "utf8"));
      },
    }),

    write_file: tool({
      description: "Write a text file inside the workspace, creating parent directories.",
      inputSchema: z.object({ path: z.string(), content: z.string() }),
      execute: async ({ path, content }) => {
        const target = resolveInWorkspace(ctx.workspace, path);
        await mkdir(dirname(target), { recursive: true });
        await writeFile(target, content, "utf8");
        return `wrote ${content.length} chars to ${path}`;
      },
    }),

    list_dir: tool({
      description:
        "List directory entries (name + kind). Listing a directory outside the " +
        "workspace requires explicit user approval.",
      inputSchema: z.object({ path: z.string().default(".") }),
      execute: async ({ path }) => {
        const { target, inside } = classifyReadable(ctx.workspace, path);
        if (!inside) {
          // Scope the session approval to this exact path, so "allow for
          // session" on one directory doesn't grant whole-filesystem reads.
          const approved = await gate(
            ctx,
            `read_outside:${target}`,
            `List directory outside the workspace: ${sanitizeForDisplay(target)}`,
            { path: target },
          );
          if (!approved) return "denied by user";
        }
        const entries = await readdir(target, { withFileTypes: true });
        return clip(
          entries
            .slice(0, 500)
            .map((e) => `${e.isDirectory() ? "dir " : "file"} ${e.name}`)
            .join("\n") || "(empty)",
        );
      },
    }),

    search_workspace: tool({
      description:
        "Semantic search over the user's own notes, meeting transcripts, " +
        "dictations, and past agent sessions. Use this to ground answers in " +
        "the user's workspace. Returns matching passages with their source.",
      inputSchema: z.object({ query: z.string(), limit: z.number().default(6) }),
      execute: async ({ query, limit }) => {
        return ctx.searchWorkspace(query, limit ?? 6);
      },
    }),

    generate_image: tool({
      description: imageGenerationAvailable()
        ? "Generate an image from a text prompt; saves a PNG into the workspace and returns its path."
        : "Generate an image from a text prompt (currently unavailable: no cloud image model configured).",
      inputSchema: z.object({
        prompt: z.string(),
        size: z.string().optional().describe("e.g. 1024x1024"),
      }),
      execute: async ({ prompt, size }) => {
        try {
          const result = await generateImageToWorkspace(ctx.workspace, prompt, size);
          ctx.emit({
            kind: "tool-result",
            callId: ctx.nextCallId(),
            name: "image-saved",
            result: result.path,
          });
          return `image saved to ${result.path} (${result.bytes} bytes)`;
        } catch (error) {
          return `image generation failed: ${String(error)}`;
        }
      },
    }),

    run_command: tool({
      description:
        "Run a program directly (no shell) in the workspace. Provide the program " +
        "and its arguments as separate values; shell features (pipes, redirects, " +
        "globbing, $(...), &&) are unavailable. Each distinct program is approved " +
        "separately.",
      inputSchema: z.object({
        program: z.string(),
        args: z.array(z.string()).default([]),
      }),
      execute: async ({ program, args }) => {
        const argv = args ?? [];
        // Per-program approval scope: approving `git` never blesses `curl`, and
        // there is no shell to chain a second program off one approval.
        const approved = await gate(
          ctx,
          `run_command:${program}`,
          `Run (no shell): ${sanitizeForDisplay([program, ...argv].join(" "))}`,
          { program, args: argv },
        );
        if (!approved) return "denied by user";
        try {
          const { stdout, stderr } = await execFileAsync(program, argv, {
            cwd: ctx.workspace,
            timeout: 120_000,
            maxBuffer: 4 * 1024 * 1024,
            // Scrub the environment: the sidecar carries the user's provider keys.
            // An approved program must never inherit those — same guard MCP
            // server spawns already use.
            env: safeMcpEnv(),
          });
          return clip(`${stdout}${stderr ? `\nstderr:\n${stderr}` : ""}` || "(no output)");
        } catch (error) {
          return clip(`command failed: ${String(error)}`);
        }
      },
    }),
  };
}
