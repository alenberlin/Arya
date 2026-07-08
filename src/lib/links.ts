import { invoke } from "@tauri-apps/api/core";

/**
 * The connected-brain graph (F1). Every first-class item — a note, dictation,
 * meeting, or mind map — is a *node* named by `(kind, id)`, and a `Link` is a
 * directed edge between two of them. Edges are polymorphic and not enforced by
 * foreign keys, so a target may be of any kind and a dangling target is allowed
 * (resolved, or shown as deleted, at read time).
 */
export type NodeKind = "note" | "dictation" | "meeting" | "mindmap";

/** A directed edge between two nodes. */
export interface Link {
  id: string;
  sourceKind: NodeKind;
  sourceId: string;
  targetKind: NodeKind;
  targetId: string;
  /** `mention` | `semantic` | `manual` | … */
  relation: string;
  /** `user` | `agent` | `system` */
  origin: string;
  weight: number;
  createdAt: string;
}

/**
 * Create a user-initiated edge, or idempotently return the existing one for the
 * same `(source, target, relation)`. `relation` defaults to `mention`; the edge
 * is always stored with `origin: "user"` and the default weight (agent and
 * semantic edges are created server-side, not from the client).
 */
export const createLink = (input: {
  sourceKind: NodeKind;
  sourceId: string;
  targetKind: NodeKind;
  targetId: string;
  relation?: string;
}) =>
  invoke<Link>("create_link", {
    sourceKind: input.sourceKind,
    sourceId: input.sourceId,
    targetKind: input.targetKind,
    targetId: input.targetId,
    relation: input.relation ?? null,
  });

/** Outbound edges from a node (what it links to). */
export const listLinksFrom = (kind: NodeKind, id: string) =>
  invoke<Link[]>("list_links_from", { kind, id });

/** Inbound edges to a node — its backlinks (what links to it). */
export const listLinksTo = (kind: NodeKind, id: string) =>
  invoke<Link[]>("list_links_to", { kind, id });

/** Delete one edge by id. */
export const deleteLink = (id: string) => invoke<void>("delete_link", { id });
