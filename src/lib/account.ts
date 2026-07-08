import { invoke } from "@tauri-apps/api/core";

export interface SignInState {
  signedIn: boolean;
  hostedAuth: boolean;
}

export interface AccountSnapshot {
  userId: string;
  includedCredits: number;
  usedCredits: number;
  topupCredits: number;
  remainingCredits: number;
}

export const accountSignInState = () => invoke<SignInState>("account_signin_state");
export const accountBeginSignIn = () => invoke<void>("account_begin_signin");
export const accountSignOut = () => invoke<void>("account_sign_out");
export const accountSnapshot = () => invoke<AccountSnapshot>("account_snapshot");
