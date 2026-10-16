// Pure presentational note row + tree helpers, extracted from NotesWorkspace
// so the main file can stay focused on local state and composition.

import type { JSX } from "react";
import type { NoteSummary } from "../lib/notes";
import { TrashIcon } from "../ui/icons";
import { formatWhen, statusChip } from "./format";

export interface NoteRowProps {
  note: NoteSummary;
  depth: number;
  treeMode: boolean;
  isOpen: boolean;
  kidsLength: number;
  isActive: boolean;
  onOpen: (id: string) => void;
  onContextMenu: (note: NoteSummary, x: number, y: number) => void;
  onToggleExpand: (id: string) => void;
  onRequestDelete: (id: string) => void;
}

/** Single row in the note list. In tree mode shows a twisty; flat mode hides it.
 * Drags are picked up by the parent <li> via the row's onDragStart handler. */
export function NoteRow({
  note,
  depth,
  treeMode,
  isOpen,
  kidsLength,
  isActive,
  onOpen,
  onContextMenu,
  onToggleExpand,
  onRequestDelete,
}: NoteRowProps): JSX.Element {
  return (
    <li className="note-row" style={treeMode ? { paddingLeft: 8 + depth * 14 } : undefined}>
      {treeMode ? (
        kidsLength > 0 ? (
          <button
            type="button"
            className="note-twisty"
            aria-label={`${isOpen ? "Collapse" : "Expand"} ${note.title}`}
            aria-expanded={isOpen}
            onClick={() => onToggleExpand(note.id)}
          >
            {isOpen ? "▾" : "▸"}
          </button>
        ) : (
          <span className="note-twisty note-twisty-empty" aria-hidden="true" />
        )
      ) : null}
      <button
        type="button"
        className="row"
        draggable
        aria-current={isActive ? "true" : undefined}
        onClick={() => void onOpen(note.id)}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", note.id);
          e.dataTransfer.effectAllowed = "move";
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          onContextMenu(note, e.clientX, e.clientY);
        }}
      >
        <div className="spread hstack" style={{ marginBottom: 4 }}>
          <span className="truncate" style={{ fontSize: 13.5, fontWeight: 500 }}>
            {note.title}
          </span>
          {statusChip(note.processingStatus)}
        </div>
        <div className="mono" style={{ fontSize: 10.5, color: "var(--text-muted)" }}>
          {formatWhen(note.createdAt)}
        </div>
      </button>
      <button
        type="button"
        className="note-del"
        aria-label={`delete ${note.title}`}
        title="Delete note"
        onClick={(e) => {
          e.stopPropagation();
          onRequestDelete(note.id);
        }}
      >
        <TrashIcon />
      </button>
    </li>
  );
}

export interface NoteTreeProps {
  items: NoteSummary[];
  depth: number;
  treeMode: boolean;
  expanded: Set<string>;
  childrenByParent: Map<string, NoteSummary[]>;
  activeNoteId: string | null;
  onOpen: (id: string) => void;
  onContextMenu: (note: NoteSummary, x: number, y: number) => void;
  onToggleExpand: (id: string) => void;
  onRequestDelete: (id: string) => void;
}

/** Recursive walk that flattens a notes tree (or list) into JSX rows. */
export function NoteTree({
  items,
  depth,
  treeMode,
  expanded,
  childrenByParent,
  activeNoteId,
  onOpen,
  onContextMenu,
  onToggleExpand,
  onRequestDelete,
}: NoteTreeProps): JSX.Element[] {
  return items.flatMap((note) => {
    const kids = childrenByParent.get(note.id) ?? [];
    const row = (
      <NoteRow
        key={note.id}
        note={note}
        depth={depth}
        treeMode={treeMode}
        isOpen={expanded.has(note.id)}
        kidsLength={kids.length}
        isActive={note.id === activeNoteId}
        onOpen={onOpen}
        onContextMenu={onContextMenu}
        onToggleExpand={onToggleExpand}
        onRequestDelete={onRequestDelete}
      />
    );
    if (!treeMode || !expanded.has(note.id) || kids.length === 0) return [row];
    return [
      row,
      ...NoteTree({
        items: kids,
        depth: depth + 1,
        treeMode,
        expanded,
        childrenByParent,
        activeNoteId,
        onOpen,
        onContextMenu,
        onToggleExpand,
        onRequestDelete,
      }),
    ];
  });
}
