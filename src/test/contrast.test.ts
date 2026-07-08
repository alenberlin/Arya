import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const css = readFileSync(join(process.cwd(), "src/styles/tokens.css"), "utf8");

function block(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escaped}\\s*\\{([\\s\\S]*?)\\n\\}`));
  if (!match) throw new Error(`missing ${selector}`);
  return match[1];
}

function token(scope: string, name: string): string {
  const match = scope.match(new RegExp(`${name}:\\s*(#[0-9a-fA-F]{6})`));
  if (!match) throw new Error(`missing ${name}`);
  return match[1];
}

function srgb(channel: number): number {
  const value = channel / 255;
  return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const raw = Number.parseInt(hex.slice(1), 16);
  const r = srgb((raw >> 16) & 255);
  const g = srgb((raw >> 8) & 255);
  const b = srgb(raw & 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

describe("design token contrast", () => {
  it("keeps muted text AA-readable on the primary surface", () => {
    const light = block(":root");
    const dark = block('[data-theme="dark"]');

    expect(
      contrast(token(light, "--text-muted"), token(light, "--surface")),
    ).toBeGreaterThanOrEqual(4.5);
    expect(contrast(token(dark, "--text-muted"), token(dark, "--surface"))).toBeGreaterThanOrEqual(
      4.5,
    );
  });
});
