// Design-token constants, derived from `src/styles/tokens.css`. The CSS file
// is the single source of truth for values rendered in the DOM; this module
// exposes the same names so non-DOM code (canvas, dynamic styles, color-mix
// helpers, telemetry) can refer to them without hardcoding literals.
//
// Keep the two in lockstep: if you add a token to tokens.css, add it here.

/** Color names. Reference values in inline styles as `var(--name)` when the
 * element is in the DOM; use these constants only when you need a value
 * outside the DOM (canvas fills, computed colors for analytics, etc.). */
export const TOKENS = {
  // Semantic colors (light)
  bg: "var(--bg)",
  surface: "var(--surface)",
  surface2: "var(--surface-2)",
  surfaceRaise: "var(--surface-raise)",
  surfaceSunken: "var(--surface-sunken)",
  border: "var(--border)",
  borderStrong: "var(--border-strong)",
  borderSubtle: "var(--border-subtle)",
  text: "var(--text)",
  textSecondary: "var(--text-secondary)",
  textMuted: "var(--text-muted)",
  accent: "var(--accent)",
  accentHover: "var(--accent-hover)",
  accentSoft: "var(--accent-soft)",
  accentGhost: "var(--accent-ghost)",
  accentContrast: "var(--accent-contrast)",
  success: "var(--success)",
  successSoft: "var(--success-soft)",
  warning: "var(--warning)",
  warningSoft: "var(--warning-soft)",
  danger: "var(--danger)",
  dangerSoft: "var(--danger-soft)",
  info: "var(--info)",
  infoSoft: "var(--info-soft)",

  // Type
  fontSerif: "var(--font-serif)",
  fontSans: "var(--font-sans)",
  fontMono: "var(--font-mono)",

  // Radius
  radiusSm: "var(--r-sm)",
  radiusMd: "var(--r-md)",
  radiusLg: "var(--r-lg)",
  radiusXl: "var(--r-xl)",
  radiusPill: "var(--r-pill)",
} as const;

export type TokenName = keyof typeof TOKENS;

/** Reusable spacer names — 4px rhythm. */
export const SPACING = {
  sp1: "var(--sp-1)",
  sp2: "var(--sp-2)",
  sp3: "var(--sp-3)",
  sp4: "var(--sp-4)",
  sp5: "var(--sp-5)",
  sp6: "var(--sp-6)",
  sp8: "var(--sp-8)",
  sp10: "var(--sp-10)",
  sp12: "var(--sp-12)",
} as const;

/** Read the computed value of a CSS custom property from a particular root.
 * Useful for canvases (mindmap, force-graph) where we need the resolved
 * RGB to feed into a non-CSS drawing API. */
export function readCssVar(name: string, root: Element = document.documentElement): string {
  return getComputedStyle(root).getPropertyValue(name).trim();
}
