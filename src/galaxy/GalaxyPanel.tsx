import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ForceGraph2D, {
  type ForceGraphMethods,
  type LinkObject,
  type NodeObject,
} from "react-force-graph-2d";
import { type Graph, galaxyGraph } from "../lib/galaxy";
import "./galaxy.css";

type GNode = { id: string; kind: string; label: string };
type GLink = { source: string; target: string; relation: string };
type FGNode = NodeObject<GNode>;
type FGLink = LinkObject<GNode, GLink>;

const TAU = Math.PI * 2;
const CANVAS_SANS = "-apple-system, BlinkMacSystemFont, 'SF Pro Text', system-ui, sans-serif";

/** Warm categorical palette — one hue per node kind, readable on both the cream
 * and dark grounds. Canvas can't read CSS variables, so these are concrete and
 * are the single source shared by the nodes and the legend swatches. */
const CATEGORY: Record<string, { color: string; label: string }> = {
  note: { color: "#be5a38", label: "Notes" },
  dictation: { color: "#c2952f", label: "Dictations" },
  mindmap: { color: "#8a6ea8", label: "Mind maps" },
  meeting: { color: "#5e7b57", label: "Meetings" },
  agent: { color: "#4f7d8a", label: "Agent chats" },
};
const FALLBACK_COLOR = "#9a8f7d";
const categoryColor = (kind: string) => CATEGORY[kind]?.color ?? FALLBACK_COLOR;
const categoryLabel = (kind: string) =>
  CATEGORY[kind]?.label ?? kind.charAt(0).toUpperCase() + kind.slice(1);

/** Edge tint by relation — soft, so links read as connective tissue, not noise. */
const EDGE_COLOR: Record<string, string> = {
  mention: "rgba(190, 90, 56, 0.42)",
  child: "rgba(120, 113, 108, 0.34)",
  semantic: "rgba(194, 149, 47, 0.34)",
};
const EDGE_FALLBACK = "rgba(120, 113, 108, 0.3)";
const EDGE_DIM = "rgba(120, 113, 108, 0.08)";

const LEGEND_EDGES: { relation: string; label: string; dashed?: boolean }[] = [
  { relation: "mention", label: "Mention" },
  { relation: "child", label: "Nested" },
  { relation: "semantic", label: "Similar" },
];

const endpointId = (v: string | FGNode): string => (typeof v === "string" ? v : v.id);
const truncate = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);
const nodeRadius = (degree: number) => 2.4 + Math.min(6.5, Math.sqrt(degree) * 1.7);

function isDarkTheme(): boolean {
  return document.documentElement.getAttribute("data-theme") === "dark";
}

/** Galaxy (F10): a 2D force-directed view of the connected brain. Notes and
 * dictations are stars; `@`-mentions, nesting, and semantic similarity are the
 * links. Selecting a star focuses its neighbourhood and opens it. */
export function GalaxyPanel() {
  const [graph, setGraph] = useState<Graph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hiddenKinds, setHiddenKinds] = useState<Set<string>>(new Set());
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [dark, setDark] = useState(isDarkTheme);

  const fgRef = useRef<ForceGraphMethods<FGNode, FGLink> | undefined>(undefined);
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

  // Repaint labels in the right ink when the app theme flips.
  useEffect(() => {
    const observer = new MutationObserver(() => setDark(isDarkTheme()));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    return () => observer.disconnect();
  }, []);

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

  // Kinds present in the graph, for the filter/legend rows (with counts).
  const kinds = useMemo(() => {
    const counts = new Map<string, number>();
    for (const n of graph?.nodes ?? []) counts.set(n.kind, (counts.get(n.kind) ?? 0) + 1);
    return [...counts.entries()]
      .map(([kind, count]) => ({ kind, count }))
      .sort((a, b) => b.count - a.count);
  }, [graph]);

  // The graph handed to the engine — filtered by kind, freshly cloned (the
  // engine mutates node/link objects). Kept stable across selection/search so
  // focusing a node never re-runs the layout; only kind filters rebuild it.
  const data = useMemo(() => {
    if (!graph) return { nodes: [] as GNode[], links: [] as GLink[] };
    const nodes = graph.nodes.filter((n) => !hiddenKinds.has(n.kind)).map((n) => ({ ...n }));
    const present = new Set(nodes.map((n) => n.id));
    const links = graph.edges
      .filter((e) => present.has(e.source) && present.has(e.target))
      .map((e) => ({ ...e }));
    return { nodes, links };
  }, [graph, hiddenKinds]);

  // Degree + adjacency, computed while source/target are still ids (pre-engine).
  const { degree, adjacency } = useMemo(() => {
    const deg = new Map<string, number>();
    const adj = new Map<string, Set<string>>();
    for (const link of data.links) {
      const s = endpointId(link.source);
      const t = endpointId(link.target);
      deg.set(s, (deg.get(s) ?? 0) + 1);
      deg.set(t, (deg.get(t) ?? 0) + 1);
      (adj.get(s) ?? adj.set(s, new Set()).get(s))?.add(t);
      (adj.get(t) ?? adj.set(t, new Set()).get(t))?.add(s);
    }
    return { degree: deg, adjacency: adj };
  }, [data]);

  // The lit set when a star is selected: itself + its direct neighbours.
  const focus = useMemo(() => {
    if (!selectedId) return null;
    const set = new Set<string>(adjacency.get(selectedId) ?? []);
    set.add(selectedId);
    return set;
  }, [selectedId, adjacency]);

  const selectedNode = useMemo(
    () => (selectedId ? (data.nodes.find((n) => n.id === selectedId) ?? null) : null),
    [selectedId, data],
  );

  const ink = dark ? "#f1ece2" : "#2a2622";
  const halo = dark ? "rgba(38, 35, 31, 0.82)" : "rgba(251, 249, 245, 0.82)";

  const paintNode = useCallback(
    (node: FGNode, ctx: CanvasRenderingContext2D, scale: number) => {
      const x = node.x ?? 0;
      const y = node.y ?? 0;
      const deg = degree.get(node.id) ?? 0;
      const r = nodeRadius(deg);
      const q = query.trim().toLowerCase();
      const matched = q ? node.label.toLowerCase().includes(q) : true;
      const lit = (focus ? focus.has(node.id) : true) && matched;

      ctx.globalAlpha = lit ? 1 : 0.14;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, TAU);
      ctx.fillStyle = categoryColor(node.kind);
      ctx.fill();

      if (node.id === selectedId) {
        ctx.globalAlpha = 1;
        ctx.lineWidth = 1.6 / scale;
        ctx.strokeStyle = ink;
        ctx.beginPath();
        ctx.arc(x, y, r + 3.5 / scale, 0, TAU);
        ctx.stroke();
      }

      const isHub = deg >= 4;
      const showLabel = lit && (scale > 1.3 || isHub || node.id === selectedId);
      if (showLabel) {
        const fontSize = Math.max(3.5, 11 / scale);
        ctx.font = `${fontSize}px ${CANVAS_SANS}`;
        const label = truncate(node.label || "Untitled", 26);
        const w = ctx.measureText(label).width;
        const pad = 2 / scale;
        const ly = y + r + 2 / scale;
        ctx.globalAlpha = 0.9;
        ctx.fillStyle = halo;
        ctx.fillRect(x - w / 2 - pad, ly, w + pad * 2, fontSize + pad * 2);
        ctx.globalAlpha = 1;
        ctx.fillStyle = ink;
        ctx.textAlign = "center";
        ctx.textBaseline = "top";
        ctx.fillText(label, x, ly + pad);
      }
      ctx.globalAlpha = 1;
    },
    [degree, focus, query, selectedId, ink, halo],
  );

  const paintPointerArea = useCallback(
    (node: FGNode, color: string, ctx: CanvasRenderingContext2D) => {
      const r = nodeRadius(degree.get(node.id) ?? 0) + 4;
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(node.x ?? 0, node.y ?? 0, r, 0, TAU);
      ctx.fill();
    },
    [degree],
  );

  const linkColor = useCallback(
    (link: FGLink) => {
      const base = EDGE_COLOR[link.relation ?? ""] ?? EDGE_FALLBACK;
      if (!focus) return base;
      return focus.has(endpointId(link.source)) && focus.has(endpointId(link.target))
        ? base
        : EDGE_DIM;
    },
    [focus],
  );

  const onNodeClick = useCallback((node: FGNode) => {
    setSelectedId(node.id);
    const fg = fgRef.current;
    if (fg && node.x != null && node.y != null) {
      fg.centerAt(node.x, node.y, 600);
      fg.zoom(2.4, 600);
    }
  }, []);

  const resetView = useCallback(() => {
    setSelectedId(null);
    fgRef.current?.zoomToFit(600, 60);
  }, []);

  const toggleKind = (kind: string) =>
    setHiddenKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });

  const openSelected = useCallback(() => {
    if (!selectedNode) return;
    const [kind, ...rest] = selectedNode.id.split(":");
    window.dispatchEvent(
      new CustomEvent("arya:open-node", { detail: { kind, id: rest.join(":") } }),
    );
  }, [selectedNode]);

  const showGraph = graph && data.nodes.length > 0;

  return (
    <div className="screen">
      <aside className="panel galaxy-rail">
        <div className="panel-head">
          <div className="panel-title">Galaxy</div>
          <div className="muted" style={{ fontSize: 12.5, marginTop: 2 }}>
            {graph ? `${graph.nodes.length} stars · ${graph.edges.length} connections` : "mapping…"}
          </div>
        </div>
        <div className="panel-body">
          <div className="galaxy-section">
            <input
              aria-label="Search the galaxy"
              placeholder="Search the galaxy…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>

          {kinds.length > 0 ? (
            <div className="galaxy-section">
              <div className="section-label">Filter</div>
              {kinds.map(({ kind, count }) => (
                <button
                  type="button"
                  key={kind}
                  className="galaxy-row"
                  aria-pressed={!hiddenKinds.has(kind)}
                  onClick={() => toggleKind(kind)}
                >
                  <span className="galaxy-dot" style={{ background: categoryColor(kind) }} />
                  <span className="galaxy-row-label">{categoryLabel(kind)}</span>
                  <span className="galaxy-row-count">{count}</span>
                </button>
              ))}
            </div>
          ) : null}

          <div className="galaxy-section">
            <div className="section-label">Connections</div>
            {LEGEND_EDGES.map((e) => (
              <div className="galaxy-legend-row" key={e.relation}>
                <span
                  className={`galaxy-edge-swatch${e.dashed ? " dashed" : ""}`}
                  style={{ borderTopColor: EDGE_COLOR[e.relation] }}
                />
                <span>{e.label}</span>
              </div>
            ))}
          </div>

          <div className="galaxy-section">
            <button type="button" className="btn-sm btn-ghost" onClick={resetView}>
              Reset view
            </button>
          </div>
        </div>
      </aside>

      <div ref={wrapRef} className="panel panel-grow galaxy-stage">
        {showGraph ? (
          <ForceGraph2D<GNode, GLink>
            ref={fgRef}
            graphData={data}
            width={size.w}
            height={size.h}
            backgroundColor="rgba(0,0,0,0)"
            nodeCanvasObject={paintNode}
            nodePointerAreaPaint={paintPointerArea}
            nodeLabel={(n) => n.label}
            linkColor={linkColor}
            linkWidth={1}
            onNodeClick={onNodeClick}
            onBackgroundClick={() => setSelectedId(null)}
            cooldownTicks={110}
          />
        ) : null}

        {error ? (
          <div className="galaxy-overlay">
            <p role="alert" className="empty">
              {error}
            </p>
          </div>
        ) : !graph ? (
          <div className="galaxy-overlay">
            <div className="empty hstack" style={{ gap: 10 }}>
              <span className="spinner" />
              Mapping your galaxy…
            </div>
          </div>
        ) : data.nodes.length === 0 ? (
          <div className="galaxy-overlay">
            <div className="empty">
              <div style={{ fontFamily: "var(--font-serif)", fontSize: 18, marginBottom: 6 }}>
                No stars yet
              </div>
              Create notes and dictations, then @-mention across them — the connections light up
              here.
            </div>
          </div>
        ) : null}

        {selectedNode ? (
          <aside className="galaxy-inspector">
            <div className="galaxy-inspector-head">
              <span
                className="galaxy-dot"
                style={{ background: categoryColor(selectedNode.kind) }}
              />
              <div style={{ flex: 1, minWidth: 0 }}>
                <div className="galaxy-inspector-title">{selectedNode.label || "Untitled"}</div>
                <div className="galaxy-inspector-kind">{categoryLabel(selectedNode.kind)}</div>
              </div>
            </div>
            <div className="galaxy-inspector-stat">
              {degree.get(selectedNode.id) ?? 0} connection
              {(degree.get(selectedNode.id) ?? 0) === 1 ? "" : "s"}
            </div>
            <button type="button" className="btn-sm btn-primary" onClick={openSelected}>
              Open
            </button>
          </aside>
        ) : showGraph ? (
          <div className="galaxy-hint">Click a star to focus and open it</div>
        ) : null}
      </div>
    </div>
  );
}
