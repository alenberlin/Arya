/**
 * In-app dialogs. The macOS webview doesn't support window.confirm/prompt, so
 * these controlled React modals replace them (and read as native to the app).
 */
import { type ReactNode, useEffect, useId, useRef, useState } from "react";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** The shared modal shell: backdrop, focus trap, Escape-to-close, and focus
 * restore on close. Exported so richer one-off dialogs (e.g. the AI-sort
 * review) get the same behaviour without duplicating it. */
export function Modal({
  open,
  onClose,
  labelledBy,
  children,
}: {
  open: boolean;
  onClose: () => void;
  labelledBy: string;
  children: ReactNode;
}) {
  const modalRef = useRef<HTMLDivElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);
  // Capture the element that had focus BEFORE the modal's autoFocus moves it
  // inside, so it can be restored on close. Runs during the open render, ahead
  // of the autoFocus commit.
  if (open && restoreFocus.current === null) {
    restoreFocus.current = document.activeElement as HTMLElement | null;
  }

  // Escape to close + trap Tab within the dialog so focus can't wander to the
  // sidebar behind it.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
        return;
      }
      if (e.key !== "Tab") return;
      const nodes = modalRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE);
      if (!nodes || nodes.length === 0) return;
      const first = nodes[0];
      const last = nodes[nodes.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Return focus to the trigger when the modal closes (keyed on `open` alone so
  // an unstable onClose closure can't fire this on every re-render).
  useEffect(() => {
    if (!open) return;
    return () => {
      restoreFocus.current?.focus?.();
      restoreFocus.current = null;
    };
  }, [open]);

  if (!open) return null;
  return (
    <div className="modal-root">
      <button
        type="button"
        className="modal-backdrop"
        aria-label="Close dialog"
        onClick={onClose}
      />
      <div
        ref={modalRef}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={labelledBy}
      >
        {children}
      </div>
    </div>
  );
}

/** A yes/no confirmation. */
export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  return (
    <Modal open={open} onClose={onCancel} labelledBy={id}>
      <h3 id={id} className="modal-title">
        {title}
      </h3>
      {message ? <p className="modal-message">{message}</p> : null}
      <div className="modal-actions">
        <button type="button" className="btn-ghost" onClick={onCancel}>
          {cancelLabel}
        </button>
        <button
          type="button"
          className={danger ? "btn-danger-solid" : "btn-primary"}
          // biome-ignore lint/a11y/noAutofocus: focusing the primary action in a modal is expected
          autoFocus
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}

/** A single-line text prompt (replaces window.prompt). */
export function PromptDialog({
  open,
  title,
  label,
  placeholder,
  initialValue = "",
  submitLabel = "Create",
  onSubmit,
  onCancel,
}: {
  open: boolean;
  title: string;
  label?: string;
  placeholder?: string;
  initialValue?: string;
  submitLabel?: string;
  onSubmit: (value: string) => void;
  onCancel: () => void;
}) {
  const id = useId();
  const [value, setValue] = useState(initialValue);
  useEffect(() => {
    if (open) setValue(initialValue);
  }, [open, initialValue]);

  return (
    <Modal open={open} onClose={onCancel} labelledBy={id}>
      <h3 id={id} className="modal-title">
        {title}
      </h3>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (value.trim()) onSubmit(value.trim());
        }}
      >
        {label ? <div className="modal-label">{label}</div> : null}
        <input
          // biome-ignore lint/a11y/noAutofocus: focusing the input in a prompt modal is expected
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={placeholder}
          aria-label={label ?? title}
        />
        <div className="modal-actions">
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="btn-primary" disabled={!value.trim()}>
            {submitLabel}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/**
 * A destructive confirmation that requires typing a phrase (default "confirm"),
 * so nothing irreversible happens on an accidental click.
 */
export function TypeToConfirmDialog({
  open,
  title,
  message,
  phrase = "confirm",
  confirmLabel = "Delete",
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  message?: string;
  phrase?: string;
  confirmLabel?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const id = useId();
  const [value, setValue] = useState("");
  useEffect(() => {
    if (open) setValue("");
  }, [open]);
  const matches = value.trim().toLowerCase() === phrase.toLowerCase();

  return (
    <Modal open={open} onClose={onCancel} labelledBy={id}>
      <h3 id={id} className="modal-title">
        {title}
      </h3>
      {message ? <p className="modal-message">{message}</p> : null}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (matches) onConfirm();
        }}
      >
        <div className="modal-label">
          Type <strong>{phrase}</strong> to continue
        </div>
        <input
          // biome-ignore lint/a11y/noAutofocus: focusing the input in this modal is expected
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={phrase}
          aria-label="type to confirm"
        />
        <div className="modal-actions">
          <button type="button" className="btn-ghost" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" className="btn-danger-solid" disabled={!matches}>
            {confirmLabel}
          </button>
        </div>
      </form>
    </Modal>
  );
}
