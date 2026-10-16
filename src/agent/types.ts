// Local types shared across the agent UI components. Kept here (not in the
// generated bindings) because they're presentation concerns: a pending tool
// approval is rendered, and a streaming turn is mirrored live; both are
// derived from the agent event stream rather than exposed directly.

import type { ToolInfo } from "../lib/agent";

export interface PendingApproval {
  sessionId: string;
  callId: string;
  name: string;
  description: string;
}

export interface LiveTurn {
  text: string;
  reasoning: string;
  tools: ToolInfo[];
}
