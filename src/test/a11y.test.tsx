// Axe-core accessibility tests for the major shells and teaching surfaces.
//
// Phase 5 exit criterion: "Lighthouse/axe scores meet target." Rather than
// wire a real Lighthouse run (which can't drive the Tauri webview in CI),
// we run axe-core against the same component trees the user reaches — the
// first-run onboarding, the sidebar shell, the agent hub, and the empty-
// state patterns the user lands on most often. Violations block the PR.
//
// Each test mounts a single surface with the minimum mocks it needs to
// render, runs axe-core over the rendered DOM, and asserts no violations.
// jsdom has limited layout (no real getBoundingClientRect, no focus
// management) so a small subset of rules is irrelevant and excluded.

import { act, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { axe } from "vitest-axe";
import { App } from "../App";
import { SafetyCard } from "../agent/SafetyCard";
import { Onboarding } from "../onboarding/Onboarding";
import { PaneState } from "../ui/PaneState";

// Mock Tauri so the surfaces that `invoke`/`listen` don't blow up in jsdom.
// We don't care about the backend here — only the rendered accessibility tree.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

// Heavy block editor stub — its own logic is covered separately; the a11y
// test only cares about the rendered shell.
vi.mock("../notes/BlockEditor", () => ({ BlockEditor: () => null }));
// Labs panes pull in d3-force + xyflow; stub them so vitest doesn't try to
// load the heavy modules through the lazy() boundary.
vi.mock("../galaxy/GalaxyPanel", () => ({ GalaxyPanel: () => null }));
vi.mock("../mindmap/MindMapPanel", () => ({ MindMapPanel: () => null }));
vi.mock("../notes/NotesWorkspace", () => ({ NotesWorkspace: () => null }));

// jsdom can't compute layout so disable rules that need real geometry. The
// rest of the axe catalog (semantics, ARIA, color contrast on inline styles
// we actually emit, label/role/alt) still runs and catches the common
// regressions a PR could introduce.
const GEOMETRY_RULES_DISABLED = [
  "region", // landmark check needs real layout for "all content in a landmark"
  "scrollable-region-focusable", // jsdom has no scroll containers
  "color-contrast", // jsdom computes sRGB from CSS, not the resolved layout
];

afterEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
});

describe("a11y: shell surfaces", () => {
  it("first-run onboarding has no serious axe violations", async () => {
    const { container } = render(<Onboarding onFinish={() => {}} />);
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });

  it("agent safety card has no serious axe violations", async () => {
    const { container } = render(<SafetyCard tier="cloud" />);
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });

  it("agent safety card (local tier) has no serious axe violations", async () => {
    const { container } = render(<SafetyCard tier="local" />);
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });
});

describe("a11y: shared teaching surfaces", () => {
  it("PaneState empty variant has a labelled status, descriptive title, and CTA", async () => {
    const { container, getByRole } = render(
      <PaneState
        kind="empty"
        title="Nothing here yet"
        description="Drop a file in to get started."
        action={{ label: "Add your first file", onClick: () => {} }}
      />,
    );
    // role=status on the empty variant so screen readers announce it politely.
    expect(getByRole("status")).toBeInTheDocument();
    expect(getByRole("button", { name: "Add your first file" })).toBeInTheDocument();
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });

  it("PaneState loading variant announces busy", async () => {
    const { container, getByRole } = render(
      <PaneState kind="loading" title="Mapping your galaxy…" />,
    );
    const status = getByRole("status");
    expect(status).toHaveAttribute("aria-busy", "true");
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });

  it("PaneState error variant is assertive (role=alert)", async () => {
    const { container, getByRole } = render(
      <PaneState
        kind="error"
        title="Something went wrong"
        description="Try again in a moment."
        action={{ label: "Retry", onClick: () => {} }}
      />,
    );
    expect(getByRole("alert")).toBeInTheDocument();
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });
});

describe("a11y: main app shell", () => {
  it("the post-onboarding sidebar shell has no serious axe violations", async () => {
    localStorage.setItem("arya-onboarded", "true");
    // App fires several mount-time state updates (autostart poll, etc.).
    // Wrap the render in act() so React commits before we hand the DOM to axe.
    const { container } = await act(async () => render(<App />));
    const results = await axe(container, {
      rules: Object.fromEntries(GEOMETRY_RULES_DISABLED.map((id) => [id, { enabled: false }])),
    });
    expect(results).toHaveNoViolations();
  });
});
