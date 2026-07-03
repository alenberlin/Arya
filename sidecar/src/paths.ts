import { isAbsolute, normalize, resolve, sep } from "node:path";

/**
 * Resolves a tool-supplied path against the workspace and proves it stays
 * inside. The Seatbelt jail is the hard boundary; this is the polite first
 * line that gives the model a clean error instead of EPERM.
 */
export function resolveInWorkspace(workspace: string, requested: string): string {
  const base = resolve(workspace);
  const target = isAbsolute(requested) ? normalize(requested) : resolve(base, requested);
  if (target !== base && !target.startsWith(base + sep)) {
    throw new Error(`path escapes the workspace: ${requested}`);
  }
  return target;
}

/** True when the path may be read: anywhere under workspace, or read-only
 * system locations the model legitimately needs. Reads outside workspace are
 * allowed (matching the jail, which only blocks writes). */
export function resolveReadable(workspace: string, requested: string): string {
  const base = resolve(workspace);
  return isAbsolute(requested) ? normalize(requested) : resolve(base, requested);
}
