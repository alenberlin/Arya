import { invoke } from "@tauri-apps/api/core";

/**
 * Knowledge Base IPC wrappers. The KB is an on-device RAG surface: uploaded
 * documents organized into collections, ingested locally, and queried with
 * grounded chat. Types mirror the Rust `serde(rename_all = "camelCase")` shapes.
 */

/** A named knowledge base with derived ingestion counts. */
export interface KbCollection {
  id: string;
  name: string;
  description: string;
  createdAt: string;
  updatedAt: string;
  documentCount: number;
  readyCount: number;
}

export interface KbStatus {
  /** Whether the local embedding model (Ollama) is reachable. */
  embedderAvailable: boolean;
}

export const kbStatus = () => invoke<KbStatus>("kb_status");

export const kbListCollections = () => invoke<KbCollection[]>("kb_list_collections");

export const kbCreateCollection = (name: string, description?: string) =>
  invoke<KbCollection>("kb_create_collection", { name, description: description ?? null });

export const kbRenameCollection = (id: string, name: string, description?: string) =>
  invoke<KbCollection>("kb_rename_collection", { id, name, description: description ?? null });

export const kbDeleteCollection = (id: string) => invoke<void>("kb_delete_collection", { id });

export type KbDocStatus = "pending" | "processing" | "ready" | "failed";

/** A document in a collection, with its ingestion status and extraction metadata. */
export interface KbDocument {
  id: string;
  collectionId: string;
  filename: string;
  ext: string;
  byteSize: number;
  status: KbDocStatus;
  /** How the text was recovered: "text" | "ocr" | "" (still pending). */
  extractor: string;
  pageCount: number;
  chunkCount: number;
  error: string | null;
  createdAt: string;
  updatedAt: string;
}

/** Payload of the `kb:progress` event emitted as documents ingest. */
export interface KbProgress {
  documentId: string;
  collectionId: string;
  status: KbDocStatus;
  error: string | null;
}

/** Extensions the ingestion pipeline can read (used for the file-picker filter). */
export const KB_ACCEPT_EXTENSIONS = [
  "pdf",
  "docx",
  "xlsx",
  "xls",
  "csv",
  "tsv",
  "txt",
  "md",
  "markdown",
  "html",
  "htm",
  "json",
  "png",
  "jpg",
  "jpeg",
  "tiff",
  "tif",
  "bmp",
  "webp",
  "gif",
];

export const kbListDocuments = (collectionId: string) =>
  invoke<KbDocument[]>("kb_list_documents", { collectionId });

export const kbAddDocuments = (collectionId: string, paths: string[]) =>
  invoke<KbDocument[]>("kb_add_documents", { collectionId, paths });

export const kbDeleteDocument = (id: string) => invoke<void>("kb_delete_document", { id });

export const kbReindexDocument = (id: string) => invoke<void>("kb_reindex_document", { id });

/** A source backing an assistant answer, keyed by its inline `[D#]` tag. */
export interface KbCitation {
  key: string;
  documentId: string;
  filename: string;
  page: number | null;
  quote: string;
}

/** A chat session scoped to one collection. */
export interface KbSession {
  id: string;
  collectionId: string;
  title: string;
  createdAt: string;
  updatedAt: string;
}

/** A persisted chat message with its parsed citations. */
export interface KbMessage {
  id: string;
  sessionId: string;
  role: "user" | "assistant";
  content: string;
  citations: KbCitation[];
  createdAt: string;
}

/** The pair of messages produced by one grounded question. */
export interface KbAnswer {
  userMessage: KbMessage;
  assistantMessage: KbMessage;
}

export const kbListSessions = (collectionId: string) =>
  invoke<KbSession[]>("kb_list_sessions", { collectionId });

export const kbCreateSession = (collectionId: string) =>
  invoke<KbSession>("kb_create_session", { collectionId });

export const kbDeleteSession = (id: string) => invoke<void>("kb_delete_session", { id });

export const kbGetMessages = (sessionId: string) =>
  invoke<KbMessage[]>("kb_get_messages", { sessionId });

export const kbAsk = (sessionId: string, query: string, model?: string) =>
  invoke<KbAnswer>("kb_ask", { sessionId, query, model: model ?? null });
