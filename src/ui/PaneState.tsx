// Shared "this pane is in this state" surface — empty / loading / error.
//
// Centralises the visual + ARIA contract so every pane shows a coherent
// teaching line + a single primary action, instead of each panel inventing
// its own. Each variant composes the same shell so the user never has to
// re-learn what a pane "in trouble" looks like as they switch tabs.

import type { ReactNode } from "react";
import { AgentIcon, SearchIcon, ShieldIcon } from "./icons";

export type PaneStateKind = "empty" | "loading" | "error";

export interface PaneStateProps {
  kind: PaneStateKind;
  /** One-sentence headline explaining the state. */
  title: string;
  /** Optional supporting line (one sentence, no walls of text). */
  description?: string;
  /** Optional primary action (rendered as a button). */
  action?: { label: string; onClick: () => void };
  /** Override the default icon for this kind. */
  icon?: ReactNode;
}

export function PaneState({
  kind,
  title,
  description,
  action,
  icon,
}: PaneStateProps) {
  return (
    <div
      className={`pane-state pane-state-${kind}`}
      role={kind === "error" ? "alert" : "status"}
      aria-live={kind === "error" ? "assertive" : "polite"}
      aria-busy={kind === "loading"}
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        gap: 8,
        padding: "32px 24px",
        color: "var(--text-secondary)",
        flex: 1,
        minHeight: 220,
      }}
    >
      <div
        aria-hidden="true"
        style={{
          width: 36,
          height: 36,
          borderRadius: 10,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: kind === "error" ? "var(--danger)" : "var(--text-muted)",
        }}
      >
        {icon ?? defaultIcon(kind)}
      </div>
      <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>{title}</div>
      {description ? (
        <div style={{ fontSize: 13, maxWidth: 380, lineHeight: 1.5 }}>{description}</div>
      ) : null}
      {action ? (
        <button
          type="button"
          className="btn-sm btn-primary"
          onClick={action.onClick}
          style={{ marginTop: 6 }}
        >
          {action.label}
        </button>
      ) : null}
    </div>
  );
}

function defaultIcon(kind: PaneStateKind): ReactNode {
  switch (kind) {
    case "loading":
      return <span className="spinner" aria-hidden="true" />;
    case "error":
      return <ShieldIcon />;
    case "empty":
    default:
      return <SearchIcon />;
  }
}

// Re-export a couple of common icon aliases so call-sites can swap the icon
// without pulling the full icon set.
export { AgentIcon };
