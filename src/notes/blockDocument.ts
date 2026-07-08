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

/** An inline `@node + instruction` action (F15) parsed from one block. */
export interface InlineCommand {
  mention: { kind: string; id: string; label: string };
  instruction: string;
}

/**
 * Parse the last `@`-mention in a block's inline content plus the text that
 * follows it into an inline command (F15): e.g. `@spec translate to German` →
 * `{ mention: spec, instruction: "translate to German" }`. Returns `null` when
 * there is no mention or no trailing instruction. Pure and schema-agnostic.
 */
export function extractInlineCommand(blockContent: unknown): InlineCommand | null {
  if (!Array.isArray(blockContent)) return null;
  let mentionIndex = -1;
  for (let i = 0; i < blockContent.length; i++) {
    if ((blockContent[i] as { type?: unknown })?.type === "mention") mentionIndex = i;
  }
  if (mentionIndex === -1) return null;

  const m = blockContent[mentionIndex] as {
    props?: { kind?: unknown; id?: unknown; label?: unknown };
  };
  const id = typeof m.props?.id === "string" ? m.props.id : "";
  if (!id) return null;
  const kind = typeof m.props?.kind === "string" ? m.props.kind : "note";
  const label = typeof m.props?.label === "string" ? m.props.label : "";

  let instruction = "";
  for (let i = mentionIndex + 1; i < blockContent.length; i++) {
    const item = blockContent[i] as { type?: unknown; text?: unknown };
    if (item?.type === "text" && typeof item.text === "string") instruction += item.text;
  }

  const trimmed = instruction.trim();
  if (!trimmed) return null;
  return { mention: { kind, id, label }, instruction: trimmed };
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
