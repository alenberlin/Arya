import { describe, expect, it } from "vitest";
import { ApprovalBroker } from "../src/approvals.js";
import { resolveInWorkspace } from "../src/paths.js";
import { resolveModel } from "../src/providers.js";

describe("workspace path jail", () => {
  const ws = "/tmp/arya-ws";

  it("resolves relative paths inside the workspace", () => {
    expect(resolveInWorkspace(ws, "notes/a.md")).toBe("/tmp/arya-ws/notes/a.md");
    expect(resolveInWorkspace(ws, ".")).toBe("/tmp/arya-ws");
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
    expect(resolveInWorkspace(ws, "/tmp/arya-ws/sub/file")).toBe("/tmp/arya-ws/sub/file");
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
