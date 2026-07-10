import { useCallback, useEffect, useRef, useState } from "react";
import {
  type KbCollection,
  kbCreateCollection,
  kbDeleteCollection,
  kbListCollections,
  kbRenameCollection,
  kbStatus,
} from "../lib/kb";
import { ConfirmDialog, PromptDialog } from "../ui/dialogs";
import { KnowledgeIcon, MoreIcon, PlusIcon } from "../ui/icons";
import { KbChat } from "./KbChat";
import { KbDocuments } from "./KbDocuments";

/**
 * Knowledge Base surface (top-level, between Galaxy and Mind Map): a left rail
 * of collections and a detail pane per collection. A collection is a named RAG
 * database of uploaded documents you ingest on-device and chat against, grounded
 * in citations. This slice owns collection CRUD and status; the detail pane's
 * document ingestion and grounded chat are layered on in later slices.
 */
export function KnowledgeBasePanel() {
  const [collections, setCollections] = useState<KbCollection[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [embedderAvailable, setEmbedderAvailable] = useState<boolean | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<KbCollection | null>(null);
  const [deleting, setDeleting] = useState<KbCollection | null>(null);
  const [menuFor, setMenuFor] = useState<{ id: string; x: number; y: number } | null>(null);

  const reload = useCallback(async () => {
    try {
      const list = await kbListCollections();
      setCollections(list);
      setActiveId((prev) => {
        if (prev && list.some((c) => c.id === prev)) return prev;
        return list[0]?.id ?? null;
      });
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void reload();
    void kbStatus()
      .then((s) => setEmbedderAvailable(s.embedderAvailable))
      .catch(() => setEmbedderAvailable(false));
  }, [reload]);

  const onCreate = async (name: string) => {
    setCreating(false);
    try {
      const created = await kbCreateCollection(name);
      await reload();
      setActiveId(created.id);
    } catch (e) {
      setError(String(e));
    }
  };

  const onRename = async (name: string) => {
    const target = renaming;
    setRenaming(null);
    if (!target) return;
    try {
      await kbRenameCollection(target.id, name);
      await reload();
    } catch (e) {
      setError(String(e));
    }
  };

  const onDelete = async () => {
    const target = deleting;
    setDeleting(null);
    if (!target) return;
    try {
      await kbDeleteCollection(target.id);
      await reload();
    } catch (e) {
      setError(String(e));
    }
  };

  const active = collections.find((c) => c.id === activeId) ?? null;

  return (
    <div className="kb">
      <aside className="kb-rail">
        <div className="kb-rail-head">
          <h1 className="kb-rail-title">
            <KnowledgeIcon className="kb-rail-title-icon" />
            Knowledge
          </h1>
          <button
            type="button"
            className="btn-icon"
            aria-label="New collection"
            title="New collection"
            onClick={() => setCreating(true)}
          >
            <PlusIcon />
          </button>
        </div>

        <div className="kb-collections">
          {collections.map((c) => (
            <div key={c.id} className={`kb-collection${c.id === activeId ? " active" : ""}`}>
              <button
                type="button"
                className="kb-collection-main"
                onClick={() => setActiveId(c.id)}
              >
                <span className="kb-collection-name">{c.name}</span>
                <span className="kb-collection-meta">
                  {c.documentCount === 0
                    ? "empty"
                    : `${c.documentCount} doc${c.documentCount === 1 ? "" : "s"}`}
                  {c.documentCount > c.readyCount ? " · indexing…" : ""}
                </span>
              </button>
              <button
                type="button"
                className="btn-icon kb-collection-more"
                aria-label={`Options for ${c.name}`}
                onClick={(e) => setMenuFor({ id: c.id, x: e.clientX, y: e.clientY })}
              >
                <MoreIcon />
              </button>
            </div>
          ))}
          {loaded && collections.length === 0 ? (
            <button type="button" className="kb-empty-create" onClick={() => setCreating(true)}>
              <PlusIcon />
              <span>Create your first collection</span>
              <span className="muted">
                Drop in PDFs, docs, spreadsheets, or images and chat grounded in them.
              </span>
            </button>
          ) : null}
        </div>

        <div className="kb-rail-foot">
          <span
            className="tier-dot"
            style={{
              background:
                embedderAvailable == null
                  ? "var(--text-muted)"
                  : embedderAvailable
                    ? "var(--success)"
                    : "var(--warning)",
            }}
          />
          <span className="muted">
            {embedderAvailable == null
              ? "checking on-device model…"
              : embedderAvailable
                ? "On-device · private"
                : "Start Ollama to ingest & search"}
          </span>
        </div>
      </aside>

      <section className="kb-detail">
        {error ? (
          <p role="alert" className="kb-error">
            {error}
          </p>
        ) : null}
        {active ? (
          <CollectionDetail
            collection={active}
            embedderAvailable={embedderAvailable}
            onChanged={reload}
          />
        ) : (
          <div className="kb-detail-empty">
            <KnowledgeIcon className="kb-detail-empty-icon" />
            <h2>Your knowledge base</h2>
            <p className="muted">
              Create a collection, add your documents, and ask questions answered only from what you
              uploaded — entirely on your Mac.
            </p>
            <button type="button" className="btn-primary" onClick={() => setCreating(true)}>
              New collection
            </button>
          </div>
        )}
      </section>

      {menuFor ? (
        <>
          <button
            type="button"
            className="menu-backdrop"
            aria-label="Close menu"
            onClick={() => setMenuFor(null)}
          />
          <div className="context-menu" style={{ left: menuFor.x, top: menuFor.y }}>
            <button
              type="button"
              onClick={() => {
                const c = collections.find((x) => x.id === menuFor.id) ?? null;
                setMenuFor(null);
                setRenaming(c);
              }}
            >
              Rename
            </button>
            <button
              type="button"
              className="danger"
              onClick={() => {
                const c = collections.find((x) => x.id === menuFor.id) ?? null;
                setMenuFor(null);
                setDeleting(c);
              }}
            >
              Delete
            </button>
          </div>
        </>
      ) : null}

      <PromptDialog
        open={creating}
        title="New collection"
        label="Name"
        placeholder="e.g. Research papers, Contracts, Handbook"
        submitLabel="Create"
        onSubmit={onCreate}
        onCancel={() => setCreating(false)}
      />
      <PromptDialog
        open={renaming != null}
        title="Rename collection"
        label="Name"
        initialValue={renaming?.name ?? ""}
        submitLabel="Save"
        onSubmit={onRename}
        onCancel={() => setRenaming(null)}
      />
      <ConfirmDialog
        open={deleting != null}
        title={`Delete "${deleting?.name ?? ""}"?`}
        message="This permanently removes the collection, its uploaded documents, and its chats. This cannot be undone."
        confirmLabel="Delete"
        danger
        onConfirm={onDelete}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
}

/**
 * Detail pane for one collection: a header plus its documents. Grounded chat is
 * layered into the body in a later slice.
 */
function CollectionDetail({
  collection,
  embedderAvailable,
  onChanged,
}: {
  collection: KbCollection;
  embedderAvailable: boolean | null;
  onChanged: () => void;
}) {
  const headingRef = useRef<HTMLHeadingElement>(null);
  // Refocus the heading when the selection changes, for screen-reader context.
  useEffect(() => {
    headingRef.current?.focus?.();
  }, []);

  return (
    <div className="kb-collection-detail">
      <header className="kb-detail-head">
        <div className="kb-detail-heading">
          <h2 tabIndex={-1} ref={headingRef}>
            {collection.name}
          </h2>
          {collection.description ? <p className="muted">{collection.description}</p> : null}
        </div>
      </header>
      <div className="kb-detail-body kb-split">
        <div className="kb-docs-col">
          <KbDocuments
            key={collection.id}
            collectionId={collection.id}
            embedderAvailable={embedderAvailable}
            onChanged={onChanged}
          />
        </div>
        <div className="kb-chat-col">
          <KbChat
            key={collection.id}
            collectionId={collection.id}
            embedderAvailable={embedderAvailable}
            hasReadyDocs={collection.readyCount > 0}
          />
        </div>
      </div>
    </div>
  );
}
