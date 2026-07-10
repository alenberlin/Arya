import { beforeEach, describe, expect, it, vi } from "vitest";
import { insertIntoFocusedFormField } from "../lib/dictationInsert";

describe("insertIntoFocusedFormField", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("inserts at the caret of a focused input and reports success", () => {
    const input = document.createElement("input");
    document.body.append(input);
    input.value = "Hello world";
    input.focus();
    input.setSelectionRange(5, 5); // caret right after "Hello"

    const handled = insertIntoFocusedFormField(", there");

    expect(handled).toBe(true);
    expect(input.value).toBe("Hello, there world");
    expect(input.selectionStart).toBe(12); // caret sits after the inserted text
  });

  it("replaces the current selection rather than duplicating it", () => {
    const input = document.createElement("input");
    document.body.append(input);
    input.value = "draft title";
    input.focus();
    input.setSelectionRange(0, 5); // select "draft"

    insertIntoFocusedFormField("final");

    expect(input.value).toBe("final title");
  });

  it("fires an input event so a React-controlled field updates", () => {
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();
    const onInput = vi.fn();
    input.addEventListener("input", onInput);

    insertIntoFocusedFormField("spoken words");

    expect(onInput).toHaveBeenCalledTimes(1);
    expect(input.value).toBe("spoken words");
  });

  it("works on a textarea", () => {
    const textarea = document.createElement("textarea");
    document.body.append(textarea);
    textarea.value = "line";
    textarea.focus();
    textarea.setSelectionRange(4, 4);

    expect(insertIntoFocusedFormField(" two")).toBe(true);
    expect(textarea.value).toBe("line two");
  });

  it("declines (returns false) when a contenteditable — not a form field — is focused", () => {
    const editable = document.createElement("div");
    editable.contentEditable = "true";
    editable.tabIndex = 0;
    document.body.append(editable);
    editable.focus();

    // The note body is a contenteditable; it must be handled by the editor's own
    // API, not this form-field path — so the helper reports it as unhandled.
    expect(insertIntoFocusedFormField("body text")).toBe(false);
  });
});
