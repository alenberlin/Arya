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
    void accountSignInState().then(setState);
    const unlisten = listen("account:signed-in", () => {
      void accountSignInState().then(setState);
    });
    return () => {
      void unlisten.then((fn) => fn());
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
