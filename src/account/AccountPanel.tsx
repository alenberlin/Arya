import { useCallback, useEffect, useState } from "react";
import {
  type AccountSnapshot,
  accountOpenBilling,
  accountSignOut,
  accountSnapshot,
  creditsToUsd,
} from "../lib/account";

/** Account and billing: tier, credit balance, usage, upgrade/top-up. */
export function AccountPanel() {
  const [snapshot, setSnapshot] = useState<AccountSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSnapshot(await accountSnapshot());
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

  if (error) {
    return (
      <section>
        <h2>Account</h2>
        <p role="alert">{error}</p>
        <button type="button" onClick={() => void refresh()}>
          Retry
        </button>
      </section>
    );
  }

  if (!snapshot) {
    return (
      <section>
        <h2>Account</h2>
        <p>Loading account…</p>
      </section>
    );
  }

  const usedPct =
    snapshot.includedCredits + snapshot.topupCredits > 0
      ? Math.min(
          100,
          Math.round(
            (snapshot.usedCredits / (snapshot.includedCredits + snapshot.topupCredits)) * 100,
          ),
        )
      : 0;

  return (
    <section>
      <h2>Account</h2>
      {notice ? <p role="status">{notice}</p> : null}
      <div style={{ maxWidth: 480 }}>
        <p>
          Plan: <strong>{snapshot.tier}</strong>
          {snapshot.subscribed ? " · subscribed" : ""}
        </p>
        <div style={{ margin: "8px 0" }}>
          <div style={{ height: 10, background: "#e5e7eb", borderRadius: 5 }}>
            <div
              style={{
                width: `${usedPct}%`,
                height: "100%",
                background: usedPct > 90 ? "#dc2626" : "#2563eb",
                borderRadius: 5,
              }}
            />
          </div>
          <small>
            {snapshot.remainingCredits.toLocaleString()} credits left (
            {creditsToUsd(snapshot.remainingCredits)}) · {snapshot.usedCredits.toLocaleString()}{" "}
            used this cycle
          </small>
        </div>
        <p>
          <small>
            Included {snapshot.includedCredits.toLocaleString()}
            {snapshot.topupCredits > 0 ? ` + ${snapshot.topupCredits.toLocaleString()} top-up` : ""}
            . Local models and on-device speech are always free.
          </small>
        </p>
        <div style={{ display: "flex", gap: 8 }}>
          {snapshot.tier !== "max" ? (
            <button type="button" onClick={() => void openBilling("upgrade")}>
              {snapshot.tier === "free" ? "Upgrade to Pro" : "Upgrade to Max"}
            </button>
          ) : null}
          {snapshot.subscribed ? (
            <button type="button" onClick={() => void openBilling("topup")}>
              Top up credits
            </button>
          ) : null}
          <button type="button" onClick={() => void openBilling("portal")}>
            Manage billing
          </button>
          <button
            type="button"
            onClick={() => void accountSignOut().then(() => setNotice("Signed out."))}
          >
            Sign out
          </button>
        </div>
      </div>
    </section>
  );
}
