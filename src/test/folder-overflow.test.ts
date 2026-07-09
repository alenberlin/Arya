import { describe, expect, it } from "vitest";
import type { Folder } from "../lib/notes";
import { splitFolders } from "../notes/NotesWorkspace";

const folder = (id: string): Folder => ({ id, name: id, createdAt: "2026-07-09T00:00:00Z" });
const ids = (fs: Folder[]) => fs.map((f) => f.id);

describe("splitFolders", () => {
  it("shows every folder inline when there are few", () => {
    const all = [folder("a"), folder("b"), folder("c")];
    const { visible, overflow } = splitFolders(all, null);
    expect(ids(visible)).toEqual(["a", "b", "c"]);
    expect(overflow).toEqual([]);
  });

  it("folds the tail into overflow past the inline limit", () => {
    const all = ["a", "b", "c", "d", "e"].map(folder);
    const { visible, overflow } = splitFolders(all, null);
    expect(ids(visible)).toEqual(["a", "b", "c"]);
    expect(ids(overflow)).toEqual(["d", "e"]);
  });

  it("keeps an already-inline active folder in place", () => {
    const all = ["a", "b", "c", "d", "e"].map(folder);
    const { visible, overflow } = splitFolders(all, "b");
    expect(ids(visible)).toEqual(["a", "b", "c"]);
    expect(ids(overflow)).toEqual(["d", "e"]);
  });

  it("surfaces an overflowed active folder into the last inline slot", () => {
    const all = ["a", "b", "c", "d", "e"].map(folder);
    const { visible, overflow } = splitFolders(all, "e");
    // "e" would have been hidden; it takes the last visible slot instead, and
    // never appears twice.
    expect(ids(visible)).toEqual(["a", "b", "e"]);
    expect(ids(overflow)).toEqual(["c", "d"]);
    expect(ids(visible)).not.toContain("d");
  });
});
