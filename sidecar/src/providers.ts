import { createAnthropic } from "@ai-sdk/anthropic";
import { createOpenAI } from "@ai-sdk/openai";
import { createOpenAICompatible } from "@ai-sdk/openai-compatible";
import type { LanguageModel } from "ai";

const OLLAMA_URL = process.env.ARYA_OLLAMA_URL ?? "http://127.0.0.1:11434";

/**
 * Resolves "provider:model" to a language model.
 *   anthropic:* / openai:*  - direct APIs with the user's own key
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

const OPENAI_BASE = process.env.ARYA_OPENAI_BASE_URL ?? "https://api.openai.com/v1";
const ANTHROPIC_BASE = process.env.ARYA_ANTHROPIC_BASE_URL ?? "https://api.anthropic.com";

/** GET a JSON endpoint with a timeout; undefined on any failure (offline, 401, …). */
async function fetchJson(url: string, headers: Record<string, string>): Promise<unknown> {
  try {
    const res = await fetch(url, { headers, signal: AbortSignal.timeout(5_000) });
    if (!res.ok) return undefined;
    return await res.json();
  } catch {
    return undefined;
  }
}

/** Static fallback when the provider's list can't be fetched. */
const ANTHROPIC_FALLBACK = ["claude-sonnet-5", "claude-opus-4-8"];
const OPENAI_FALLBACK = ["gpt-5.2", "gpt-5-mini"];

/** Chat models only — drop embedding / image / audio / realtime / old completions. */
function isOpenAiChatModel(id: string): boolean {
  return (
    /^(gpt-|o1|o3|o4)/i.test(id) &&
    !/-(embedding|audio|image|tts|whisper|realtime|davinci|babbage)/i.test(id)
  );
}

function isAnthropicChatModel(id: string): boolean {
  return /^claude-/i.test(id) && !/-embedding/i.test(id);
}

async function anthropicModels(key: string): Promise<string[]> {
  const data = await fetchJson(`${ANTHROPIC_BASE}/v1/models`, {
    "x-api-key": key,
    "anthropic-version": "2023-06-01",
  });
  const list = (data as { data?: Array<{ id?: string }> } | undefined)?.data ?? [];
  const models = list
    .map((m) => m.id)
    .filter((id): id is string => !!id && isAnthropicChatModel(id));
  return models.length ? models : ANTHROPIC_FALLBACK;
}

async function openaiModels(key: string): Promise<string[]> {
  const data = await fetchJson(`${OPENAI_BASE}/models`, {
    authorization: `Bearer ${key}`,
  });
  const list = (data as { data?: Array<{ id?: string }> } | undefined)?.data ?? [];
  const models = list.map((m) => m.id).filter((id): id is string => !!id && isOpenAiChatModel(id));
  return models.length ? models : OPENAI_FALLBACK;
}

/**
 * Cloud models available to the desktop, fetched live from each provider whose
 * key is set (so the picker shows the user's actual models, not a stale list),
 * with a static fallback when the provider is unreachable.
 */
export async function listCloudModels(): Promise<string[]> {
  const [anthropic, openai] = await Promise.all([
    process.env.ANTHROPIC_API_KEY
      ? anthropicModels(process.env.ANTHROPIC_API_KEY)
      : Promise.resolve([]),
    process.env.OPENAI_API_KEY ? openaiModels(process.env.OPENAI_API_KEY) : Promise.resolve([]),
  ]);
  return [...anthropic.map((m) => `anthropic:${m}`), ...openai.map((m) => `openai:${m}`)];
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
