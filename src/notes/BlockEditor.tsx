import "@blocknote/core/fonts/inter.css";
import "@blocknote/ariakit/style.css";
import { BlockNoteView } from "@blocknote/ariakit";
import { filterSuggestionItems } from "@blocknote/core";
import { SuggestionMenuController, useCreateBlockNote } from "@blocknote/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  extractInlineCommand,
  extractMentionTargets,
  type MentionTarget,
  parseInitialContent,
} from "./blockDocument";
import { notesSchema } from "./mentionSchema";

/** A node offered in the `@`-mention menu. */
export interface MentionItem {
  kind: "note" | "dictation" | "meeting" | "mindmap";
  id: string;
  label: string;
}

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
  /** Nodes offered in the `@`-mention menu. */
  mentionItems: MentionItem[];
  /** Fired on every edit: block-JSON, its markdown projection, and mention targets. */
  onChange: (documentJson: string, bodyMd: string, mentions: MentionTarget[]) => void;
  /** Navigate to a mentioned node when its chip is clicked. */
  onOpenNode: (kind: string, id: string) => void;
  /** Run an inline `@node + instruction` action (⌘↵). Returns the result text to
   * insert after the block, or empty to insert nothing. */
  onInlineCommand?: (
    mention: { kind: string; id: string; label: string },
    instruction: string,
  ) => Promise<string>;
}

/**
 * The note body editor (F2/F1): a BlockNote block editor with `@`-mentions that
 * link to other nodes in the connected brain. Content is stored as block-JSON
 * (`documentJson`) with a markdown projection (`bodyMd`) for search; the current
 * mention targets are emitted alongside so the caller can reconcile edges.
 * Legacy notes convert their markdown to blocks once on mount.
 *
 * Mount keyed by note id so it re-initializes when the open note changes.
 */
export function BlockEditor({
  initialDocumentJson,
  initialBodyMd,
  mentionItems,
  onChange,
  onOpenNode,
  onInlineCommand,
}: BlockEditorProps) {
  const scheme = useResolvedScheme();
  const initialContent = useMemo(
    () => parseInitialContent(initialDocumentJson),
    [initialDocumentJson],
  );
  const editor = useCreateBlockNote({ schema: notesSchema, initialContent });
  const containerRef = useRef<HTMLDivElement>(null);

  // Legacy path: a note with markdown but no block-JSON is converted once so the
  // user sees their existing content (which persists it via the resulting change).
  useEffect(() => {
    if (initialContent !== undefined || !initialBodyMd.trim()) return;
    const blocks = editor.tryParseMarkdownToBlocks(initialBodyMd);
    if (blocks.length > 0) {
      editor.replaceBlocks(editor.document, blocks);
    }
  }, [editor, initialContent, initialBodyMd]);

  // Navigate when a mention chip is clicked. Delegated via a native listener
  // (rather than a JSX onClick on a non-interactive container) so the chips can
  // stay plain spans inside the contenteditable.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const handler = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;
      const chip = target.closest(".mention-chip");
      const kind = chip?.getAttribute("data-kind");
      const id = chip?.getAttribute("data-id");
      if (kind && id) onOpenNode(kind, id);
    };
    el.addEventListener("click", handler);
    return () => el.removeEventListener("click", handler);
  }, [onOpenNode]);

  // F15: ⌘↵ (or Ctrl+↵) on a block ending in "@node <instruction>" runs the AI
  // action — resolve the node's content, apply the instruction — and inserts the
  // result after the block. The mention stays in place as provenance.
  useEffect(() => {
    const el = containerRef.current;
    if (!el || !onInlineCommand) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key !== "Enter" || !(event.metaKey || event.ctrlKey)) return;
      const block = editor.getTextCursorPosition().block;
      const command = extractInlineCommand(block.content);
      if (!command) return;
      event.preventDefault();
      void onInlineCommand(command.mention, command.instruction).then((result) => {
        if (result.trim()) {
          editor.insertBlocks(editor.tryParseMarkdownToBlocks(result), block, "after");
        }
      });
    };
    el.addEventListener("keydown", handler);
    return () => el.removeEventListener("keydown", handler);
  }, [editor, onInlineCommand]);

  const emit = useCallback(() => {
    const doc = editor.document;
    onChange(JSON.stringify(doc), editor.blocksToMarkdownLossy(doc), extractMentionTargets(doc));
  }, [editor, onChange]);

  const getMentionItems = useCallback(
    (query: string) =>
      filterSuggestionItems(
        mentionItems.map((item) => ({
          title: item.label,
          onItemClick: () => {
            editor.insertInlineContent([
              { type: "mention", props: { kind: item.kind, id: item.id, label: item.label } },
              " ",
            ]);
          },
        })),
        query,
      ),
    [editor, mentionItems],
  );

  return (
    <div className="block-editor" ref={containerRef}>
      <BlockNoteView editor={editor} theme={scheme} onChange={emit}>
        <SuggestionMenuController
          triggerCharacter="@"
          getItems={async (query) => getMentionItems(query)}
        />
      </BlockNoteView>
    </div>
  );
}
