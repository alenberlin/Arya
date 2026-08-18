import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { KnowledgeBasePanel } from "../knowledge/KnowledgeBasePanel";

// The Rust side has its own tests; the mock stands in for the shell so these
// prove UI wiring. Layout is not covered here — jsdom performs none.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

// The chat pane is not under test, and its thread scroller calls
// Element.scrollTo, which jsdom does not implement.
vi.mock("../knowledge/KbChat", () => ({ KbChat: () => null }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    switch (cmd) {
      case "kb_status":
        return { embedderAvailable: true };
      case "kb_list_collections":
        return [
          {
            id: "c1",
            name: "Fitness",
            description: "",
            createdAt: "2026-08-18T06:31:08.419Z",
            updatedAt: "2026-08-18T06:31:08.419Z",
            documentCount: 1,
            readyCount: 1,
          },
        ];
      case "kb_list_documents":
      case "kb_list_sessions":
      case "kb_get_messages":
        return [];
      default:
        return null;
    }
  }),
}));

/** jsdom has no DataTransfer, so a file drag is synthesized by hand. */
function fileDrop(files: File[]): Event {
  const event = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { types: ["Files"], files } });
  return event;
}

/** Renders the surface; the first collection opens on its own. */
async function openCollection() {
  const { unmount } = render(<KnowledgeBasePanel />);
  const toggle = await screen.findByRole("button", { name: /(Hide|Show) documents/ });
  return { toggle, unmount };
}

const docsPanel = () => document.getElementById("kb-docs-panel");

describe("documents panel collapse", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("collapses and restores the documents panel", async () => {
    const { toggle } = await openCollection();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(docsPanel()).not.toHaveClass("collapsed");

    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(docsPanel()).toHaveClass("collapsed");
    expect(toggle).toHaveTextContent("Show documents");

    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(docsPanel()).not.toHaveClass("collapsed");
  });

  it("remembers the collapsed state across mounts", async () => {
    const { toggle, unmount } = await openCollection();
    await userEvent.click(toggle);
    expect(localStorage.getItem("arya-kb-docs-collapsed")).toBe("true");

    // A fresh mount reads the stored preference rather than defaulting open.
    unmount();
    const { toggle: remounted } = await openCollection();
    expect(remounted).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps ingesting dropped files while collapsed, and reveals the panel", async () => {
    // The panel is hidden with CSS rather than unmounted precisely so the
    // window-wide drop listener survives; a drop must still land and show.
    const { toggle } = await openCollection();
    await userEvent.click(toggle);
    expect(docsPanel()).toHaveClass("collapsed");

    window.dispatchEvent(fileDrop([new File(["%PDF-1.7"], "paper.pdf")]));

    await waitFor(() => expect(docsPanel()).not.toHaveClass("collapsed"));
  });
});
