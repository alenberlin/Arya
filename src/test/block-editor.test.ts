import { describe, expect, it } from "vitest";
import { extractMentionTargets, parseInitialContent } from "../notes/blockDocument";

describe("parseInitialContent", () => {
  it("returns undefined for empty or blank input", () => {
    expect(parseInitialContent("")).toBeUndefined();
    expect(parseInitialContent("   ")).toBeUndefined();
  });

  it("returns undefined defensively for non-string input", () => {
    expect(parseInitialContent(undefined as unknown as string)).toBeUndefined();
    expect(parseInitialContent(null as unknown as string)).toBeUndefined();
  });

  it("returns undefined for invalid JSON rather than throwing", () => {
    expect(parseInitialContent("{not valid json")).toBeUndefined();
  });

  it("returns undefined for a non-array or empty array", () => {
    expect(parseInitialContent('{"type":"heading"}')).toBeUndefined();
    expect(parseInitialContent("[]")).toBeUndefined();
  });

  it("parses a valid block-JSON array", () => {
    const blocks = parseInitialContent('[{"type":"paragraph","content":"hi"}]');
    expect(blocks).toEqual([{ type: "paragraph", content: "hi" }]);
  });
});

describe("extractMentionTargets", () => {
  it("returns [] for non-array or empty input", () => {
    expect(extractMentionTargets(null)).toEqual([]);
    expect(extractMentionTargets([])).toEqual([]);
  });

  it("collects mentions from inline content and nested children, de-duplicated", () => {
    const doc = [
      {
        type: "paragraph",
        content: [
          { type: "text", text: "see " },
          { type: "mention", props: { kind: "note", id: "n1", label: "A" } },
          { type: "mention", props: { kind: "note", id: "n1", label: "A" } },
        ],
        children: [
          {
            type: "paragraph",
            content: [{ type: "mention", props: { kind: "dictation", id: "d9", label: "D" } }],
          },
        ],
      },
    ];
    expect(extractMentionTargets(doc)).toEqual([
      { kind: "note", id: "n1" },
      { kind: "dictation", id: "d9" },
    ]);
  });

  it("defaults a missing kind to note and ignores mentions without an id", () => {
    const doc = [
      {
        type: "paragraph",
        content: [
          { type: "mention", props: { id: "x1", label: "X" } },
          { type: "mention", props: { kind: "note", label: "no id" } },
        ],
      },
    ];
    expect(extractMentionTargets(doc)).toEqual([{ kind: "note", id: "x1" }]);
  });
});
