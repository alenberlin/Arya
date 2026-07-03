import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

interface SearchHit {
  sourceKind: string;
  sourceId: string;
  title: string;
  content: string;
  score: number;
}

interface RagStatus {
  embedderAvailable: boolean;
  indexedChunks: number;
}

/** Semantic search over the whole workspace, fully local. */
export function SearchPanel() {
  const [status, setStatus] = useState<RagStatus | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await invoke<RagStatus>("rag_status"));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const onSearch = async () => {
    if (!query.trim()) return;
    setBusy(true);
    try {
      setHits(await invoke<SearchHit[]>("rag_search", { query: query.trim(), limit: 10 }));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onReindex = async () => {
    setBusy(true);
    try {
      await invoke<number>("rag_reindex");
      await refreshStatus();
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section>
      <h2>Search your workspace</h2>
      <p>
        <small>
          Semantic search across notes, transcripts, dictations, and agent sessions. Runs entirely
          on this Mac.
          {status
            ? ` ${status.indexedChunks} chunks indexed · embedder ${status.embedderAvailable ? "ready" : "offline (start Ollama)"}`
            : ""}
        </small>
      </p>
      {error ? <p role="alert">{error}</p> : null}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void onSearch();
        }}
        style={{ display: "flex", gap: 6, maxWidth: 560 }}
      >
        <input
          aria-label="search query"
          placeholder="Ask anything about your notes and meetings…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ flex: 1 }}
        />
        <button type="submit" disabled={busy}>
          Search
        </button>
        <button type="button" onClick={() => void onReindex()} disabled={busy}>
          Reindex
        </button>
      </form>
      <ul aria-label="search results" style={{ listStyle: "none", padding: 0 }}>
        {hits.map((hit) => (
          <li
            key={`${hit.sourceId}-${hit.content.slice(0, 24)}`}
            style={{ borderTop: "1px solid var(--border)", padding: "8px 0" }}
          >
            <strong>{hit.title}</strong>{" "}
            <small>
              {hit.sourceKind} · score {hit.score.toFixed(2)}
            </small>
            <div style={{ fontSize: 13, color: "var(--text-secondary)" }}>
              {hit.content.slice(0, 320)}
            </div>
          </li>
        ))}
        {hits.length === 0 && !busy ? <li>No results yet.</li> : null}
      </ul>
    </section>
  );
}
