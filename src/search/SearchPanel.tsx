import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { FileIcon, SearchIcon } from "../ui/icons";

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
  const [searched, setSearched] = useState(false);
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
      setSearched(true);
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

  const pct = (score: number) => Math.round(Math.min(1, Math.max(0, score)) * 100);

  return (
    <div className="screen-center">
      <div className="screen-col search">
        <div style={{ textAlign: "center", marginBottom: 24 }}>
          <h1 className="hero-title">Search everything</h1>
          <p className="muted" style={{ margin: "6px 0 0" }}>
            Ask in plain language across notes, transcripts and dictations — on-device.
          </p>
        </div>

        <form
          className="search-box"
          onSubmit={(e) => {
            e.preventDefault();
            void onSearch();
          }}
        >
          <SearchIcon className="search-icon" />
          <input
            aria-label="search query"
            placeholder="Ask anything about your notes and meetings…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <button type="submit" className="btn-sm btn-primary" disabled={busy}>
            Search
          </button>
        </form>

        <div className="hstack spread" style={{ margin: "10px 2px 18px", fontSize: 12 }}>
          <div className="hstack muted">
            <span
              className="tier-dot"
              style={{
                background: status?.embedderAvailable ? "var(--success)" : "var(--warning)",
              }}
            />
            {searched ? `${hits.length} results · ` : ""}
            {status
              ? `${status.indexedChunks} items indexed · ${status.embedderAvailable ? "engine ready" : "engine offline (start Ollama)"}`
              : "checking index…"}
          </div>
          <button
            type="button"
            className="btn-sm btn-ghost"
            onClick={() => void onReindex()}
            disabled={busy}
          >
            Reindex
          </button>
        </div>

        {error ? (
          <p role="alert" style={{ marginBottom: 12 }}>
            {error}
          </p>
        ) : null}

        <ul aria-label="search results" className="plain">
          {hits.map((hit) => (
            <li key={`${hit.sourceId}-${hit.content.slice(0, 24)}`}>
              <button type="button" className="result-card">
                <div className="result-meta">
                  <FileIcon className="result-kind-icon" />
                  <span>
                    {hit.sourceKind} · {hit.title}
                  </span>
                  <span className="result-score">{pct(hit.score)}% match</span>
                </div>
                <div style={{ fontSize: 14, color: "var(--text)", lineHeight: 1.6 }}>
                  {hit.content.slice(0, 320)}
                </div>
              </button>
            </li>
          ))}
          {searched && hits.length === 0 && !busy ? (
            <li className="empty">
              No matches. Try different words, or Reindex if you've added content.
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
