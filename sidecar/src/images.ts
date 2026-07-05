import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { createOpenAI } from "@ai-sdk/openai";
import { generateImage } from "ai";

/**
 * Text-to-image via OpenAI's image model (the first cloud image provider;
 * the Arya API proxy takes over key custody in M11/M12). Returns the
 * workspace-relative path of the saved PNG.
 */
export async function generateImageToWorkspace(
  workspace: string,
  prompt: string,
  size?: string,
): Promise<{ path: string; bytes: number }> {
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
  return Boolean(process.env.OPENAI_API_KEY);
}
