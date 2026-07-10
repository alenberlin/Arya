import { CheckIcon, LockIcon, McpIcon, ShieldIcon } from "../ui/icons";

/** One line of the security posture: an icon, a claim, and how it's enforced. */
interface Posture {
  icon: (p: { className?: string }) => React.JSX.Element;
  title: string;
  body: string;
}

/**
 * Each item states a guarantee the agent runtime actually enforces, in plain
 * language, so the posture is legible without reading the code:
 *  - sandbox → the macOS Seatbelt profile in agent/sidecar.rs (writes confined
 *    to the workspace; default write-mode is "sandboxed")
 *  - approvals → the ApprovalBroker (once / session / deny; unanswered → denied)
 *  - on-device → local models never leave the machine; cloud is marked
 *  - MCP → external tools launch only after confirmation and are gated through
 *    the same broker as built-in tools
 */
const POSTURE: Posture[] = [
  {
    icon: ShieldIcon,
    title: "Sandboxed by default",
    body: "Arya runs the agent under a macOS Seatbelt profile that confines file writes to its own workspace — a kernel boundary, not a policy. Reading or writing anywhere else has to be approved first.",
  },
  {
    icon: CheckIcon,
    title: "Every risky tool asks first",
    body: "Running a command, writing a file, or reaching outside the workspace pauses for your approval — allow once, allow for this session, or deny. Anything you don't answer is denied automatically.",
  },
  {
    icon: LockIcon,
    title: "On-device by default",
    body: "With a local model, nothing leaves your Mac: no prompts, no files, no telemetry. Cloud models are clearly marked before you send, and only what you send goes out.",
  },
  {
    icon: McpIcon,
    title: "MCP servers are opt-in",
    body: "External tool servers launch only after you confirm, and the tools they add are gated by the same approval as everything else — they still ask before they run.",
  },
];

/**
 * A read-only panel that makes Arya's local-first agent security legible (F13).
 * It describes the guarantees already enforced by the runtime; it does not
 * change any security mechanic.
 */
export function SecurityPanel() {
  return (
    <div className="screen-center">
      <div className="screen-col">
        <div className="screen-head">
          <div>
            <h1>Security</h1>
            <p>
              How Arya keeps the agent contained and on your side — on by default, nothing to
              configure.
            </p>
          </div>
        </div>

        <ul aria-label="security posture" className="plain stack" style={{ gap: 12 }}>
          {POSTURE.map(({ icon: Icon, title, body }) => (
            <li key={title} className="card security-item">
              <div className="security-icon" aria-hidden="true">
                <Icon />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="security-title">{title}</div>
                <p className="security-body">{body}</p>
              </div>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
