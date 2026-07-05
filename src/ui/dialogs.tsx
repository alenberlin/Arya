/**
 * In-app dialogs. The macOS webview doesn't support window.confirm/prompt, so
 * these controlled React modals replace them (and read as native to the app).
 */
import { type ReactNode, useEffect, useId, useState } from "react";

function Modal({
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
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div className="modal-root">
      <button
        type="button"
        className="modal-backdrop"
        aria-label="Close dialog"
        onClick={onClose}
      />
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby={labelledBy}>
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
