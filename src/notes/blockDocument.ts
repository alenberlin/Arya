import type { PartialBlock } from "@blocknote/core";

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
