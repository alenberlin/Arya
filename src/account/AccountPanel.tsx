import { useCallback, useEffect, useState } from "react";
import {
  type AccountSnapshot,
  accountOpenBilling,
  accountSignInState,
  accountSignOut,
  accountSnapshot,
  creditsToUsd,
  type SignInState,
} from "../lib/account";
import { AccountIcon, LockIcon } from "../ui/icons";

const cap = (s: string) => (s ? s.charAt(0).toUpperCase() + s.slice(1) : s);

/** On-device model privacy tiers — shown in both local and cloud modes. */
function PrivacyTiers() {
  return (
    <div className="card" style={{ marginBottom: 14 }}>
      <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 14 }}>Model privacy tiers</div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--success)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Local models</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Run on your Mac via Ollama. Nothing leaves the device.
          </div>
        </div>
        <span className="badge badge-success">On-device</span>
      </div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--success)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Whisper · speech</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Transcription &amp; dictation, fully offline.
          </div>
        </div>
        <span className="badge badge-success">On-device</span>
      </div>
      <div className="tier-row">
        <span className="tier-dot" style={{ background: "var(--warning)" }} />
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 13.5, fontWeight: 500 }}>Cloud models</div>
          <div className="muted" style={{ fontSize: 12 }}>
            Optional. Sends your prompt to the provider through the open-source proxy.
          </div>
        </div>
        <span className="badge badge-warning">Cloud</span>
      </div>
    </div>
  );
}

/** Account and billing: tier, credit balance, usage, upgrade/top-up. */
export function AccountPanel() {
  const [signIn, setSignIn] = useState<SignInState | null>(null);
  const [snapshot, setSnapshot] = useState<AccountSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const state = await accountSignInState();
      setSignIn(state);
      // Only the hosted/cloud build has an account+billing backend; local mode
      // never reaches out, so it can't (and shouldn't) error.
      if (state.hostedAuth) {
        setSnapshot(await accountSnapshot());
      }
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const openBilling = async (intent: "upgrade" | "topup" | "portal") => {
    const opened = await accountOpenBilling(intent);
    if (!opened) {
      setNotice("Hosted billing isn't configured in this build (local mode).");
    }
  };

  // Local mode: no account, nothing to fetch, no errors — everything's on-device.
  if (signIn && !signIn.hostedAuth) {
    return (
      <div className="screen-center">
        <div className="screen-col narrow">
          <h1 style={{ marginBottom: 20 }}>Account</h1>
          <div className="card hstack" style={{ gap: 16, marginBottom: 14 }}>
            <span className="avatar avatar-lg">
              <LockIcon />
            </span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 17, fontWeight: 600 }}>Local mode</div>
              <div className="muted" style={{ fontSize: 13 }}>
                Everything runs on your Mac — no account needed. Local models and on-device speech
                are always free.
              </div>
            </div>
          </div>
          <PrivacyTiers />
          <p className="muted" style={{ fontSize: 12 }}>
            Sign-in, cloud credits, and billing appear here when the app is built with the hosted
            proxy configured.
          </p>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="screen-center">
        <div className="screen-col narrow">
          <h1 style={{ marginBottom: 16 }}>Account</h1>
          <p role="alert">{error}</p>
          <button type="button" onClick={() => void refresh()}>
            Retry
          </button>
        </div>
      </div>
    );
  }

  if (!snapshot) {
    return (
      <div className="screen-center">
        <div className="screen-col narrow">
          <h1 style={{ marginBottom: 16 }}>Account</h1>
          <p>Loading account…</p>
        </div>
      </div>
    );
  }

  const total = snapshot.includedCredits + snapshot.topupCredits;
  const remainingPct =
    total > 0 ? Math.min(100, Math.round((snapshot.remainingCredits / total) * 100)) : 0;

  return (
    <div className="screen-center">
      <div className="screen-col narrow">
        <h1 style={{ marginBottom: 20 }}>Account</h1>
        {notice ? (
          <p role="status" className="muted" style={{ marginBottom: 12 }}>
            {notice}
          </p>
        ) : null}

        <div className="card hstack" style={{ gap: 16, marginBottom: 14 }}>
          <span className="avatar avatar-lg">
            <AccountIcon />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 17, fontWeight: 600 }}>{cap(snapshot.tier)} plan</div>
            <div className="muted" style={{ fontSize: 13 }}>
              {snapshot.subscribed ? "Subscribed" : "Local mode"} · on-device speech is always free
            </div>
          </div>
          <button type="button" onClick={() => void openBilling("portal")}>
            Manage plan
          </button>
        </div>

        <div className="card" style={{ marginBottom: 14 }}>
          <div className="spread" style={{ alignItems: "baseline", marginBottom: 12 }}>
            <div style={{ fontSize: 14, fontWeight: 600 }}>Cloud credits</div>
            <div className="mono muted" style={{ fontSize: 13 }}>
              {snapshot.remainingCredits.toLocaleString()} / {total.toLocaleString()}
            </div>
          </div>
          <div className="meter">
            <div
              className="meter-fill"
              style={{
                width: `${remainingPct}%`,
                background: remainingPct < 10 ? "var(--danger)" : "var(--accent)",
              }}
            />
          </div>
          <div className="muted" style={{ fontSize: 12, marginTop: 10 }}>
            {snapshot.remainingCredits.toLocaleString()} credits left (
            {creditsToUsd(snapshot.remainingCredits)}). Only used when you choose a cloud model —
            local models and on-device speech are always free.
          </div>
          <div className="hstack" style={{ marginTop: 14 }}>
            {snapshot.tier !== "max" ? (
              <button
                type="button"
                className="btn-primary"
                onClick={() => void openBilling("upgrade")}
              >
                {snapshot.tier === "free" ? "Upgrade to Pro" : "Upgrade to Max"}
              </button>
            ) : null}
            {snapshot.subscribed ? (
              <button type="button" onClick={() => void openBilling("topup")}>
                Top up credits
              </button>
            ) : null}
          </div>
        </div>

        <PrivacyTiers />

        <button
          type="button"
          className="btn-ghost"
          onClick={() => void accountSignOut().then(() => setNotice("Signed out."))}
        >
          Sign out
        </button>
      </div>
    </div>
  );
}
