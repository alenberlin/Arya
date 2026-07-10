import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useEffect } from "react";

/**
 * The event the Rust side emits with a completed dictation's final text when the
 * dictation targeted Arya itself (rather than an external app). In-app dictation
 * bypasses the OS clipboard/Accessibility path — which a ProseMirror
 * contenteditable rejects — and inserts through the focused editor's own API.
 */
export const DICTATION_INSERT_EVENT = "dictation:insert";

/**
 * Insert `text` at the caret of the focused `<input>`/`<textarea>`, updating a
 * React-controlled field via a synthetic `input` event (the native value setter
 * is used so React's value tracker sees the change and fires `onChange`).
 *
 * @returns `true` if a form field was focused and received the text; `false`
 * otherwise, letting the caller route the text to a richer editor.
 */
export function insertIntoFocusedFormField(text: string): boolean {
  const el = document.activeElement;
  if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) {
    return false;
  }
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? el.value.length;
  const next = el.value.slice(0, start) + text + el.value.slice(end);
  const proto =
    el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setValue = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  setValue?.call(el, next);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  const caret = start + text.length;
  el.setSelectionRange(caret, caret);
  return true;
}

/**
 * Listen for in-app dictation inserts and land the text in a focused form field
 * (note title, agent composer, search box, …). The note body handles its own
 * insert in {@link BlockEditor}, guarded by focus so exactly one target acts.
 */
export function useFormFieldDictation(): void {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    void listen<string>(DICTATION_INSERT_EVENT, (event) => {
      if (event.payload) insertIntoFocusedFormField(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);
}
