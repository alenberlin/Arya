//! Thin adapter over the generated typed command bindings (`bindings.ts`).
//!
//! `bindings.ts` commands return a `{ status: "ok" | "error" }` union so errors
//! are explicit; the app's existing wrappers expect `Promise<T>` that rejects on
//! error. `run` converts between the two. The generated types are looser than the
//! hand-written domain types in this folder (e.g. `sourceKind: string` vs a
//! `NodeKind` union), so `run` casts the payload to the caller's `T`.

export { commands } from "./bindings";

type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: unknown };

/** Await a generated command and return its payload as `T`, throwing on error. */
export async function run<T = unknown>(p: Promise<CommandResult<unknown>>): Promise<T> {
  const v = await p;
  if (v.status === "ok") return v.data as T;
  throw v.error;
}
