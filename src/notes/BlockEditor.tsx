import "@blocknote/core/fonts/inter.css";
import "@blocknote/ariakit/style.css";
import { BlockNoteView } from "@blocknote/ariakit";
import { useCreateBlockNote } from "@blocknote/react";
import { useEffect, useMemo, useState } from "react";
import { parseInitialContent } from "./blockDocument";

/** The app's current colour scheme: follow `data-theme`, or the OS when it's
 * "system"/unset. Module-level so it isn't an effect dependency. */
function resolveScheme(): "light" | "dark" {
  const attr = document.documentElement.getAttribute("data-theme");
  if (attr === "dark" || attr === "light") return attr;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Track the resolved colour scheme so the editor matches the app live. */
function useResolvedScheme(): "light" | "dark" {
  const [scheme, setScheme] = useState<"light" | "dark">(resolveScheme);
  useEffect(() => {
    const update = () => setScheme(resolveScheme());
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", update);
    return () => {
      observer.disconnect();
      media.removeEventListener("change", update);
    };
  }, []);
  return scheme;
}

interface BlockEditorProps {
  /** BlockNote block-JSON; empty for a legacy note. */
  initialDocumentJson: string;
  /** Markdown fallback for legacy notes, converted to blocks on first mount. */
  initialBodyMd: string;
  /** Fired on every edit with the block-JSON and its markdown projection. */
  onChange: (documentJson: string, bodyMd: string) => void;
}

/**
 * The note body editor (F2): a BlockNote block editor. Content is stored as
 * block-JSON (`documentJson`), and a markdown projection (`bodyMd`) is emitted on
 * every change so full-text search and the RAG index keep working. A legacy note
 * that only has `body_md` is converted to blocks once on mount (which persists it
 * as block-JSON via the resulting change).
 *
 * Mount this keyed by note id so it re-initializes when the open note changes;
 * it is otherwise uncontrolled (BlockNote owns the live document).
 */
export function BlockEditor({ initialDocumentJson, initialBodyMd, onChange }: BlockEditorProps) {
  const scheme = useResolvedScheme();
  const initialContent = useMemo(
    () => parseInitialContent(initialDocumentJson),
    [initialDocumentJson],
  );
  const editor = useCreateBlockNote({ initialContent });

  // Legacy path: a note saved before the block editor has markdown but no
  // block-JSON. Convert it once so the user sees their existing content.
  useEffect(() => {
    if (initialContent !== undefined || !initialBodyMd.trim()) return;
    const blocks = editor.tryParseMarkdownToBlocks(initialBodyMd);
    if (blocks.length > 0) {
      editor.replaceBlocks(editor.document, blocks);
    }
  }, [editor, initialContent, initialBodyMd]);

  return (
    <BlockNoteView
      editor={editor}
      theme={scheme}
      onChange={() => {
        const doc = editor.document;
        onChange(JSON.stringify(doc), editor.blocksToMarkdownLossy(doc));
      }}
    />
  );
}
