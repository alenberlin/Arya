import { useState } from "react";
import { AgentIcon, McpIcon, RoutinesIcon, ShieldIcon } from "../ui/icons";
import { AgentPanel } from "./AgentPanel";
import { McpPanel } from "./McpPanel";
import { RoutinesPanel } from "./RoutinesPanel";
import { SecurityPanel } from "./SecurityPanel";

type AgentView = "chat" | "routines" | "mcp" | "security";

const VIEWS: {
  id: AgentView;
  label: string;
  icon: (p: { className?: string }) => React.JSX.Element;
}[] = [
  { id: "chat", label: "Chat", icon: AgentIcon },
  { id: "routines", label: "Routines", icon: RoutinesIcon },
  { id: "mcp", label: "MCP servers", icon: McpIcon },
  { id: "security", label: "Security", icon: ShieldIcon },
];

/**
 * The Agent hub: chat, scheduled routines, MCP servers, and the security
 * posture live here as sub-views rather than as separate sidebar pillars, so
 * the sidebar leads with the core surfaces (Notes, Dictation, …) and everything
 * agent-related is one click deep under a single "Agent" pillar (D5).
 */
export function AgentSection() {
  const [view, setView] = useState<AgentView>("chat");
  return (
    <div className="agent-section">
      <div className="tabstrip agent-tabs" role="tablist" aria-label="agent sections">
        {VIEWS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            role="tab"
            className="tab"
            aria-selected={view === id}
            onClick={() => setView(id)}
          >
            <Icon />
            {label}
          </button>
        ))}
      </div>
      <div className="agent-section-body">
        {view === "chat" ? <AgentPanel /> : null}
        {view === "routines" ? <RoutinesPanel /> : null}
        {view === "mcp" ? <McpPanel /> : null}
        {view === "security" ? <SecurityPanel /> : null}
      </div>
    </div>
  );
}
