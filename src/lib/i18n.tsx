// Minimal in-tree i18n scaffold. The codebase ships English-only today, but
// every user-facing literal should still be looked up via this catalog so we
// can drop in a second locale without grep-and-replace across panels.
//
// API:
//   const t = useT();
//   t("notes.empty.noFilter")  // -> "No notes yet. Press Record to capture one."
//
// Adding a new locale: copy `en.ts` to `xx.ts` and translate; the hook will
// fall back to English for any missing key so a partial locale keeps the
// app usable.

import { useMemo } from "react";
import { en } from "./locales/en";

export type Catalog = Record<string, string>;
export type Locale = "en";

const CATALOGS: Record<Locale, Catalog> = { en };

/**
 * `useT` returns a translator bound to the current locale. Resolves a key,
 * interpolates `{name}`-style placeholders with the values map, and falls
 * back to the English string when a translation hasn't been authored yet
 * — keeping a partial locale useful rather than showing English keys.
 */
export function useT(): (key: string, vars?: Record<string, string | number>) => string {
  // Today there's only one locale; this is the hook seam we'll flip when
  // the user locale becomes configurable.
  const locale: Locale = "en";
  return useMemo(() => {
    const fallback = CATALOGS.en;
    const active = CATALOGS[locale] ?? fallback;
    return (key: string, vars?: Record<string, string | number>) => {
      const raw = active[key] ?? fallback[key] ?? key;
      if (!vars) return raw;
      return raw.replace(/\{(\w+)\}/g, (_, name) =>
        Object.hasOwn(vars, name) ? String(vars[name]) : `{${name}}`,
      );
    };
  }, []);
}

/** Read a string synchronously outside React (for non-component code paths). */
export function tSync(key: string, vars?: Record<string, string | number>): string {
  const raw = CATALOGS.en[key] ?? key;
  if (!vars) return raw;
  return raw.replace(/\{(\w+)\}/g, (_, name) =>
    Object.hasOwn(vars, name) ? String(vars[name]) : `{${name}}`,
  );
}
