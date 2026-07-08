import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ForceGraph2D from "react-force-graph-2d";
import { type Graph, galaxyGraph } from "../lib/galaxy";

/** Node/link colours. Canvas can't read CSS variables, so these are concrete
 * values chosen to read on both the light and dark grounds. */
const NODE_COLOR: Record<string, string> = {
  note: "#6f8cf0",
  dictation: "#e0954e",
};
const EDGE_COLOR: Record<string, string> = {
  mention: "rgba(111, 140, 240, 0.55)",
  child: "rgba(150, 150, 160, 0.45)",
  semantic: "rgba(224, 149, 78, 0.45)",
};

type GNode = { id: string; kind: string; label: string; dim?: boolean };
type GLink = { source: string; target: string; relation: string };

/** Galaxy (F10): a 2D force-directed view of the connected brain — notes and
 * dictations as stars, `@`-mentions / nesting / semantic similarity as links. */
export function GalaxyPanel() {
  const [graph, setGraph] = useState<Graph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const wrapRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 800, h: 600 });

  const load = useCallback(async () => {
    try {
      setGraph(await galaxyGraph());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Size the canvas to its container.
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const update = () => setSize({ w: el.clientWidth, h: el.clientHeight });
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // react-force-graph mutates node/link objects (adds x/y and resolves link
  // endpoints), so hand it fresh clones each load/filter.
  const data = useMemo(() => {
    if (!graph) return { nodes: [] as GNode[], links: [] as GLink[] };
    const q = query.trim().toLowerCase();
    return {
      nodes: graph.nodes.map((n) => ({
        ...n,
        dim: q ? !n.label.toLowerCase().includes(q) : false,
      })),
      links: graph.edges.map((e) => ({ ...e })),
    };
  }, [graph, query]);

  return (
    <div className="screen">
      <div className="panel panel-grow" style={{ display: "flex", flexDirection: "column" }}>
        <div className="panel-head hstack spread">
          <div>
            <div className="panel-title">Galaxy</div>
            <div className="muted" style={{ fontSize: 12.5 }}>
              {graph
                ? `${graph.nodes.length} stars · ${graph.edges.length} connections`
                : "loading…"}
            </div>
          </div>
          <div className="hstack" style={{ gap: 8 }}>
            <input
              aria-label="highlight nodes"
              placeholder="Highlight…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ width: 160 }}
            />
            <button type="button" className="btn-sm btn-ghost" onClick={() => void load()}>
              Refresh
            </button>
          </div>
        </div>
        {error ? (
          <p role="alert" style={{ padding: "0 16px" }}>
            {error}
          </p>
        ) : null}
        <div ref={wrapRef} style={{ flex: 1, minHeight: 0, position: "relative" }}>
          {graph && graph.nodes.length > 0 ? (
            <ForceGraph2D<GNode, GLink>
              graphData={data}
              width={size.w}
              height={size.h}
              backgroundColor="rgba(0,0,0,0)"
              nodeRelSize={5}
              nodeLabel={(n) => n.label}
              nodeColor={(n) => (n.dim ? "rgba(150,150,160,0.25)" : (NODE_COLOR[n.kind] ?? "#888"))}
              linkColor={(l) => EDGE_COLOR[l.relation] ?? "rgba(150,150,160,0.35)"}
              linkWidth={1}
              cooldownTicks={100}
            />
          ) : graph ? (
            <div className="empty" style={{ paddingTop: 48 }}>
              Your galaxy is empty. Create notes and @-mention across them — the connections appear
              here.
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
