import type { ApprovalDecision } from "./protocol.js";

/**
 * Approval broker: tools park here until the user decides. "once" approves a
 * single call; "session" (and "always") pre-approve that exact tool scope for
 * the rest of this session; "deny" rejects. There is no cross-session
 * persistence layer, so "always" currently behaves exactly like "session" —
 * a durable shell grant would be unsafe. Callers that need narrow grants
 * (e.g. run_command) pass a per-target scope name like `run_command:<program>`.
 */
export class ApprovalBroker {
  private pending = new Map<string, { resolve: (approved: boolean) => void; toolName: string }>();
  private sessionApproved = new Set<string>();

  /** Returns true if the tool may run without asking. */
  isPreApproved(toolName: string): boolean {
    return this.sessionApproved.has(toolName);
  }

  /** Parks a call until resolve() arrives. */
  wait(callId: string, toolName: string): Promise<boolean> {
    return new Promise((resolvePromise) => {
      this.pending.set(callId, { resolve: resolvePromise, toolName });
    });
  }

  resolve(callId: string, decision: ApprovalDecision): boolean {
    const entry = this.pending.get(callId);
    if (!entry) return false;
    this.pending.delete(callId);
    // "session" and "always" both pre-approve this scope for the session only;
    // no durable/cross-session grant exists (durable shell grants are unsafe).
    if (decision === "session" || decision === "always") {
      this.sessionApproved.add(entry.toolName);
    }
    entry.resolve(decision !== "deny");
    return true;
  }

  grantSession(toolName: string): void {
    this.sessionApproved.add(toolName);
  }

  /** Deny everything still pending (session cancel/shutdown). */
  denyAll(): void {
    for (const [, entry] of this.pending) {
      entry.resolve(false);
    }
    this.pending.clear();
  }
}
