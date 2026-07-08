import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import { createLink, deleteLink, listLinksFrom, listLinksTo } from "../lib/links";

describe("links lib bindings", () => {
  beforeEach(() => invoke.mockReset());

  it("createLink sends camelCase args and null-fills an omitted relation", async () => {
    invoke.mockResolvedValue({ id: "l1" });
    await createLink({
      sourceKind: "note",
      sourceId: "a",
      targetKind: "dictation",
      targetId: "d",
    });
    expect(invoke).toHaveBeenCalledWith("create_link", {
      sourceKind: "note",
      sourceId: "a",
      targetKind: "dictation",
      targetId: "d",
      relation: null,
    });
  });

  it("createLink forwards an explicit relation", async () => {
    invoke.mockResolvedValue({ id: "l2" });
    await createLink({
      sourceKind: "note",
      sourceId: "a",
      targetKind: "meeting",
      targetId: "m",
      relation: "semantic",
    });
    expect(invoke).toHaveBeenCalledWith(
      "create_link",
      expect.objectContaining({ relation: "semantic" }),
    );
  });

  it("reads outbound and inbound edges by node", async () => {
    invoke.mockResolvedValue([]);
    await listLinksFrom("note", "a");
    expect(invoke).toHaveBeenCalledWith("list_links_from", { kind: "note", id: "a" });
    await listLinksTo("mindmap", "mm");
    expect(invoke).toHaveBeenCalledWith("list_links_to", { kind: "mindmap", id: "mm" });
  });

  it("deletes an edge by id", async () => {
    invoke.mockResolvedValue(undefined);
    await deleteLink("l1");
    expect(invoke).toHaveBeenCalledWith("delete_link", { id: "l1" });
  });
});
