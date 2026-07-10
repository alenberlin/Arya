import { createAnthropic } from "@ai-sdk/anthropic";
import { createOpenAI } from "@ai-sdk/openai";
import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import type { LanguageModel } from "ai";

const OLLAMA_URL = process.env.ARYA_OLLAMA_URL ?? "http://127.0.0.1:11434";

function aryaApiUrl(): string | undefined {
  return process.env.ARYA_API_URL || undefined;
}

function proxyHeaders(): Record<string, string> {
  const token = process.env.ARYA_API_TOKEN;
  if (!token) {
    throw new Error("ARYA_API_TOKEN is required when ARYA_API_URL is set");
  }
  return { authorization: `Bearer ${token}` };
}

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
      const proxyUrl = aryaApiUrl();
      if (proxyUrl) {
        // Route through the proxy: it holds the key and meters usage.
        const proxied = createOpenAICompatible({
          name: "arya-anthropic",
          baseURL: `${proxyUrl}/v1/anthropic`,
          headers: proxyHeaders(),
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
      const proxyUrl = aryaApiUrl();
      if (proxyUrl) {
        const proxied = createOpenAICompatible({
          name: "arya-openai",
          baseURL: `${proxyUrl}/v1/openai`,
          headers: proxyHeaders(),
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
  const proxyUrl = aryaApiUrl();
  if (proxyUrl) {
    try {
      const response = await fetch(`${proxyUrl}/v1/models`, {
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

// Ollama lists embedding-only models (e.g. nomic-embed-text) alongside chat
// models. They reject chat with "does not support chat", which surfaces as an
// AI_NoOutputGeneratedError — so they must never be offered as agent models.
// These families are embedding-only; used only when /api/show predates the
// `capabilities` field.
const EMBEDDING_FAMILIES = new Set(["bert", "nomic-bert", "xlm-roberta", "gte"]);

/**
 * Whether an Ollama model can generate chat completions (vs. being embedding-
 * only). Prefers Ollama's authoritative `capabilities` list; falls back to a
 * name/family heuristic for models that predate that field.
 */
export function isChatModel(
  name: string,
  family: string | undefined,
  capabilities: string[] | undefined,
): boolean {
  if (Array.isArray(capabilities)) return capabilities.includes("completion");
  if (family && EMBEDDING_FAMILIES.has(family)) return false;
  return !/embed/i.test(name);
}

/** A model's capabilities via /api/show, or undefined when unavailable. */
async function ollamaCapabilities(name: string): Promise<string[] | undefined> {
  try {
    const res = await fetch(`${OLLAMA_URL}/api/show`, {
      method: "POST",
      body: JSON.stringify({ model: name }),
      signal: AbortSignal.timeout(2_000),
    });
    if (!res.ok) return undefined;
    const info = (await res.json()) as { capabilities?: string[] };
    return Array.isArray(info.capabilities) ? info.capabilities : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Local chat models available from Ollama (empty when not running). Embedding-
 * only models are filtered out: they can't generate, and offering one as an
 * agent model produces an AI_NoOutputGeneratedError on the first message.
 */
export async function listOllamaModels(): Promise<string[]> {
  try {
    const response = await fetch(`${OLLAMA_URL}/api/tags`, {
      signal: AbortSignal.timeout(1_500),
    });
    if (!response.ok) return [];
    const data = (await response.json()) as {
      models?: Array<{ name: string; details?: { family?: string } }>;
    };
    const models = data.models ?? [];
    const resolved = await Promise.all(
      models.map(async (m) => {
        const capabilities = await ollamaCapabilities(m.name);
        return isChatModel(m.name, m.details?.family, capabilities) ? `ollama:${m.name}` : null;
      }),
    );
    return resolved.filter((m): m is string => m !== null);
  } catch {
    return [];
  }
}
