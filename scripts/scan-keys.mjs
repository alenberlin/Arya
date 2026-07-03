// Fails if any provider API key literal appears in the desktop sources.
// The desktop app must never carry provider keys; Arya API holds them.
// Run in CI before packaging (M14).
import { execSync } from "node:child_process";

const KEY_PATTERNS = [
  "sk-ant-[A-Za-z0-9-]{20,}",
  "sk-[A-Za-z0-9]{32,}",
  "AKIA[0-9A-Z]{16}",
];
const SCAN_DIRS = ["src", "src-tauri/src", "sidecar/src"];

let failed = false;
for (const pattern of KEY_PATTERNS) {
  try {
    const out = execSync(`grep -rEn "${pattern}" ${SCAN_DIRS.join(" ")}`, {
      encoding: "utf8",
    }).trim();
    if (out) {
      console.error(`FAIL: possible embedded key (pattern ${pattern}):\n${out}`);
      failed = true;
    }
  } catch {
    // grep exits non-zero when there are no matches — that's the pass case.
  }
}

if (failed) {
  process.exit(1);
}
console.log("scan-keys: no embedded provider keys in desktop sources");
