import { useEffect, useState } from "react";
import { type Link, listLinksTo } from "../lib/links";
import type { NoteSummary } from "../lib/notes";

interface BacklinksPanelProps {
  /** The note whose inbound mentions to show. */
  noteId: string;
  /** Source titles are resolved from the loaded notes list. */
  notes: NoteSummary[];
  onOpen: (kind: string, id: string) => void;
}

/**
 * Backlinks (F3): the nodes that `@`-mention the open note. Reads inbound edges
 * from the connected-brain graph and lets the user jump to each source.
 */
export function BacklinksPanel({ noteId, notes, onOpen }: BacklinksPanelProps) {
  const [links, setLinks] = useState<Link[]>([]);

  useEffect(() => {
    let active = true;
    void listLinksTo("note", noteId)
      .then((next) => {
        if (active) setLinks(Array.isArray(next) ? next : []);
      })
      .catch(() => {
        if (active) setLinks([]);
      });
    return () => {
      active = false;
    };
  }, [noteId]);

  if (links.length === 0) return null;

  const titleFor = (link: Link) => notes.find((n) => n.id === link.sourceId)?.title ?? "Untitled";

  return (
    <div style={{ marginTop: 22 }}>
      <span className="section-label" style={{ display: "block", marginBottom: 8 }}>
        Linked from · {links.length}
      </span>
      <ul aria-label="backlinks" className="plain">
        {links.map((link) => (
          <li key={link.id}>
            <button
              type="button"
              className="backlink"
              onClick={() => onOpen(link.sourceKind, link.sourceId)}
            >
              {titleFor(link)}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
