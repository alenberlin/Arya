// Agent safety-model onboarding card.
//
// A short, dismissible card that surfaces the three knobs that decide what
// the agent can do on the user's Mac: the Seatbelt write-jail, the per-tool
// approval prompt, and the tool scope (the sandbox roots the sidecar sees).
// It only shows when the chosen model is "cloud" — for local models the
// entire pipeline is on-device, so the safety story is reduced to the
// approval prompt alone and is already obvious from the rest of the UI.
//
// Dismissal is per-user and persisted in localStorage. A "Show again" link
// in the card footer reopens it for users who want a refresher.

import { useEffect, useState } from "react";
import { CheckIcon, LockIcon, ShieldIcon } from "../ui/icons";

const STORAGE_KEY = "arya-agent-safety-dismissed-v1";

function readDismissed(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "true";
  } catch {
    // localStorage can throw in private modes / iframes; treat any failure
    // as "not dismissed" so the user still sees the card at least once.
    return false;
  }
}

function writeDismissed(dismissed: boolean): void {
  try {
    if (dismissed) {
      localStorage.setItem(STORAGE_KEY, "true");
    } else {
      localStorage.removeItem(STORAGE_KEY);
    }
  } catch {
    // best-effort; a failed write just means the card shows again next launch
  }
}

export interface SafetyCardProps {
  /** "cloud" => full card; "local" => condensed one-line version. */
  tier: "local" | "cloud";
}

/** Renders the safety card when it's relevant. Returns null once dismissed. */
export function SafetyCard({ tier }: SafetyCardProps) {
  // Start optimistic: assume the card has been dismissed so it doesn't flash
  // for users on subsequent visits. The effect below will reveal it on the
  // very first render of the panel when the user has never seen it.
  const [open, setOpen] = useState<boolean>(false);

  useEffect(() => {
    // Defer the read off the first paint to avoid a synchronous localStorage
    // hit during the initial commit, which would also re-render children.
    if (!readDismissed()) setOpen(true);
  }, []);

  if (!open) {
    return (
      <button
        type="button"
        className="safety-card-reopen"
        onClick={() => setOpen(true)}
        aria-label="Show agent safety model"
        style={{
          alignSelf: "flex-start",
          margin: "8px 22px 0",
          background: "transparent",
          border: "none",
          color: "var(--text-muted)",
          fontSize: 12,
          cursor: "pointer",
          padding: 0,
        }}
      >
        How is the agent sandboxed?
      </button>
    );
  }

  return (
    <div
      className="safety-card"
      role="region"
      aria-label="Agent safety model"
      style={{
        margin: "12px 22px 0",
        border: "1px solid var(--accent-soft)",
        borderRadius: 14,
        background: "var(--accent-ghost)",
        padding: "14px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
        <div
          aria-hidden="true"
          style={{
            flex: "0 0 auto",
            width: 30,
            height: 30,
            borderRadius: 9,
            background: "var(--surface)",
            border: "1px solid var(--accent-soft)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: "var(--accent)",
          }}
        >
          <ShieldIcon />
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13.5, fontWeight: 600, marginBottom: 2 }}>
            How the agent is sandboxed
          </div>
          <div style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.5 }}>
            {tier === "cloud"
              ? "Your text leaves this Mac to reach the cloud model. Three guards decide what it can do:"
              : "Everything runs on this Mac. Two guards decide what the agent can touch:"}
          </div>
        </div>
      </div>

      <ul
        className="safety-card-list"
        style={{
          listStyle: "none",
          margin: 0,
          padding: 0,
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <SafetyItem
          icon={<LockIcon />}
          title="Seatbelt write-jail"
          body="The sidecar runs under a macOS sandbox profile that whitelists the agent workspace and your notes folder. Anything outside is denied at the kernel level — no prompt, no override."
        />
        <SafetyItem
          icon={<CheckIcon />}
          title="Per-tool approval"
          body={`Before the agent runs a tool, you'll see a prompt like "Allow Arya to run <em>name</em>?" — approve or deny each one individually. Nothing runs without an explicit OK.`}
        />
        {tier === "cloud" ? (
          <SafetyItem
            icon={<ShieldIcon />}
            title="Scope"
            body="The agent can read your notes and the workspace folder, and propose edits to either. It cannot read other apps, your desktop, browser, or system files. Your text is sent only when a turn is in flight — no background uploads."
          />
        ) : null}
      </ul>

      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button
          type="button"
          className="btn-sm btn-primary"
          onClick={() => {
            writeDismissed(true);
            setOpen(false);
          }}
        >
          Got it
        </button>
        <button
          type="button"
          className="btn-sm"
          onClick={() => {
            writeDismissed(false);
            setOpen(false);
          }}
        >
          Don't show again
        </button>
        <span
          aria-hidden="true"
          style={{ flex: 1 }}
        />
        <button
          type="button"
          className="btn-sm"
          onClick={() => setOpen(false)}
          aria-label="Close"
        >
          Close
        </button>
      </div>
    </div>
  );
}

function SafetyItem({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <li style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
      <div
        aria-hidden="true"
        style={{
          flex: "0 0 auto",
          width: 24,
          height: 24,
          borderRadius: 7,
          background: "var(--surface)",
          border: "1px solid var(--border)",
          color: "var(--text-secondary)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {icon}
      </div>
      <div style={{ minWidth: 0, fontSize: 12.5, lineHeight: 1.5 }}>
        <span style={{ fontWeight: 600 }}>{title}.</span>{" "}
        <span dangerouslySetInnerHTML={{ __html: body }} />
      </div>
    </li>
  );
}
