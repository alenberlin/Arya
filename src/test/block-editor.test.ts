import { describe, expect, it } from "vitest";
import { parseInitialContent } from "../notes/blockDocument";

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
