import { createAnthropic } from "@ai-sdk/anthropic";
import { createOpenAI } from "@ai-sdk/openai";
import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import type { LanguageModel } from "ai";

const OLLAMA_URL = process.env.ARYA_OLLAMA_URL ?? "http://127.0.0.1:11434";

/**
 * Resolves "provider:model" to a language model. Providers:
 *   anthropic:* / openai:*  - direct APIs (keys via env until the Arya API
 *                             proxy lands in M11)
 *   ollama:*                - local, free, via Ollama's OpenAI endpoint
 */
export function resolveModel(qualified: string): LanguageModel {
  const [provider, ...rest] = qualified.split(":");
  const model = rest.join(":");
  if (!provider || !model) {
    throw new Error(`model must be provider-qualified, got "${qualified}"`);
  }
  switch (provider) {
    case "anthropic": {
      const anthropic = createAnthropic({
        apiKey: process.env.ANTHROPIC_API_KEY,
        baseURL: process.env.ARYA_ANTHROPIC_BASE_URL,
      });
      return anthropic(model);
    }
    case "openai": {
      const openai = createOpenAI({
        apiKey: process.env.OPENAI_API_KEY,
        baseURL: process.env.ARYA_OPENAI_BASE_URL,
      });
      return openai(model);
    }
    case "ollama": {
      const ollama = createOpenAICompatible({
        name: "ollama",
        baseURL: `${OLLAMA_URL}/v1`,
      });
      return ollama(model);
    }
    default:
      throw new Error(`unknown provider "${provider}"`);
  }
}

/** Local models currently available from Ollama (empty when not running). */
export async function listOllamaModels(): Promise<string[]> {
  try {
    const response = await fetch(`${OLLAMA_URL}/api/tags`, {
      signal: AbortSignal.timeout(1_500),
    });
    if (!response.ok) return [];
    const data = (await response.json()) as { models?: Array<{ name: string }> };
    return (data.models ?? []).map((m) => `ollama:${m.name}`);
  } catch {
    return [];
  }
}
