import "@xyflow/react/dist/style.css";
import {
  Background,
  BackgroundVariant,
  Controls,
  type Edge,
  Handle,
  type Node,
  type NodeProps,
  NodeToolbar,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
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
import { PlusIcon, TrashIcon } from "../ui/icons";
import "./mindmap.css";

type Shape = "rounded" | "square" | "diamond";
type MindData = { label?: string; depth?: number; shape?: Shape; color?: string };
type MindNode = Node<MindData, "mind">;

/** Warm depth palette — node hue by distance from a root (AlenAI colours by
 * hierarchy depth; these are muted warm hues that read on the cream ground). */
const WARM_DEPTH = ["#be5a38", "#c2952f", "#5e7b57", "#8a6ea8", "#6e857e", "#a8582f", "#9a7b4f"];
const nodeColor = (d: MindData) => d.color ?? WARM_DEPTH[(d.depth ?? 0) % WARM_DEPTH.length];

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

/** Assign each node a depth (distance from a root, BFS) so colours reflect the
 * hierarchy even for maps saved before depth was tracked. */
function withDepths(nodes: MindNode[], edges: Edge[]): MindNode[] {
  const childrenOf = new Map<string, string[]>();
  const hasParent = new Set<string>();
  for (const e of edges) {
    hasParent.add(e.target);
    const list = childrenOf.get(e.source) ?? [];
    list.push(e.target);
    childrenOf.set(e.source, list);
  }
  const depth = new Map<string, number>();
  const queue: [string, number][] = nodes.filter((n) => !hasParent.has(n.id)).map((n) => [n.id, 0]);
  const seen = new Set<string>();
  while (queue.length) {
    const [id, d] = queue.shift() as [string, number];
    if (seen.has(id)) continue;
    seen.add(id);
    depth.set(id, d);
    for (const c of childrenOf.get(id) ?? []) queue.push([c, d + 1]);
  }
  return nodes.map((n) => ({
    ...n,
    type: "mind" as const,
    data: { ...n.data, depth: depth.get(n.id) ?? 0 },
  }));
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

const nodeTypes = { mind: MindNodeView };
type Menu = { x: number; y: number; nodeId: string | null; flowX: number; flowY: number };

/** The canvas for one map: nodes/edges persisted as an opaque JSON document
 * with debounced autosave, keyed by map id so it re-initializes on switch. */
function MindMapCanvas({ mapId }: { mapId: string }) {
  const [nodes, setNodes, onNodesChange] = useNodesState<MindNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [menu, setMenu] = useState<Menu | null>(null);
  const rf = useReactFlow<MindNode, Edge>();
  const loaded = useRef(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let active = true;
    loaded.current = false;
    void getMindmap(mapId)
      .then((map) => {
        if (!active) return;
        try {
          const doc = map.docJson ? JSON.parse(map.docJson) : {};
          const rawNodes: MindNode[] = Array.isArray(doc.nodes) ? doc.nodes : [];
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

  const setShape = (nodeId: string, shape: Shape) => {
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, shape } } : n)));
    setMenu(null);
  };

  const setColor = (nodeId: string, color: string) => {
    setNodes((nds) => nds.map((n) => (n.id === nodeId ? { ...n, data: { ...n.data, color } } : n)));
    setMenu(null);
  };

  const deleteBranch = (nodeId: string) => {
    // Remove the node and everything downstream of it (its branch).
    const doomed = new Set<string>([nodeId]);
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
    <div className="mind-canvas">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        nodesConnectable={false}
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

      {menu ? (
        <div className="mind-ctx" style={{ left: menu.x, top: menu.y }}>
          {/* A window-level click/Escape listener closes this; item clicks run
              their action first, then bubble to close. */}
          {menu.nodeId ? (
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
                onClick={() => menu.nodeId && deleteBranch(menu.nodeId)}
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
