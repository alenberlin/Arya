import { mkdirSync, mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ApprovalBroker } from "../src/approvals.js";
import { safeMcpEnv } from "../src/mcp.js";
import { classifyReadable, resolveInWorkspace } from "../src/paths.js";
import { resolveModel } from "../src/providers.js";

describe("workspace path jail", () => {
  const ws = "/tmp/arya-ws";

  it("resolves relative paths inside the workspace", () => {
    expect(resolveInWorkspace(ws, "notes/a.md").endsWith("/arya-ws/notes/a.md")).toBe(true);
    expect(resolveInWorkspace(ws, ".").endsWith("/arya-ws")).toBe(true);
  });

  it("rejects traversal escapes", () => {
    expect(() => resolveInWorkspace(ws, "../outside.txt")).toThrow(/escapes/);
    expect(() => resolveInWorkspace(ws, "a/../../etc/passwd")).toThrow(/escapes/);
  });

  it("rejects absolute paths outside the workspace", () => {
    expect(() => resolveInWorkspace(ws, "/etc/passwd")).toThrow(/escapes/);
    // Sibling directory sharing the prefix must not pass.
    expect(() => resolveInWorkspace(ws, "/tmp/arya-ws-evil/x")).toThrow(/escapes/);
  });

  it("accepts absolute paths inside the workspace", () => {
    expect(resolveInWorkspace(ws, "/tmp/arya-ws/sub/file").endsWith("/arya-ws/sub/file")).toBe(
      true,
    );
  });

  it("rejects writes through an in-workspace symlink that points outside", () => {
    const realWs = mkdtempSync(join(tmpdir(), "arya-ws-"));
    const outside = mkdtempSync(join(tmpdir(), "arya-out-"));
    // A symlink inside the workspace pointing at an external dir.
    symlinkSync(outside, join(realWs, "link"));
    try {
      // Lexically "inside", but resolves out via the link — must be rejected.
      expect(() => resolveInWorkspace(realWs, "link/evil.txt")).toThrow(/link|escapes/);
    } finally {
      rmSync(realWs, { recursive: true, force: true });
      rmSync(outside, { recursive: true, force: true });
    }
  });

  it("allows a new file whose parent exists inside the workspace", () => {
    const realWs = mkdtempSync(join(tmpdir(), "arya-ws-"));
    mkdirSync(join(realWs, "notes"));
    try {
      const resolved = resolveInWorkspace(realWs, "notes/new.md");
      expect(resolved.endsWith("/notes/new.md")).toBe(true);
    } finally {
      rmSync(realWs, { recursive: true, force: true });
    }
  });
});

describe("readable path classification (read confinement)", () => {
  it("marks in-workspace paths as inside", () => {
    const realWs = mkdtempSync(join(tmpdir(), "arya-ws-"));
    try {
      expect(classifyReadable(realWs, "notes/a.md").inside).toBe(true);
      expect(classifyReadable(realWs, ".").inside).toBe(true);
    } finally {
      rmSync(realWs, { recursive: true, force: true });
    }
  });

  it("marks out-of-workspace paths as outside so reads get gated", () => {
    const realWs = mkdtempSync(join(tmpdir(), "arya-ws-"));
    try {
      expect(classifyReadable(realWs, "/etc/passwd").inside).toBe(false);
      expect(classifyReadable(realWs, "../../etc/passwd").inside).toBe(false);
    } finally {
      rmSync(realWs, { recursive: true, force: true });
    }
  });

  it("treats an in-workspace symlink pointing outside as outside", () => {
    const realWs = mkdtempSync(join(tmpdir(), "arya-ws-"));
    const outside = mkdtempSync(join(tmpdir(), "arya-out-"));
    symlinkSync(outside, join(realWs, "link"));
    try {
      expect(classifyReadable(realWs, "link/secret").inside).toBe(false);
    } finally {
      rmSync(realWs, { recursive: true, force: true });
      rmSync(outside, { recursive: true, force: true });
    }
  });
});

describe("MCP env scrubbing", () => {
  it("strips ambient secrets but keeps essentials and declared env", () => {
    const saved = {
      ARYA_API_TOKEN: process.env.ARYA_API_TOKEN,
      ANTHROPIC_API_KEY: process.env.ANTHROPIC_API_KEY,
      PATH: process.env.PATH,
    };
    process.env.ARYA_API_TOKEN = "super-secret";
    process.env.ANTHROPIC_API_KEY = "sk-ant-secret-value";
    process.env.PATH = "/usr/bin";
    try {
      const env = safeMcpEnv({ MY_SERVER_TOKEN: "declared" });
      expect(env.ARYA_API_TOKEN).toBeUndefined();
      expect(env.ANTHROPIC_API_KEY).toBeUndefined();
      expect(env.PATH).toBe("/usr/bin");
      expect(env.MY_SERVER_TOKEN).toBe("declared");
    } finally {
      for (const [key, value] of Object.entries(saved)) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
    }
  });
});

describe("approval broker", () => {
  it("once approves a single call only", async () => {
    const broker = new ApprovalBroker();
    const decision = broker.wait("c1", "run_command");
    expect(broker.resolve("c1", "once")).toBe(true);
    await expect(decision).resolves.toBe(true);
    expect(broker.isPreApproved("run_command")).toBe(false);
  });

  it("session approval pre-approves subsequent calls", async () => {
    const broker = new ApprovalBroker();
    const decision = broker.wait("c1", "run_command");
    broker.resolve("c1", "session");
    await expect(decision).resolves.toBe(true);
    expect(broker.isPreApproved("run_command")).toBe(true);
  });

  it("deny resolves false and unknown ids are reported", async () => {
    const broker = new ApprovalBroker();
    const decision = broker.wait("c1", "write_file");
    expect(broker.resolve("nope", "once")).toBe(false);
    broker.resolve("c1", "deny");
    await expect(decision).resolves.toBe(false);
  });

  it("denyAll flushes pending approvals", async () => {
    const broker = new ApprovalBroker();
    const a = broker.wait("a", "run_command");
    const b = broker.wait("b", "write_file");
    broker.denyAll();
    await expect(a).resolves.toBe(false);
    await expect(b).resolves.toBe(false);
  });
});

describe("model resolution", () => {
  it("requires provider-qualified ids", () => {
    expect(() => resolveModel("claude-sonnet-5")).toThrow(/provider-qualified/);
    expect(() => resolveModel("nope:model")).toThrow(/unknown provider/);
  });

  it("builds models for each provider", () => {
    expect(resolveModel("ollama:llama3.2")).toBeTruthy();
    expect(resolveModel("anthropic:claude-sonnet-5")).toBeTruthy();
    expect(resolveModel("openai:gpt-5-mini")).toBeTruthy();
  });
});
