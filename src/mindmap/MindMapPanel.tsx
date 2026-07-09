import "@xyflow/react/dist/style.css";
import {
  Background,
  BackgroundVariant,
  BaseEdge,
  Controls,
  type Edge,
  type EdgeProps,
  getBezierPath,
  Handle,
  type Node,
  type NodeProps,
  NodeResizer,
  NodeToolbar,
  Position,
  ReactFlow,
  ReactFlowProvider,
  SelectionMode,
  useEdgesState,
  useInternalNode,
  useNodesState,
  useReactFlow,
} from "@xyflow/react";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  createMindmap,
  deleteMindmap,
  getMindmap,
  listMindmaps,
  type MindmapSummary,
  updateMindmap,
} from "../lib/mindmap";
import { PlusIcon, StickyNoteIcon, TrashIcon } from "../ui/icons";
import "./mindmap.css";

type Shape = "rounded" | "square" | "diamond";
type MindData = { label?: string; depth?: number; shape?: Shape; color?: string };
type MindNode = Node<MindData, "mind">;

/** A freestanding annotation — not part of the tree (no edges), same as
 * AlenAI's sticky notes: text + colour only, no rotation, no recolour UI. */
type StickyData = { text?: string; color?: string };
type StickyNode = Node<StickyData, "sticky">;

type MMNode = MindNode | StickyNode;

/** Warm depth palette — node hue by distance from a root (AlenAI colours by
 * hierarchy depth; these are muted warm hues that read on the cream ground). */
const WARM_DEPTH = ["#be5a38", "#c2952f", "#5e7b57", "#8a6ea8", "#6e857e", "#a8582f", "#9a7b4f"];
const nodeColor = (d: MindData) => d.color ?? WARM_DEPTH[(d.depth ?? 0) % WARM_DEPTH.length];

/** Pastel sticky-note palette in Arya's warm language (AlenAI picks one of a
 * fixed set at random on creation; ours are muted to read on the cream ground). */
const STICKY_COLORS = ["#f3dfa3", "#f0c8ac", "#d9c7e6", "#c3d9bc", "#f0c2c3", "#c1d8e0", "#e6d2ab"];
const randomStickyColor = () => STICKY_COLORS[Math.floor(Math.random() * STICKY_COLORS.length)];

/** The four sides — each carries a visible source dot and an invisible target
 * anchor so programmatic parent→child edges attach to the facing side. */
const SIDES = [
  { pos: Position.Top, src: "top", tgt: "ttop" },
  { pos: Position.Right, src: "right", tgt: "tright" },
  { pos: Position.Bottom, src: "bottom", tgt: "tbottom" },
  { pos: Position.Left, src: "left", tgt: "tleft" },
] as const;

/** Add-child directions: the "+" button side, the parent source handle, and the
 * child target handle (the child's side that faces the parent). */
const ADD_DIRS = [
  { dir: "up", pos: Position.Top, sh: "top", th: "tbottom" },
  { dir: "right", pos: Position.Right, sh: "right", th: "tleft" },
  { dir: "down", pos: Position.Bottom, sh: "bottom", th: "ttop" },
  { dir: "left", pos: Position.Left, sh: "left", th: "tright" },
] as const;

const SHAPES: { shape: Shape; label: string }[] = [
  { shape: "rounded", label: "Rounded" },
  { shape: "square", label: "Square" },
  { shape: "diamond", label: "Diamond" },
];

function newId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `n-${Date.now()}-${Math.floor(performance.now())}`;
}

/** Child offset from its parent, per direction — mirrors AlenAI's fan-out so
 * siblings don't stack (stagger by how many children already exist). */
function offsetFor(dir: (typeof ADD_DIRS)[number]["dir"], w: number, h: number, count: number) {
  const stagger = count > 0;
  switch (dir) {
    case "right":
      return { dx: w + 80, dy: count * 60 - (stagger ? 30 : 0) };
    case "left":
      return { dx: -(w + 80), dy: count * 60 - (stagger ? 30 : 0) };
    case "up":
      return { dx: count * (w + 20) - (stagger ? w / 2 : 0), dy: -(h + 60) };
    default:
      return { dx: count * (w + 20) - (stagger ? w / 2 : 0), dy: h + 60 };
  }
}

/** Assign each mind-node a depth (distance from a root, BFS) so colours reflect
 * the hierarchy even for maps saved before depth was tracked. Sticky notes
 * aren't part of the tree (no edges reference them) and pass through as-is. */
export function withDepths(nodes: MMNode[], edges: Edge[]): MMNode[] {
  const childrenOf = new Map<string, string[]>();
  const hasParent = new Set<string>();
  for (const e of edges) {
    hasParent.add(e.target);
    const list = childrenOf.get(e.source) ?? [];
    list.push(e.target);
    childrenOf.set(e.source, list);
  }
  const depth = new Map<string, number>();
  const mindNodes = nodes.filter((n) => n.type !== "sticky");
  const queue: [string, number][] = mindNodes
    .filter((n) => !hasParent.has(n.id))
    .map((n) => [n.id, 0]);
  const seen = new Set<string>();
  while (queue.length) {
    const [id, d] = queue.shift() as [string, number];
    if (seen.has(id)) continue;
    seen.add(id);
    depth.set(id, d);
    for (const c of childrenOf.get(id) ?? []) queue.push([c, d + 1]);
  }
  return nodes.map((n) =>
    n.type === "sticky"
      ? n
      : { ...n, type: "mind" as const, data: { ...n.data, depth: depth.get(n.id) ?? 0 } },
  );
}

/** A node card. Select it for four directional "+" buttons; double-click to
 * rename; right-click (handled by the canvas) for shape/colour/delete. */
function MindNodeView({ id, data, selected }: NodeProps<MindNode>) {
  const rf = useReactFlow<MindNode, Edge>();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(data.label ?? "");

  useEffect(() => {
    if (!editing) setText(data.label ?? "");
  }, [data.label, editing]);

  const shape = data.shape ?? "rounded";
  const square = shape !== "rounded";

  const commit = () => {
    setEditing(false);
    rf.setNodes((nds) =>
      nds.map((n) => (n.id === id ? { ...n, data: { ...n.data, label: text } } : n)),
    );
  };

  const addChild = (spec: (typeof ADD_DIRS)[number]) => {
    const parent = rf.getNode(id);
    if (!parent) return;
    const w = parent.measured?.width ?? 140;
    const h = parent.measured?.height ?? 44;
    const count = rf.getEdges().filter((e) => e.source === id).length;
    const { dx, dy } = offsetFor(spec.dir, w, h, count);
    const childId = newId();
    rf.setNodes((nds) => [
      ...nds,
      {
        id: childId,
        type: "mind",
        position: { x: parent.position.x + dx, y: parent.position.y + dy },
        data: { label: "New node", depth: (parent.data.depth ?? 0) + 1, shape: "rounded" },
      },
    ]);
    rf.setEdges((eds) => [
      ...eds,
      {
        id: `e-${id}-${childId}`,
        source: id,
        target: childId,
        sourceHandle: spec.sh,
        targetHandle: spec.th,
      },
    ]);
  };

  // The card is a control only when not editing (the input takes over then).
  const interactive = editing
    ? {}
    : {
        role: "button" as const,
        tabIndex: 0,
        onDoubleClick: () => setEditing(true),
        onKeyDown: (e: React.KeyboardEvent) => {
          if (e.key === "Enter" || e.key === "F2") {
            e.preventDefault();
            setEditing(true);
          }
        },
      };

  return (
    <>
      {ADD_DIRS.map((spec) => (
        <NodeToolbar key={spec.dir} isVisible={selected && !editing} position={spec.pos} offset={6}>
          <button
            type="button"
            className="mind-add"
            aria-label={`Add node ${spec.dir}`}
            onClick={() => addChild(spec)}
          >
            +
          </button>
        </NodeToolbar>
      ))}
      <div
        className={`mind-node shape-${shape}${selected ? " selected" : ""}`}
        style={{ background: nodeColor(data), ...(square ? { width: 104, height: 104 } : {}) }}
        {...interactive}
      >
        {SIDES.map((s) => (
          <Handle
            key={s.src}
            id={s.src}
            type="source"
            position={s.pos}
            className="mind-handle"
            isConnectable={false}
          />
        ))}
        {SIDES.map((s) => (
          <Handle
            key={s.tgt}
            id={s.tgt}
            type="target"
            position={s.pos}
            className="mind-handle-target"
            isConnectable={false}
          />
        ))}
        {editing ? (
          <input
            // biome-ignore lint/a11y/noAutofocus: rename UX expects the field focused on open
            autoFocus
            className="mind-node-input"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commit();
              } else if (e.key === "Escape") {
                setEditing(false);
                setText(data.label ?? "");
              }
            }}
          />
        ) : (
          <span className="mind-node-label">{data.label || "Untitled"}</span>
        )}
      </div>
    </>
  );
}

/** A freestanding sticky note: double-click to edit, drag to move, resize from
 * the corners when selected. Not connected to the tree — no handles, no edges. */
function StickyNoteView({ id, data, selected }: NodeProps<StickyNode>) {
  const rf = useReactFlow<MMNode, Edge>();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(data.text ?? "");

  useEffect(() => {
    if (!editing) setText(data.text ?? "");
  }, [data.text, editing]);

  const commit = () => {
    setEditing(false);
    rf.setNodes((nds) => nds.map((n) => (n.id === id ? { ...n, data: { ...n.data, text } } : n)));
  };

  // The card is a control only when not editing (the textarea takes over then).
  const interactive = editing
    ? {}
    : {
        role: "button" as const,
        tabIndex: 0,
        onDoubleClick: () => setEditing(true),
        onKeyDown: (e: React.KeyboardEvent) => {
          if (e.key === "Enter" || e.key === "F2") {
            e.preventDefault();
            setEditing(true);
          }
        },
      };

  return (
    <>
      <NodeResizer isVisible={selected} minWidth={100} minHeight={60} />
      <div
        className="sticky-note"
        style={{ background: data.color ?? STICKY_COLORS[0] }}
        {...interactive}
      >
        {editing ? (
          <textarea
            // biome-ignore lint/a11y/noAutofocus: edit UX expects the field focused on open
            autoFocus
            className="sticky-note-input"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setEditing(false);
                setText(data.text ?? "");
              }
            }}
          />
        ) : (
          <p className="sticky-note-text">{data.text || "Type a note…"}</p>
        )}
      </div>
    </>
  );
}

const nodeTypes = { mind: MindNodeView, sticky: StickyNoteView };

type SideName = "top" | "right" | "bottom" | "left";
const OPPOSITE_SIDE: Record<SideName, SideName> = {
  top: "bottom",
  right: "left",
  bottom: "top",
  left: "right",
};
const RF_POSITION: Record<SideName, Position> = {
  top: Position.Top,
  right: Position.Right,
  bottom: Position.Bottom,
  left: Position.Left,
};

/** Which side of a node faces a point at (dx, dy) away from its centre, by
 * dominant axis — mirrors AlenAI's floating-edge routing exactly. */
export function facingSide(dx: number, dy: number): SideName {
  if (Math.abs(dx) > Math.abs(dy)) return dx > 0 ? "right" : "left";
  return dy > 0 ? "bottom" : "top";
}

function sidePoint(
  node: {
    internals: { positionAbsolute: { x: number; y: number } };
    measured: { width?: number; height?: number };
  },
  side: SideName,
) {
  const { x, y } = node.internals.positionAbsolute;
  const w = node.measured.width ?? 0;
  const h = node.measured.height ?? 0;
  switch (side) {
    case "top":
      return { x: x + w / 2, y };
    case "bottom":
      return { x: x + w / 2, y: y + h };
    case "left":
      return { x, y: y + h / 2 };
    default:
      return { x: x + w, y: y + h / 2 };
  }
}

/** A connector that re-picks its attachment side every render from the two
 * nodes' live positions, instead of staying pinned to whichever side was
 * closest when the edge was first created — so dragging a node never leaves
 * a line looping out of the wrong side. */
function FloatingMindEdge({ id, source, target, markerEnd, style }: EdgeProps) {
  const sourceNode = useInternalNode(source);
  const targetNode = useInternalNode(target);
  if (!sourceNode || !targetNode) return null;

  const sw = sourceNode.measured.width ?? 0;
  const sh = sourceNode.measured.height ?? 0;
  const tw = targetNode.measured.width ?? 0;
  const th = targetNode.measured.height ?? 0;
  const sCenter = {
    x: sourceNode.internals.positionAbsolute.x + sw / 2,
    y: sourceNode.internals.positionAbsolute.y + sh / 2,
  };
  const tCenter = {
    x: targetNode.internals.positionAbsolute.x + tw / 2,
    y: targetNode.internals.positionAbsolute.y + th / 2,
  };
  const side = facingSide(tCenter.x - sCenter.x, tCenter.y - sCenter.y);
  const from = sidePoint(sourceNode, side);
  const to = sidePoint(targetNode, OPPOSITE_SIDE[side]);

  const [path] = getBezierPath({
    sourceX: from.x,
    sourceY: from.y,
    sourcePosition: RF_POSITION[side],
    targetX: to.x,
    targetY: to.y,
    targetPosition: RF_POSITION[OPPOSITE_SIDE[side]],
  });

  return <BaseEdge id={id} path={path} markerEnd={markerEnd} style={style} />;
}

const edgeTypes = { mindEdge: FloatingMindEdge };

type Menu = { x: number; y: number; nodeId: string | null; flowX: number; flowY: number };

/** The canvas for one map: nodes/edges persisted as an opaque JSON document
 * with debounced autosave, keyed by map id so it re-initializes on switch. */
function MindMapCanvas({ mapId }: { mapId: string }) {
  const [nodes, setNodes, onNodesChange] = useNodesState<MMNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [menu, setMenu] = useState<Menu | null>(null);
  const rf = useReactFlow<MMNode, Edge>();
  const loaded = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    loaded.current = false;
    void getMindmap(mapId)
      .then((map) => {
        if (!active) return;
        try {
          const doc = map.docJson ? JSON.parse(map.docJson) : {};
          const rawNodes: MMNode[] = Array.isArray(doc.nodes) ? doc.nodes : [];
          const rawEdges: Edge[] = Array.isArray(doc.edges) ? doc.edges : [];
          setNodes(withDepths(rawNodes, rawEdges));
          setEdges(rawEdges);
        } catch {
          setNodes([]);
          setEdges([]);
        }
        loaded.current = true;
      })
      .catch(() => {
        loaded.current = true;
      });
    return () => {
      active = false;
    };
  }, [mapId, setNodes, setEdges]);

  // Debounced autosave once the initial document has loaded.
  useEffect(() => {
    if (!loaded.current) return;
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      void updateMindmap(mapId, { docJson: JSON.stringify({ nodes, edges }) }).catch(() => {});
    }, 600);
    return () => {
      if (saveTimer.current) clearTimeout(saveTimer.current);
    };
  }, [nodes, edges, mapId]);

  // Close the context menu on any outside interaction.
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const addRoot = (at: { x: number; y: number }) =>
    setNodes((nds) => [
      ...nds,
      {
        id: newId(),
        type: "mind",
        position: at,
        data: { label: "New node", depth: 0, shape: "rounded" },
      },
    ]);

  /** Spawn a sticky at the viewport centre, cascading like AlenAI's so
   * repeated clicks don't stack notes exactly on top of each other. */
  const addSticky = () => {
    const rect = canvasRef.current?.getBoundingClientRect();
    const center = rect
      ? rf.screenToFlowPosition({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 })
      : { x: 0, y: 0 };
    const cascade = nodes.filter((n) => n.type === "sticky").length * 20;
    setNodes((nds) => [
      ...nds,
      {
        id: newId(),
        type: "sticky",
        position: { x: center.x - 75 + cascade, y: center.y - 50 + cascade },
        width: 150,
        height: 100,
        data: { text: "", color: randomStickyColor() },
      },
    ]);
  };

  const setShape = (nodeId: string, shape: Shape) => {
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, shape } } : n)));
    setMenu(null);
  };

  const setColor = (nodeId: string, color: string) => {
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, color } } : n)));
    setMenu(null);
  };

  /** Remove a set of nodes and, for any mind-node among them, everything
   * downstream of it too (its branch). Stickies have no outgoing edges, so
   * they just fall out with a plain removal — one path handles both, and
   * this backs both the context-menu delete and multi-select Backspace. */
  const deleteBranches = (nodeIds: string[]) => {
    const doomed = new Set<string>(nodeIds);
    let grew = true;
    while (grew) {
      grew = false;
      for (const e of edges) {
        if (doomed.has(e.source) && !doomed.has(e.target)) {
          doomed.add(e.target);
          grew = true;
        }
      }
    }
    setNodes((nds) =>
      withDepths(
        nds.filter((n) => !doomed.has(n.id)),
        edges,
      ),
    );
    setEdges((eds) => eds.filter((e) => !doomed.has(e.source) && !doomed.has(e.target)));
    setMenu(null);
  };

  return (
    <div className="mind-canvas" ref={canvasRef}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        defaultEdgeOptions={{ type: "mindEdge" }}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodesDelete={(deleted) => deleteBranches(deleted.map((n) => n.id))}
        nodesConnectable={false}
        selectionOnDrag
        selectionMode={SelectionMode.Partial}
        panOnDrag={[1, 2]}
        panOnScroll
        zoomOnScroll={false}
        fitView
        onNodeContextMenu={(e, node) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, nodeId: node.id, flowX: 0, flowY: 0 });
        }}
        onPaneContextMenu={(e) => {
          e.preventDefault();
          const p = rf.screenToFlowPosition({ x: e.clientX, y: e.clientY });
          setMenu({ x: e.clientX, y: e.clientY, nodeId: null, flowX: p.x, flowY: p.y });
        }}
      >
        <Background variant={BackgroundVariant.Lines} gap={40} size={1} color="var(--border)" />
        <Controls showInteractive={false} />
      </ReactFlow>

      <button type="button" className="mind-sticky-btn" title="Add sticky note" onClick={addSticky}>
        <StickyNoteIcon />
        Add note
      </button>

      {menu ? (
        <div className="mind-ctx" style={{ left: menu.x, top: menu.y }}>
          {/* A window-level click/Escape listener closes this; item clicks run
              their action first, then bubble to close. */}
          {menu.nodeId && nodes.find((n) => n.id === menu.nodeId)?.type === "sticky" ? (
            <button
              type="button"
              className="mind-ctx-item danger"
              onClick={() => menu.nodeId && deleteBranches([menu.nodeId])}
            >
              Delete
            </button>
          ) : menu.nodeId ? (
            <>
              <div className="mind-ctx-label">Shape</div>
              {SHAPES.map((s) => (
                <button
                  type="button"
                  key={s.shape}
                  className="mind-ctx-item"
                  onClick={() => menu.nodeId && setShape(menu.nodeId, s.shape)}
                >
                  {s.label}
                </button>
              ))}
              <div className="mind-ctx-sep" />
              <div className="mind-ctx-label">Colour</div>
              <div className="mind-ctx-colors">
                {WARM_DEPTH.map((c) => (
                  <button
                    type="button"
                    key={c}
                    className="mind-ctx-color"
                    style={{ background: c }}
                    aria-label={`Colour ${c}`}
                    onClick={() => menu.nodeId && setColor(menu.nodeId, c)}
                  />
                ))}
              </div>
              <div className="mind-ctx-sep" />
              <button
                type="button"
                className="mind-ctx-item danger"
                onClick={() => menu.nodeId && deleteBranches([menu.nodeId])}
              >
                Delete branch
              </button>
            </>
          ) : (
            <button
              type="button"
              className="mind-ctx-item"
              onClick={() => {
                addRoot({ x: menu.flowX, y: menu.flowY });
                setMenu(null);
              }}
            >
              Add node here
            </button>
          )}
        </div>
      ) : null}
    </div>
  );
}

/** Mind Map (F11/M12): a list of maps beside the canvas. Select a node for
 * directional add-buttons; double-click to rename; right-click for shape/colour. */
export function MindMapPanel() {
  const [maps, setMaps] = useState<MindmapSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const next = await listMindmaps();
      setMaps(next);
      setActiveId((current) => current ?? next[0]?.id ?? null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const onCreate = async () => {
    try {
      const map = await createMindmap();
      await refresh();
      setActiveId(map.id);
    } catch (e) {
      setError(String(e));
    }
  };

  const onDelete = async (id: string) => {
    try {
      await deleteMindmap(id);
      setActiveId((current) => (current === id ? null : current));
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="screen">
      <div className="panel" style={{ width: 240, flex: "0 0 240px" }}>
        <div className="panel-head hstack spread">
          <span className="panel-title">Mind maps</span>
          <button
            type="button"
            className="btn-icon bare"
            aria-label="new mind map"
            title="New mind map"
            onClick={() => void onCreate()}
          >
            <PlusIcon />
          </button>
        </div>
        <div className="panel-body">
          <ul aria-label="mind maps" className="plain">
            {maps.map((map) => (
              <li key={map.id} className="note-row">
                <button
                  type="button"
                  className="row"
                  aria-current={map.id === activeId ? "true" : undefined}
                  onClick={() => setActiveId(map.id)}
                >
                  {map.title}
                </button>
                <button
                  type="button"
                  className="note-del"
                  aria-label={`delete ${map.title}`}
                  title="Delete mind map"
                  onClick={() => void onDelete(map.id)}
                >
                  <TrashIcon />
                </button>
              </li>
            ))}
            {maps.length === 0 ? (
              <li className="empty">No mind maps yet. Create one with +</li>
            ) : null}
          </ul>
        </div>
      </div>

      <div className="panel panel-grow" style={{ position: "relative", overflow: "hidden" }}>
        {error ? (
          <p role="alert" style={{ padding: "8px 12px" }}>
            {error}
          </p>
        ) : null}
        {activeId ? (
          <ReactFlowProvider>
            <MindMapCanvas key={activeId} mapId={activeId} />
          </ReactFlowProvider>
        ) : (
          <div className="empty" style={{ paddingTop: 48 }}>
            Select a mind map, or create one to start.
          </div>
        )}
      </div>
    </div>
  );
}
