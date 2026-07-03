import { createAnthropic } from "@ai-sdk/anthropic";
import { createOpenAI } from "@ai-sdk/openai";
import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import type { LanguageModel } from "ai";

const OLLAMA_URL = process.env.ARYA_OLLAMA_URL ?? "http://127.0.0.1:11434";
// When set, cloud providers route through the Arya API proxy (which holds
// the real keys). The desktop app never carries provider keys.
const ARYA_API_URL = process.env.ARYA_API_URL;
const ARYA_API_TOKEN = process.env.ARYA_API_TOKEN ?? "local-dev-token";

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
      if (ARYA_API_URL) {
        // Route through the proxy: it holds the key and meters usage.
        const proxied = createOpenAICompatible({
          name: "arya-anthropic",
          baseURL: `${ARYA_API_URL}/v1/anthropic`,
          headers: { authorization: `Bearer ${ARYA_API_TOKEN}` },
        });
        return proxied(model);
      }
      const anthropic = createAnthropic({
        apiKey: process.env.ANTHROPIC_API_KEY,
        baseURL: process.env.ARYA_ANTHROPIC_BASE_URL,
      });
      return anthropic(model);
    }
    case "openai": {
      if (ARYA_API_URL) {
        const proxied = createOpenAICompatible({
          name: "arya-openai",
          baseURL: `${ARYA_API_URL}/v1/openai`,
          headers: { authorization: `Bearer ${ARYA_API_TOKEN}` },
        });
        return proxied(model);
      }
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
