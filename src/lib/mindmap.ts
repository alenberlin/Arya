import { invoke } from "@tauri-apps/api/core";

export interface MindmapSummary {
  id: string;
  title: string;
  updatedAt: string;
}

export interface Mindmap {
  id: string;
  title: string;
  /** Opaque React Flow document: `{ nodes, edges }` (empty for a new map). */
  docJson: string;
  createdAt: string;
  updatedAt: string;
}

export const createMindmap = (title = "") => invoke<Mindmap>("create_mindmap", { title });
export const listMindmaps = () => invoke<MindmapSummary[]>("list_mindmaps");
export const getMindmap = (id: string) => invoke<Mindmap>("get_mindmap", { id });
export const updateMindmap = (id: string, fields: { title?: string; docJson?: string }) =>
  invoke<void>("update_mindmap", {
    id,
    title: fields.title ?? null,
    docJson: fields.docJson ?? null,
  });
export const deleteMindmap = (id: string) => invoke<void>("delete_mindmap", { id });
