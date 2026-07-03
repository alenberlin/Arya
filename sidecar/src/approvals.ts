import type { ApprovalDecision } from "./protocol.js";

/**
 * Approval broker: tools park here until the user decides. "once" approves
 * one call, "session" the tool for this session, "always" persists via the
 * shell (which stores it and replays as session grants), "deny" rejects.
 */
export class ApprovalBroker {
  private pending = new Map<
    string,
    { resolve: (approved: boolean) => void; toolName: string }
  >();
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
