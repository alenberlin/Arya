import "@xyflow/react/dist/style.css";
import {
  addEdge,
  Background,
  type Connection,
  Controls,
  type Edge,
  type Node,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
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

function newId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `n-${Date.now()}-${Math.floor(performance.now())}`;
}

/** The React Flow canvas for one map: nodes/edges/viewport persisted as an
 * opaque JSON document with debounced autosave. Keyed by map id so it
 * re-initializes cleanly when the open map changes. */
function MindMapCanvas({ mapId }: { mapId: string }) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
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
          setNodes(Array.isArray(doc.nodes) ? doc.nodes : []);
          setEdges(Array.isArray(doc.edges) ? doc.edges : []);
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

  // Debounced autosave once the initial document has loaded (so the load itself
  // doesn't immediately re-save an empty canvas over real content).
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

  const onConnect = useCallback((c: Connection) => setEdges((eds) => addEdge(c, eds)), [setEdges]);

  const addNode = () =>
    setNodes((nds) => [
      ...nds,
      {
        id: newId(),
        position: { x: 80 + (nds.length % 6) * 60, y: 80 + Math.floor(nds.length / 6) * 70 },
        data: { label: "New node" },
      },
    ]);

  const onNodeDoubleClick = (_e: React.MouseEvent, node: Node) => {
    const label = window.prompt("Node label", String(node.data?.label ?? ""));
    if (label != null) {
      setNodes((nds) =>
        nds.map((n) => (n.id === node.id ? { ...n, data: { ...n.data, label } } : n)),
      );
    }
  };

  return (
    <div style={{ position: "absolute", inset: 0 }}>
      <button
        type="button"
        className="btn-sm btn-primary"
        style={{ position: "absolute", zIndex: 5, top: 12, left: 12 }}
        onClick={addNode}
      >
        Add node
      </button>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        onNodeDoubleClick={onNodeDoubleClick}
        fitView
      >
        <Background />
        <Controls />
      </ReactFlow>
    </div>
  );
}

/** Mind Map (F11/M12): a list of maps beside a React Flow canvas. Double-click a
 * node to rename; drag to connect; changes autosave. */
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
