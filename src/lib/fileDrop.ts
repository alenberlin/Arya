import { useEffect, useRef, useState } from "react";

/**
 * Window-level file-drop plumbing.
 *
 * The main window sets `dragDropEnabled: false` (`src-tauri/tauri.conf.json`) so
 * in-page HTML5 dragging keeps working — note rows onto folder chips, BlockNote's
 * block handles. The cost is that wry installs no drag handler, so an OS file drag
 * that nothing cancels falls through to the webview's default action: it navigates
 * the window to the dropped file, replacing the app with a bare PDF or image view
 * that has no way back. {@link installFileDropGuard} cancels those drags for the
 * whole window so an unhandled drop is inert, and {@link useWindowFileDrop} lets
 * the visible surface claim the files.
 */

/** Whether a drag carries OS files, as opposed to a dragged in-page element. */
export function carriesFiles(event: { dataTransfer: DataTransfer | null }): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes("Files");
}

/**
 * Cancel OS file drags at the window so the webview can never navigate away from
 * the app. Drags carrying no files are left untouched, which is what keeps in-page
 * HTML5 drag and drop working.
 *
 * @returns A teardown that removes the listeners.
 */
export function installFileDropGuard(): () => void {
  const block = (event: DragEvent) => {
    if (carriesFiles(event)) event.preventDefault();
  };
  // dragenter and dragover must both be cancelled for the page to count as a
  // valid drop target; otherwise no drop event fires at all and the webview
  // performs its default navigation instead.
  window.addEventListener("dragenter", block);
  window.addEventListener("dragover", block);
  window.addEventListener("drop", block);
  return () => {
    window.removeEventListener("dragenter", block);
    window.removeEventListener("dragover", block);
    window.removeEventListener("drop", block);
  };
}

/**
 * Accept files dropped anywhere in the window while `enabled`. Requires
 * {@link installFileDropGuard} to have run — the guard is what cancels the drag,
 * and without it the browser never dispatches a drop.
 *
 * Only one surface renders at a time, so at most one caller claims a given drop.
 *
 * @param onFiles - Receives the dropped files; never called with an empty list.
 * @param enabled - When false the hook is inert and the guard swallows the drop.
 * @returns Whether a file drag is currently over the window, for drop affordances.
 */
export function useWindowFileDrop(onFiles: (files: File[]) => void, enabled = true): boolean {
  const [dragActive, setDragActive] = useState(false);
  // dragenter/dragleave fire again for every element the pointer crosses, so a
  // depth counter is what stops the affordance flickering over child nodes.
  const depth = useRef(0);
  const handler = useRef(onFiles);
  handler.current = onFiles;

  useEffect(() => {
    depth.current = 0;
    if (!enabled) {
      setDragActive(false);
      return;
    }
    const onEnter = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      depth.current += 1;
      setDragActive(true);
    };
    const onLeave = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setDragActive(false);
    };
    const onDrop = (event: DragEvent) => {
      if (!carriesFiles(event)) return;
      depth.current = 0;
      setDragActive(false);
      const files = Array.from(event.dataTransfer?.files ?? []);
      if (files.length > 0) handler.current(files);
    };
    window.addEventListener("dragenter", onEnter);
    window.addEventListener("dragleave", onLeave);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragenter", onEnter);
      window.removeEventListener("dragleave", onLeave);
      window.removeEventListener("drop", onDrop);
    };
  }, [enabled]);

  return dragActive;
}
