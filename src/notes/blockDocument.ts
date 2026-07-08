import type { PartialBlock } from "@blocknote/core";

/** A mention's target node, extracted from a document for edge reconciliation. */
export interface MentionTarget {
  kind: string;
  id: string;
}

/**
 * Walk a BlockNote document (blocks → inline content → nested children) and
 * collect every mention's target `(kind, id)`, de-duplicated. Schema-agnostic
 * and pure — no editor runtime — so it is unit-testable in isolation.
 */
export function extractMentionTargets(blocks: unknown): MentionTarget[] {
  const out: MentionTarget[] = [];
  const seen = new Set<string>();
  const walk = (nodes: unknown) => {
    if (!Array.isArray(nodes)) return;
    for (const node of nodes) {
      if (!node || typeof node !== "object") continue;
      const n = node as {
        type?: unknown;
        props?: { kind?: unknown; id?: unknown };
        content?: unknown;
        children?: unknown;
      };
      if (n.type === "mention" && typeof n.props?.id === "string" && n.props.id) {
        const kind = typeof n.props.kind === "string" ? n.props.kind : "note";
        const key = `${kind}:${n.props.id}`;
        if (!seen.has(key)) {
          seen.add(key);
          out.push({ kind, id: n.props.id });
        }
      }
      walk(n.content);
      walk(n.children);
    }
  };
  walk(blocks);
  return out;
}

/**
 * Parse stored BlockNote JSON into initial editor content, or `undefined` for an
 * empty editor. Invalid or empty JSON returns `undefined` (never throws), so a
 * corrupt or missing document degrades to empty and the legacy-markdown path can
 * fill it. Pure and view-free (`PartialBlock` is a type-only import, erased at
 * runtime), so it carries no BlockNote runtime dependency and is unit-testable.
 */
export function parseInitialContent(documentJson: string): PartialBlock[] | undefined {
  // Defensive: persisted data (or a stale caller) could hand us a non-string or
  // corrupt JSON; never let that crash the editor — degrade to empty instead.
  if (typeof documentJson !== "string" || !documentJson.trim()) return undefined;
  try {
    const parsed: unknown = JSON.parse(documentJson);
    return Array.isArray(parsed) && parsed.length > 0 ? (parsed as PartialBlock[]) : undefined;
  } catch {
    return undefined;
  }
}
