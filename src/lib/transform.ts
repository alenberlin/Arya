import { invoke } from "@tauri-apps/api/core";

/** Where an AI transform runs. Local (Ollama) is private and offline. */
export type TransformProvider = "local" | "cloud";

/**
 * Apply a free-form instruction to text via a local (Ollama, default) or cloud
 * LLM. The result reorganizes, rephrases, summarizes, or translates only the
 * given text — it never invents. Powers the inline `@node + instruction` action
 * (F15) and the "Sort" brain-dump reorganizer (F16).
 */
export const aiTransform = (
  sourceText: string,
  instruction: string,
  provider: TransformProvider = "local",
  model?: string,
) =>
  invoke<string>("ai_transform", {
    sourceText,
    instruction,
    provider,
    model: model ?? null,
  });
