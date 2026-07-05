import { realpathSync } from "node:fs";
import { dirname, isAbsolute, normalize, resolve, sep } from "node:path";

/** Lexical containment check (no symlink resolution). */
function isLexicallyInside(base: string, target: string): boolean {
  return target === base || target.startsWith(base + sep);
}

/**
 * Resolves a tool-supplied path against the workspace and proves it stays
 * inside — including after symlink resolution, so an in-workspace symlink
 * can't redirect a write outside. The Seatbelt jail is the hard boundary;
 * this gives the model a clean error instead of EPERM and closes the symlink
 * gap the jail alone wouldn't (for the paths it does permit).
 */
export function resolveInWorkspace(workspace: string, requested: string): string {
  const base = realBase(workspace);
  const target = isAbsolute(requested) ? normalize(requested) : resolve(base, requested);
  // Compare on realpath-normalized forms so OS-level ancestor symlinks (e.g.
  // /tmp -> /private/tmp) and in-workspace symlinks are handled consistently:
  // a lexically-inside path that resolves outside is rejected.
  const realTarget = realpathOfNearest(target);
  if (!isLexicallyInside(base, realTarget)) {
    throw new Error(`path escapes the workspace: ${requested}`);
  }
  return target;
}

function realBase(workspace: string): string {
  // Resolve through the same nearest-existing-ancestor logic as targets, so
  // an OS-level symlink on an ancestor (e.g. /tmp -> /private/tmp on macOS)
  // normalizes both sides consistently.
  return realpathOfNearest(resolve(workspace));
}

/** realpath of `target`, or of its nearest existing ancestor when the leaf
 * doesn't exist yet (a write creating a new file). */
function realpathOfNearest(target: string): string {
  let current = target;
  // Walk up until an existing path is found.
  for (let i = 0; i < 64; i++) {
    try {
      const real = realpathSync(current);
      // Re-append the not-yet-existing tail relative to the resolved ancestor.
      if (current === target) return real;
      const tail = target.slice(current.length);
      return real + tail;
    } catch {
      const parent = dirname(current);
      if (parent === current) break;
      current = parent;
    }
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

/**
 * Classifies a read/list path: resolves it (relative → workspace) and reports
 * whether it stays inside the workspace after symlink resolution. Callers gate
 * out-of-workspace reads behind explicit user approval instead of reading
 * silently — reads are the prompt-injection exfiltration vector the Seatbelt
 * jail (write-only) does not cover.
 */
export function classifyReadable(
  workspace: string,
  requested: string,
): { target: string; inside: boolean } {
  const base = realBase(workspace);
  const target = isAbsolute(requested) ? normalize(requested) : resolve(base, requested);
  const realTarget = realpathOfNearest(target);
  return { target, inside: isLexicallyInside(base, realTarget) };
}
