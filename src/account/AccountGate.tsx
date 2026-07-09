import { listen } from "@tauri-apps/api/event";
import { type ReactNode, useEffect, useState } from "react";
import { accountBeginSignIn, accountSignInState, type SignInState } from "../lib/account";

/**
 * Sign-in wall. In local/open-source mode (no hosted auth) it never blocks;
 * with Clerk configured it shows a sign-in screen until a session exists.
 */
export function AccountGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<SignInState | null>(null);

  useEffect(() => {
    const refresh = () => void accountSignInState().then(setState);
    refresh();
    // Re-check on BOTH transitions: sign-in unlocks the app, and sign-out must
    // re-raise the wall without a reload (it previously only listened for
    // signed-in, so the gate stayed unlocked after sign-out).
    const unlistenIn = listen("account:signed-in", refresh);
    const unlistenOut = listen("account:signed-out", refresh);
    return () => {
      void unlistenIn.then((fn) => fn());
      void unlistenOut.then((fn) => fn());
    };
  }, []);

  if (!state) {
    return <p style={{ padding: "2rem" }}>Loading…</p>;
  }
  if (state.signedIn || !state.hostedAuth) {
    return <>{children}</>;
  }

  return (
    <main style={{ fontFamily: "system-ui", padding: "3rem", textAlign: "center" }}>
      <h1>Welcome to Arya</h1>
      <p>Private AI on your Mac. Sign in to sync your plan and credits.</p>
      <button type="button" onClick={() => void accountBeginSignIn()}>
        Sign in
      </button>
    </main>
  );
}
