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

/**
 * Cloud models available to the desktop. In proxy mode (ARYA_API_URL set) they
 * come from the Arya API catalog (/v1/models) — the source of truth — so the
 * list reflects what the server can actually serve, not what local keys exist.
 * In a direct-key dev setup they come from the provider keys in the env.
 */
export async function listCloudModels(): Promise<string[]> {
  if (ARYA_API_URL) {
    try {
      const response = await fetch(`${ARYA_API_URL}/v1/models`, {
        signal: AbortSignal.timeout(2_000),
      });
      if (!response.ok) return [];
      const data = (await response.json()) as { models?: Array<{ id: string }> };
      // Ollama (local) models are listed separately via listOllamaModels().
      return (data.models ?? []).map((m) => m.id).filter((id) => id && !id.startsWith("ollama:"));
    } catch {
      return [];
    }
  }
  const cloud: string[] = [];
  if (process.env.ANTHROPIC_API_KEY) {
    cloud.push("anthropic:claude-sonnet-5", "anthropic:claude-opus-4-8");
  }
  if (process.env.OPENAI_API_KEY) {
    cloud.push("openai:gpt-5.2", "openai:gpt-5-mini");
  }
  return cloud;
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
