// Sidebar listing agent sessions + the model picker. Pure presentation —
// state lives in the parent (`AgentPanel`), which passes session/model data
// down and the callbacks that the list fires.

import type { AgentSession } from "../lib/agent";
import { modelPrivacy } from "../lib/agent";
import { MoreIcon, PlusIcon } from "../ui/icons";

export interface AgentSessionListProps {
  sessions: AgentSession[];
  activeId: string | null;
  models: string[];
  model: string;
  onNewSession: () => void;
  onSelectSession: (session: AgentSession) => void;
  onSetModel: (model: string) => void;
  onOpenSessionMenu: (sessionId: string, x: number, y: number) => void;
}

/** Sidebar: model picker on top, list of recent sessions underneath. */
export function AgentSessionList({
  sessions,
  activeId,
  models,
  model,
  onNewSession,
  onSelectSession,
  onSetModel,
  onOpenSessionMenu,
}: AgentSessionListProps) {
  const tier = model ? modelPrivacy(model) : null;
  return (
    <div className="panel" style={{ width: 270, flex: "0 0 270px" }}>
      <div className="panel-head">
        <button
          type="button"
          className="btn"
          style={{ width: "100%" }}
          onClick={onNewSession}
          disabled={!model}
        >
          <PlusIcon /> New chat
        </button>
      </div>
      <div style={{ padding: 12, borderBottom: "1px solid var(--border-subtle)" }}>
        <div className="section-label" style={{ marginBottom: 8, padding: "0 4px" }}>
          Model
        </div>
        <select aria-label="agent model" value={model} onChange={(e) => onSetModel(e.target.value)}>
          {models.map((m) => (
            <option key={m} value={m}>
              {m} {modelPrivacy(m) === "local" ? "(local, free)" : "(cloud)"}
            </option>
          ))}
        </select>
        {tier ? (
          <div className="hstack" style={{ marginTop: 6, padding: "0 4px", fontSize: 11 }}>
            <span
              className="tier-dot"
              style={{ background: tier === "local" ? "var(--success)" : "var(--warning)" }}
            />
            <span style={{ color: tier === "local" ? "var(--success)" : "var(--warning)" }}>
              {tier === "local" ? "On-device · free · private" : "Cloud"}
            </span>
          </div>
        ) : null}
      </div>
      <div className="panel-body">
        <div className="section-label" style={{ padding: "4px 8px" }}>
          Recent
        </div>
        <ul aria-label="agent sessions" className="plain">
          {sessions.map((session) => (
            <li key={session.id} className="hstack" style={{ gap: 2 }}>
              <button
                type="button"
                className="list-item"
                style={{ flex: 1, minWidth: 0 }}
                aria-current={activeId === session.id ? "true" : undefined}
                onClick={() => onSelectSession(session)}
              >
                <div className="item-title">{session.title}</div>
              </button>
              <button
                type="button"
                className="btn-icon bare"
                style={{ color: "var(--text-muted)" }}
                aria-label={`actions for ${session.title}`}
                title="Chat actions"
                onClick={(e) => {
                  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                  onOpenSessionMenu(session.id, r.right - 210, r.bottom + 4);
                }}
              >
                <MoreIcon />
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
