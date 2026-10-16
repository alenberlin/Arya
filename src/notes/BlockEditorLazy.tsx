// Lazy wrapper around BlockEditor so the @blocknote/* CSS + modules only
// load the first time the user actually opens a note — keeping the Notes
// pane's first paint free of editor-weighting JS.

import { type ComponentProps, lazy, Suspense } from "react";
import type { MentionItem } from "./BlockEditor";
import { BlockEditor } from "./BlockEditor";

const LazyBlockEditor = lazy(async () => ({ default: BlockEditor }));

export type BlockEditorProps = ComponentProps<typeof LazyBlockEditor>;

/** Skeleton shown while the BlockNote bundle (and its CSS) download. The
 * editor pane only opens when a note is selected, so this fallback is
 * typically seen for a few hundred ms the first time. */
function EditorSkeleton() {
  return (
    <div
      className="block-editor-skeleton"
      aria-busy="true"
      style={{
        padding: "16px 22px",
        color: "var(--text-muted)",
        fontSize: 13,
      }}
    >
      Loading editor…
    </div>
  );
}

export function BlockEditorLazy(props: BlockEditorProps) {
  return (
    <Suspense fallback={<EditorSkeleton />}>
      <LazyBlockEditor {...props} />
    </Suspense>
  );
}

// Re-export the type-only mention item so existing imports keep working.
export type { MentionItem };
