import { invoke } from "@tauri-apps/api/core";

export interface SignInState {
  signedIn: boolean;
  hostedAuth: boolean;
}

export interface AccountSnapshot {
  userId: string;
  tier: string;
  includedCredits: number;
  usedCredits: number;
  topupCredits: number;
  remainingCredits: number;
  subscribed: boolean;
}

export const accountSignInState = () => invoke<SignInState>("account_signin_state");
export const accountBeginSignIn = () => invoke<void>("account_begin_signin");
export const accountSignOut = () => invoke<void>("account_sign_out");
export const accountSnapshot = () => invoke<AccountSnapshot>("account_snapshot");
export const accountOpenBilling = (intent: "upgrade" | "topup" | "portal") =>
  invoke<boolean>("account_open_billing", { target: intent });

/** $1 = 1000 credits. */
export function creditsToUsd(credits: number): string {
  return `$${(credits / 1000).toFixed(2)}`;
}
