import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createOpenAI } from "@ai-sdk/openai";
import { generateImage } from "ai";

// Image generation is not yet routed through the Arya proxy. Whenever the proxy
// holds key custody (ARYA_API_URL set — the production configuration), image
// generation is disabled rather than reaching for a local provider key, which
// would bypass the proxy. It remains available only in a direct-key dev setup.
const PROXY_MODE = Boolean(process.env.ARYA_API_URL);

/**
 * Text-to-image via OpenAI's image model. Disabled in proxy mode until a
 * server-side image endpoint lands; returns the workspace-relative PNG path.
 */
export async function generateImageToWorkspace(
  workspace: string,
  prompt: string,
  size?: string,
): Promise<{ path: string; bytes: number }> {
  if (PROXY_MODE) {
    throw new Error(
      "image generation is not available in proxy mode yet (deferred to a later pass)",
    );
  }
  if (!process.env.OPENAI_API_KEY) {
    throw new Error("image generation needs a cloud image model; no OpenAI API key is configured");
  }
  const openai = createOpenAI({ apiKey: process.env.OPENAI_API_KEY });
  const { image } = await generateImage({
    model: openai.image("gpt-image-1"),
    prompt,
    size: (size ?? "1024x1024") as `${number}x${number}`,
  });
  const dir = join(workspace, "images");
  await mkdir(dir, { recursive: true });
  const name = `image-${Date.now()}.png`;
  const bytes = image.uint8Array;
  await writeFile(join(dir, name), bytes);
  return { path: `images/${name}`, bytes: bytes.length };
}

export function imageGenerationAvailable(): boolean {
  return !PROXY_MODE && Boolean(process.env.OPENAI_API_KEY);
}
