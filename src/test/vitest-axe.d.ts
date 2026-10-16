// Type augmentation: bridge vitest-axe's matcher to vitest's modern
// `Assertion` interface. vitest-axe targets vitest 1.x's `Vi.Assertion`
// namespace; vitest 3.x exports `Assertion` from `@vitest/expect`. This
// shim makes `expect(axe(...)).toHaveNoViolations()` type-check.

import "vitest";
import type { AxeMatchers } from "vitest-axe/matchers";

declare module "vitest" {
  interface Assertion<T = unknown> extends AxeMatchers {}
  interface AsymmetricMatchersContaining extends AxeMatchers {}
}
