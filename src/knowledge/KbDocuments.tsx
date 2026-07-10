import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  KB_ACCEPT_EXTENSIONS,
  type KbDocument,
  type KbProgress,
  kbAddDocuments,
  kbDeleteDocument,
  kbListDocuments,
  kbReindexDocument,
} from "../lib/kb";
import { FileIcon, PlusIcon, TrashIcon } from "../ui/icons";

/** Human-readable file size. */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Document list + ingestion for one collection. Files are added via the picker
 * or by dropping them anywhere in the window; each ingests on-device in the
 * background, and this list reflects live status from the `kb:progress` event.
 */
export function KbDocuments({
  collectionId,
  embedderAvailable,
  onChanged,
}: {
  collectionId: string;
  embedderAvailable: boolean | null;
  onChanged: () => void;
}) {
  const [documents, setDocuments] = useState<KbDocument[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [dragActive, setDragActive] = useState(false);
  const [busy, setBusy] = useState(false);
  // Read the current collection inside stable event callbacks without
  // resubscribing on every change.
  const collectionRef = useRef(collectionId);
  collectionRef.current = collectionId;

  const load = useCallback(async () => {
    try {
      setDocuments(await kbListDocuments(collectionId));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [collectionId]);

  useEffect(() => {
    void load();
  }, [load]);

  // Refresh as documents transition pending → processing → ready/failed.
  useEffect(() => {
    const unlisten = listen<KbProgress>("kb:progress", (event) => {
      if (event.payload.collectionId === collectionRef.current) {
        void load();
        onChanged();
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [load, onChanged]);

  const addPaths = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      setBusy(true);
      try {
        await kbAddDocuments(collectionRef.current, paths);
        await load();
        onChanged();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [load, onChanged],
  );

  const pick = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        title: "Add documents",
        filters: [{ name: "Documents", extensions: KB_ACCEPT_EXTENSIONS }],
      });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      await addPaths(paths);
    } catch (e) {
      setError(String(e));
    }
  }, [addPaths]);

  // Accept files dropped anywhere while this collection is open.
  useEffect(() => {
    const promise = getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as { type: string; paths?: string[] };
      if (payload.type === "enter" || payload.type === "over") {
        setDragActive(true);
      } else if (payload.type === "leave") {
        setDragActive(false);
      } else if (payload.type === "drop") {
        setDragActive(false);
        if (payload.paths?.length) void addPaths(payload.paths);
      }
    });
    return () => {
      void promise.then((unlisten) => unlisten());
    };
  }, [addPaths]);

  const remove = async (id: string) => {
    try {
      await kbDeleteDocument(id);
      await load();
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const reindex = async (id: string) => {
    try {
      await kbReindexDocument(id);
      await load();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className={`kb-docs${dragActive ? " drag" : ""}`}>
      <div className="kb-docs-toolbar">
        <button
          type="button"
          className="btn-sm btn-primary"
          onClick={() => void pick()}
          disabled={busy}
        >
          <PlusIcon /> Add documents
        </button>
        <span className="muted kb-docs-hint">
          or drop files here — PDF, Word, Excel, CSV, images…
        </span>
      </div>

      {embedderAvailable === false ? (
        <p className="kb-docs-warn">
          The on-device embedding model (Ollama) isn't running, so new documents can't be indexed
          yet. Start Ollama, then use Reindex.
        </p>
      ) : null}
      {error ? (
        <p role="alert" className="kb-docs-warn">
          {error}
        </p>
      ) : null}

      {documents.length === 0 ? (
        <div className="kb-docs-empty">
          <FileIcon className="kb-docs-empty-icon" />
          <p className="muted">
            No documents yet. Add PDFs, Word/Excel files, CSVs, or images and Arya will read them
            on-device.
          </p>
        </div>
      ) : (
        <ul className="kb-doc-list plain">
          {documents.map((doc) => (
            <li key={doc.id} className="kb-doc">
              <FileIcon className="kb-doc-icon" />
              <div className="kb-doc-main">
                <div className="kb-doc-name" title={doc.filename}>
                  {doc.filename}
                </div>
                <div className="kb-doc-meta">
                  <DocStatus doc={doc} />
                </div>
                {doc.status === "failed" && doc.error ? (
                  <div className="kb-doc-error">{doc.error}</div>
                ) : null}
              </div>
              <div className="kb-doc-actions">
                {doc.status === "failed" ? (
                  <button
                    type="button"
                    className="btn-sm btn-ghost"
                    onClick={() => void reindex(doc.id)}
                  >
                    Reindex
                  </button>
                ) : null}
                <button
                  type="button"
                  className="btn-icon"
                  aria-label={`Delete ${doc.filename}`}
                  title="Delete"
                  onClick={() => void remove(doc.id)}
                >
                  <TrashIcon />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {dragActive ? (
        <div className="kb-dropzone-overlay">Drop to add to this collection</div>
      ) : null}
    </div>
  );
}

/** The status line for one document: a badge plus a short summary. */
function DocStatus({ doc }: { doc: KbDocument }) {
  if (doc.status === "pending" || doc.status === "processing") {
    return (
      <>
        <span className="badge badge-warning kb-badge-pulse">Indexing…</span>
        <span className="muted">{formatSize(doc.byteSize)}</span>
      </>
    );
  }
  if (doc.status === "failed") {
    return <span className="badge badge-danger">Failed</span>;
  }
  const kind = doc.extractor === "ocr" ? "OCR" : "Text";
  return (
    <>
      <span className="badge badge-success">Ready</span>
      <span className="badge badge-accent">{kind}</span>
      <span className="muted">
        {doc.chunkCount} chunk{doc.chunkCount === 1 ? "" : "s"}
        {doc.pageCount > 1 ? ` · ${doc.pageCount} pages` : ""} · {formatSize(doc.byteSize)}
      </span>
    </>
  );
}
