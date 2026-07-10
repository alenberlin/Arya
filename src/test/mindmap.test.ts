import { describe, expect, it } from "vitest";
import { facingSide, withDepths } from "../mindmap/MindMapPanel";

/** Narrows a `withDepths` result to its mind-node depth, for assertions. */
function depthOf(nodes: ReturnType<typeof withDepths>, id: string): number | undefined {
  const node = nodes.find((n) => n.id === id);
  return node && node.type !== "sticky" ? node.data.depth : undefined;
}

describe("facingSide", () => {
  it("picks the horizontal side when the horizontal delta dominates", () => {
    expect(facingSide(100, 10)).toBe("right");
    expect(facingSide(-100, 10)).toBe("left");
    expect(facingSide(100, -10)).toBe("right");
  });

  it("picks the vertical side when the vertical delta dominates", () => {
    expect(facingSide(10, 100)).toBe("bottom");
    expect(facingSide(10, -100)).toBe("top");
    expect(facingSide(-10, -100)).toBe("top");
  });

  it("falls back to vertical on an exact tie, so it never picks a side arbitrarily", () => {
    expect(facingSide(50, 50)).toBe("bottom");
    expect(facingSide(50, -50)).toBe("top");
  });

  it("re-picks the opposite side once the dominant axis flips — the actual bug report", () => {
    // A child dragged from directly right of its parent (right) to directly
    // below it (bottom) must stop routing through "right" once dy overtakes dx.
    expect(facingSide(120, 5)).toBe("right");
    expect(facingSide(5, 120)).toBe("bottom");
  });
});

describe("withDepths", () => {
  it("assigns BFS depth to mind nodes from a root", () => {
    const nodes = [
      { id: "root", type: "mind" as const, position: { x: 0, y: 0 }, data: {} },
      { id: "child", type: "mind" as const, position: { x: 0, y: 0 }, data: {} },
      { id: "grandchild", type: "mind" as const, position: { x: 0, y: 0 }, data: {} },
    ];
    const edges = [
      { id: "e1", source: "root", target: "child" },
      { id: "e2", source: "child", target: "grandchild" },
    ];
    const result = withDepths(nodes, edges);
    expect(depthOf(result, "root")).toBe(0);
    expect(depthOf(result, "child")).toBe(1);
    expect(depthOf(result, "grandchild")).toBe(2);
  });

  it("treats legacy untyped nodes as mind nodes for backward compatibility", () => {
    const nodes = [
      { id: "a", position: { x: 0, y: 0 }, data: {} } as unknown as Parameters<
        typeof withDepths
      >[0][number],
    ];
    const result = withDepths(nodes, []);
    expect(result[0].type).toBe("mind");
    expect(depthOf(result, "a")).toBe(0);
  });

  it("passes sticky notes through untouched — no depth, no type coercion", () => {
    const nodes = [
      { id: "root", type: "mind" as const, position: { x: 0, y: 0 }, data: {} },
      {
        id: "sticky-1",
        type: "sticky" as const,
        position: { x: 200, y: 200 },
        width: 150,
        height: 100,
        data: { text: "reminder", color: "#f3dfa3" },
      },
    ];
    const result = withDepths(nodes, []);
    const sticky = result.find((n) => n.id === "sticky-1");
    expect(sticky?.type).toBe("sticky");
    expect(sticky?.data).toEqual({ text: "reminder", color: "#f3dfa3" });
    expect((sticky?.data as { depth?: number }).depth).toBeUndefined();
  });

  it("doesn't let a sticky note's id collide with the tree's parent/child bookkeeping", () => {
    // Sticky notes never appear as an edge source/target, so they must never
    // be assigned a depth even if a mind node happens to share no relation.
    const nodes = [
      { id: "orphan", type: "mind" as const, position: { x: 0, y: 0 }, data: {} },
      {
        id: "sticky-1",
        type: "sticky" as const,
        position: { x: 0, y: 0 },
        data: { text: "", color: "#f3dfa3" },
      },
    ];
    const result = withDepths(nodes, []);
    expect(depthOf(result, "orphan")).toBe(0);
    expect(result.find((n) => n.id === "sticky-1")?.type).toBe("sticky");
  });
});
