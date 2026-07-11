//! Client wrappers for model setup: cloud API keys, Ollama status, and speech
//! (Whisper) model downloads. Keys never round-trip to the UI — only "set /
//! not set" — so a stored secret can't leak through the frontend.

import { invoke } from "@tauri-apps/api/core";
import { type Event, listen } from "@tauri-apps/api/event";

// ---- Cloud API keys -------------------------------------------------------

export type KeyProvider = "openai" | "anthropic";

/** Which providers currently have a key stored (booleans only). */
export type KeysStatus = {
  openai: boolean;
  anthropic: boolean;
};

export function keysStatus(): Promise<KeysStatus> {
  return invoke<KeysStatus>("keys_status");
}

/** Store or replace a key. A blank value removes it. Returns fresh status. */
export function keysSet(provider: KeyProvider, key: string): Promise<KeysStatus> {
  return invoke<KeysStatus>("keys_set", { provider, key });
}

/** Remove a provider key. Returns fresh status. */
export function keysClear(provider: KeyProvider): Promise<KeysStatus> {
  return invoke<KeysStatus>("keys_clear", { provider });
}

// ---- Ollama (local models) ------------------------------------------------

export type OllamaStatus = {
  reachable: boolean;
  modelCount: number;
  url: string;
};

export function ollamaStatus(): Promise<OllamaStatus> {
  return invoke<OllamaStatus>("ollama_status");
}

// ---- Speech (Whisper) models ----------------------------------------------

export type SpeechModelStatus = {
  id: string;
  fileName: string;
  approxBytes: number;
  downloaded: boolean;
};

export function speechModelsStatus(): Promise<SpeechModelStatus[]> {
  return invoke<SpeechModelStatus[]>("speech_models_status");
}

export function downloadSpeechModel(id: string): Promise<void> {
  return invoke("download_speech_model", { id });
}

export function deleteSpeechModel(id: string): Promise<void> {
  return invoke("delete_speech_model", { id });
}

export type SpeechDownloadProgress = {
  id: string;
  received: number;
  total: number;
  done: boolean;
};

/** Subscribe to `speech:download-progress`. Returns an unlisten function. */
export function onSpeechDownloadProgress(
  handler: (p: SpeechDownloadProgress) => void,
): Promise<() => void> {
  return listen("speech:download-progress", (e: Event<SpeechDownloadProgress>) =>
    handler(e.payload),
  );
}

/** Human-friendly byte size, e.g. `574 MB`. */
export function formatBytes(bytes: number): string {
  if (bytes >= 1_000_000_000) return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  if (bytes >= 1_000_000) return `${Math.round(bytes / 1_000_000)} MB`;
  if (bytes >= 1_000) return `${Math.round(bytes / 1_000)} KB`;
  return `${bytes} B`;
}
