import type { ApprovalDecision } from "./protocol.js";

/**
 * Approval broker: tools park here until the user decides. "once" approves a
 * single call; "session" pre-approves that exact tool scope for the rest of
 * this session; "deny" rejects. Callers that need narrow grants (e.g.
 * run_command) pass a per-target scope name like `run_command:<program>`.
 */
/** How long a tool approval may sit unanswered before it auto-denies, so a turn
 * can't wedge forever if the UI closes or a resolve message is lost. */
const APPROVAL_TTL_MS = 5 * 60_000;

interface Pending {
  resolve: (approved: boolean) => void;
  toolName: string;
  timer: ReturnType<typeof setTimeout>;
}

export class ApprovalBroker {
  private pending = new Map<string, Pending>();
  private sessionApproved = new Set<string>();

  /** Returns true if the tool may run without asking. */
  isPreApproved(toolName: string): boolean {
    return this.sessionApproved.has(toolName);
  }

  /** Parks a call until resolve() arrives, or auto-denies after `ttlMs`. */
  wait(callId: string, toolName: string, ttlMs: number = APPROVAL_TTL_MS): Promise<boolean> {
    return new Promise((resolvePromise) => {
      const timer = setTimeout(() => {
        const entry = this.pending.get(callId);
        if (entry) {
          this.pending.delete(callId);
          entry.resolve(false); // TTL elapsed → auto-deny; the turn continues
        }
      }, ttlMs);
      timer.unref?.();
      this.pending.set(callId, { resolve: resolvePromise, toolName, timer });
    });
  }

  resolve(callId: string, decision: ApprovalDecision | string): boolean {
    const entry = this.pending.get(callId);
    if (!entry) return false;
    clearTimeout(entry.timer);
    this.pending.delete(callId);
    if (decision !== "once" && decision !== "session" && decision !== "deny") {
      entry.resolve(false);
      return true;
    }
    if (decision === "session") {
      this.sessionApproved.add(entry.toolName);
    }
    entry.resolve(decision !== "deny");
    return true;
  }

  /** Deny everything still pending (session cancel/shutdown). */
  denyAll(): void {
    for (const [, entry] of this.pending) {
      clearTimeout(entry.timer);
      entry.resolve(false);
    }
    this.pending.clear();
  }
}
