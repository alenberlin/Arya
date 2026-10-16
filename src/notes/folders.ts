// Folder-list presentation helpers shared by the list panel + sort dialog.

import type { Folder } from "../lib/notes";
import { MAX_VISIBLE_FOLDERS } from "./format";

/** Split folders into the inline chips and the overflow that folds into the
 * "More" dropdown. The active folder is always kept visible as a chip (swapped
 * into the last inline slot if it would otherwise overflow), so the current
 * selection is never hidden behind the menu. */
export function splitFolders(
  all: Folder[],
  active: string | null,
): { visible: Folder[]; overflow: Folder[] } {
  if (all.length <= MAX_VISIBLE_FOLDERS) return { visible: all, overflow: [] };
  let visible = all.slice(0, MAX_VISIBLE_FOLDERS);
  if (active && !visible.some((f) => f.id === active)) {
    const activeFolder = all.find((f) => f.id === active);
    if (activeFolder) visible = [...all.slice(0, MAX_VISIBLE_FOLDERS - 1), activeFolder];
  }
  const visibleIds = new Set(visible.map((f) => f.id));
  return { visible, overflow: all.filter((f) => !visibleIds.has(f.id)) };
}
