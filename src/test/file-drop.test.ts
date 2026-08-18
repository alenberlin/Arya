import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { carriesFiles, installFileDropGuard, useWindowFileDrop } from "../lib/fileDrop";
import { kbAccepts } from "../lib/kb";

/**
 * jsdom implements neither DragEvent nor DataTransfer, so drags are synthesized
 * with the two fields the module actually reads.
 */
function drag(type: string, init: { types: string[]; files?: File[] }): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: { types: init.types, files: init.files ?? [] },
  });
  return event;
}

const fileDrag = (type: string, files: File[] = []) => drag(type, { types: ["Files"], files });
const elementDrag = (type: string) => drag(type, { types: ["text/plain"] });

let teardown: (() => void) | null = null;
afterEach(() => {
  teardown?.();
  teardown = null;
});

describe("installFileDropGuard", () => {
  // The regression this exists for: with dragDropEnabled false and nothing
  // cancelling the drag, the webview navigates to the dropped file and the app
  // is replaced by a PDF viewer with no way back.
  it("cancels file drags so the webview cannot navigate to the file", () => {
    teardown = installFileDropGuard();
    for (const type of ["dragenter", "dragover", "drop"]) {
      const event = fileDrag(type);
      window.dispatchEvent(event);
      expect(event.defaultPrevented, `${type} must be cancelled`).toBe(true);
    }
  });

  it("leaves in-page element drags alone", () => {
    teardown = installFileDropGuard();
    // Dragging a note row onto a folder chip, or a BlockNote block handle: the
    // guard must not touch these or in-app dragging stops working.
    for (const type of ["dragenter", "dragover", "drop"]) {
      const event = elementDrag(type);
      window.dispatchEvent(event);
      expect(event.defaultPrevented, `${type} must pass through`).toBe(false);
    }
  });

  it("stops cancelling once torn down", () => {
    installFileDropGuard()();
    const event = fileDrag("dragover");
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});

describe("useWindowFileDrop", () => {
  it("hands dropped files to the surface", () => {
    const onFiles = vi.fn();
    renderHook(() => useWindowFileDrop(onFiles));
    const file = new File(["%PDF-1.7"], "paper.pdf", { type: "application/pdf" });

    act(() => {
      window.dispatchEvent(fileDrag("drop", [file]));
    });

    expect(onFiles).toHaveBeenCalledTimes(1);
    expect(onFiles.mock.calls[0][0]).toEqual([file]);
  });

  it("ignores drops that carry no files, and empty file lists", () => {
    const onFiles = vi.fn();
    renderHook(() => useWindowFileDrop(onFiles));

    act(() => {
      window.dispatchEvent(elementDrag("drop"));
      window.dispatchEvent(fileDrag("drop", []));
    });

    expect(onFiles).not.toHaveBeenCalled();
  });

  it("keeps the affordance up while the pointer crosses child elements", () => {
    const { result } = renderHook(() => useWindowFileDrop(vi.fn()));
    expect(result.current).toBe(false);

    // enter parent, enter child, leave parent: still over the window.
    act(() => {
      window.dispatchEvent(fileDrag("dragenter"));
      window.dispatchEvent(fileDrag("dragenter"));
      window.dispatchEvent(fileDrag("dragleave"));
    });
    expect(result.current).toBe(true);

    act(() => {
      window.dispatchEvent(fileDrag("dragleave"));
    });
    expect(result.current).toBe(false);
  });

  it("is inert while disabled", () => {
    const onFiles = vi.fn();
    const { result } = renderHook(() => useWindowFileDrop(onFiles, false));

    act(() => {
      window.dispatchEvent(fileDrag("dragenter"));
      window.dispatchEvent(fileDrag("drop", [new File(["x"], "a.txt")]));
    });

    expect(onFiles).not.toHaveBeenCalled();
    expect(result.current).toBe(false);
  });

  it("stops listening when the surface unmounts", () => {
    const onFiles = vi.fn();
    renderHook(() => useWindowFileDrop(onFiles)).unmount();

    act(() => {
      window.dispatchEvent(fileDrag("drop", [new File(["x"], "a.txt")]));
    });

    expect(onFiles).not.toHaveBeenCalled();
  });
});

describe("carriesFiles", () => {
  it("distinguishes OS file drags from element drags", () => {
    expect(carriesFiles({ dataTransfer: { types: ["Files"] } as unknown as DataTransfer })).toBe(
      true,
    );
    expect(
      carriesFiles({ dataTransfer: { types: ["text/plain"] } as unknown as DataTransfer }),
    ).toBe(false);
    expect(carriesFiles({ dataTransfer: null })).toBe(false);
  });
});

describe("kbAccepts", () => {
  it("accepts what the ingestion pipeline can read, regardless of case", () => {
    expect(kbAccepts("paper.pdf")).toBe(true);
    expect(kbAccepts("SCAN.PDF")).toBe(true);
    expect(kbAccepts("notes.with.dots.md")).toBe(true);
  });

  it("rejects everything else", () => {
    expect(kbAccepts("archive.zip")).toBe(false);
    expect(kbAccepts("Makefile")).toBe(false);
    expect(kbAccepts("")).toBe(false);
  });
});
