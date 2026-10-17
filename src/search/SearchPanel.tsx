import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { FileIcon, SearchIcon } from "../ui/icons";
import { PaneState } from "../ui/PaneState";

/** Hide semantic matches below this cosine score. Literal text matches are
 *  unaffected. Below ~0.45 the embedding is "close enough" in vector space
 *  for the surface to look unrelated — anything from notes to dictations to
 *  an unrelated ARIA conversation starts to look like a hit, which makes the
 *  search panel feel broken. */
const SEMANTIC_MIN_SCORE = 0.45;

interface SemanticHit {
  sourceKind: string;
  sourceId: string;
  title: string;
  content: string;
  score: number;
}

interface TextHit {
  sourceKind: string;
  sourceId: string;
  title: string;
  snippet: string;
  createdAt: string;
}

type Match = { kind: "text" } | { kind: "semantic"; score: number };

interface DisplayHit {
  sourceKind: string;
  sourceId: string;
  title: string;
  content: string;
  match: Match;
}

interface RagStatus {
  embedderAvailable: boolean;
  indexedChunks: number;
}

/**
 * Map a search hit's source kind to the deep-link kind the shell understands.
 * "session" and "transcript" are agent/meeting content that open as their owning
 * node (agent tab / note) rather than a dedicated surface.
 */
function openHit(hit: DisplayHit) {
  const kind =
    hit.sourceKind === "session"
      ? "agent"
      : hit.sourceKind === "transcript"
        ? "note"
        : hit.sourceKind === "kb_document"
          ? "knowledge"
          : hit.sourceKind;
  window.dispatchEvent(new CustomEvent("arya:open-node", { detail: { kind, id: hit.sourceId } }));
}

/**
 * Search across everything (F14). A literal title+content pass (`search_all`)
 * always runs and works offline; when the local embedder is up, semantic results
 * (`rag_search`) are merged in. Exact/text matches rank first, then semantic by
 * score. Deduplicated by node.
 */
export function SearchPanel() {
  const [status, setStatus] = useState<RagStatus | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<DisplayHit[]>([]);
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
    const q = query.trim();
    if (!q) return;
    setBusy(true);
    try {
      const [text, semantic] = await Promise.all([
        invoke<TextHit[]>("search_all", { query: q, limit: 20 }),
        status?.embedderAvailable
          ? invoke<SemanticHit[]>("rag_search", { query: q, limit: 10 })
          : Promise.resolve<SemanticHit[]>([]),
      ]);

      const byNode = new Map<string, DisplayHit>();
      for (const h of text) {
        byNode.set(`${h.sourceKind}:${h.sourceId}`, {
          sourceKind: h.sourceKind,
          sourceId: h.sourceId,
          title: h.title,
          content: h.snippet,
          match: { kind: "text" },
        });
      }
      for (const h of semantic) {
        const key = `${h.sourceKind}:${h.sourceId}`;
        // A literal (exact) hit already covers this node; keep it.
        if (byNode.has(key)) continue;
        byNode.set(key, {
          sourceKind: h.sourceKind,
          sourceId: h.sourceId,
          title: h.title,
          content: h.content,
          match: { kind: "semantic", score: h.score },
        });
      }

      const scoreOf = (m: Match) => (m.kind === "semantic" ? m.score : 1);
      const merged = [...byNode.values()]
        // Hide semantic results below a relevance threshold. A literal (text)
        // hit always passes. The cosine-sim scores below ~0.45 are essentially
        // noise — they pick up anything because the embedding is "close enough"
        // in a high-dimensional space even when the topic has nothing to do
        // with the query, which surfaces unrelated dictations and looks like
        // the search is broken. Bumping the threshold surfaces the empty
        // state ("no close matches") instead.
        .filter((h) => (h.match.kind === "text" ? true : h.match.score >= SEMANTIC_MIN_SCORE))
        .sort((a, b) => {
          // Exact/text matches first, then semantic by descending score.
          const rank = (m: Match) => (m.kind === "text" ? 1 : 0);
          return rank(b.match) - rank(a.match) || scoreOf(b.match) - scoreOf(a.match);
        });

      setHits(merged);
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
            Search notes, dictations, mind maps, and Knowledge collections by title or content —
            on-device.
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
            placeholder="Search your notes, meetings, and dictations…"
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
              ? `${status.indexedChunks} items indexed · ${status.embedderAvailable ? "semantic + text" : "text search (start Ollama for semantic)"}`
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

        {!searched ? (
          <PaneState
            kind="empty"
            title="Type a query to search"
            description="Matches titles, transcript text, and dictation content. Search runs locally — nothing leaves your Mac."
          />
        ) : null}

        <ul aria-label="search results" className="plain">
          {hits.map((hit) => (
            <li key={`${hit.sourceKind}:${hit.sourceId}`}>
              <button
                type="button"
                className="result-card"
                onClick={() => openHit(hit)}
                title="Open this result"
              >
                <div className="result-meta">
                  <FileIcon className="result-kind-icon" />
                  <span>
                    {hit.sourceKind === "kb_document" ? "Knowledge" : hit.sourceKind} · {hit.title}
                  </span>
                  <span className="result-score">
                    {hit.match.kind === "semantic"
                      ? `${pct(hit.match.score)}% match`
                      : "text match"}
                  </span>
                </div>
                <div style={{ fontSize: 14, color: "var(--text)", lineHeight: 1.6 }}>
                  {hit.content.slice(0, 320)}
                </div>
              </button>
            </li>
          ))}
          {searched && hits.length === 0 && !busy ? (
            <li>
              <PaneState
                kind="empty"
                title="No matches"
                description="Try different words, or Reindex if you've added content."
              />
            </li>
          ) : null}
        </ul>
      </div>
    </div>
  );
}
