import "@testing-library/jest-dom/vitest";
import * as axeMatchers from "vitest-axe/matchers";
import "vitest-axe/extend-expect";
import { expect } from "vitest";

// Register a11y matchers (`toHaveNoViolations`) so any a11y test can call
// `expect(axe(container)).toHaveNoViolations()`.
expect.extend(axeMatchers);

// axe-core calls `HTMLCanvasElement.prototype.getContext` to detect
// icon-ligature glyphs (the only way axe has of knowing an <i class="icon">
// renders as text rather than imagery). jsdom doesn't implement canvas, so
// stub it to a no-op and silence the "Not implemented" stderr noise that
// would otherwise dominate a11y test output.
if (typeof HTMLCanvasElement !== "undefined") {
  // biome-ignore lint/suspicious/noExplicitAny: stubbing a jsdom-missing API.
  HTMLCanvasElement.prototype.getContext = (() => null) as any;
}

// jsdom implements neither ResizeObserver nor matchMedia; the dictation pill
// uses ResizeObserver (to size its window) and matchMedia (to resolve Auto
// theme) on mount.
if (!window.ResizeObserver) {
  window.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}
if (!window.matchMedia) {
  window.matchMedia = ((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}
