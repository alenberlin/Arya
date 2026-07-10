import { invoke } from "@tauri-apps/api/core";

/** A node in the connected-brain graph (F10). Composite id `"<kind>:<uuid>"`. */
export interface GraphNode {
  id: string;
  kind: string;
  label: string;
}

export interface GraphEdge {
  source: string;
  target: string;
  /** mention | child | semantic | … */
  relation: string;
}

export interface Graph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

/** Assemble the Galaxy graph for the whole workspace. */
export const galaxyGraph = () => invoke<Graph>("galaxy_graph");
