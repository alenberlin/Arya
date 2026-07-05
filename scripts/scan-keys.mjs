// Fails if a real secret literal appears in any tracked, shippable file.
// The desktop app must never carry provider keys — Arya API holds them
// server-side — and no signing/cloud secret may land in git history or a
// release bundle. Runs in `make verify` and in the release workflow before
// packaging. Scans the whole tracked tree via `git grep`, not a fixed dir list,
// so configs, workflows, docs, scripts, and arya-api are all covered.
import { execSync } from "node:child_process";

// High-signal secret shapes. Kept specific so legitimate config doesn't trip.
const KEY_PATTERNS = [
  "sk-ant-[A-Za-z0-9-]{20,}", // Anthropic
  "sk-(proj-)?[A-Za-z0-9]{32,}", // OpenAI
  "AKIA[0-9A-Z]{16}", // AWS access key id
  "AIza[0-9A-Za-z_-]{35}", // Google API key
  "ghp_[A-Za-z0-9]{36}", // GitHub personal access token
  "sk_(live|test)_[A-Za-z0-9]{20,}", // Stripe / Clerk-style secret key
  "xox[baprs]-[A-Za-z0-9-]{10,}", // Slack token
  "-----BEGIN [A-Z ]*PRIVATE KEY-----", // PEM private key
];

// Placeholders, examples, and the well-known local-dev token that structurally
// resemble a secret but are safe. A matched line is ignored if it hits any.
const ALLOWLIST = [
  /REPLACE_WITH/i,
  /YOUR_[A-Z0-9_]+/,
  /\b(example|placeholder|dummy|redacted|changeme)\b/i,
  /local-dev-token/,
  /sk-ant-\.\.\.|sk-\.\.\./, // truncated illustrations in docs / .env.example
  /x{6,}/i, // xxxxxx-style redactions
];

// Paths that contain the pattern strings themselves, or are binary/noise.
const EXCLUDES = [
  ":!scripts/scan-keys.mjs",
  ":!pnpm-lock.yaml",
  ":!*.png",
  ":!*.jpg",
  ":!*.jpeg",
  ":!*.gif",
  ":!*.icns",
  ":!*.ico",
  ":!*.woff",
  ":!*.woff2",
  ":!*.svg",
];

let failed = false;
for (const pattern of KEY_PATTERNS) {
  let out = "";
  try {
    // `-e` guards patterns that begin with `-` (e.g. PEM headers) from being
    // parsed as git-grep options.
    out = execSync(`git grep -nIE -e "${pattern}" -- . ${EXCLUDES.join(" ")}`, {
      encoding: "utf8",
    }).trim();
  } catch (err) {
    // git grep exits 1 when there are no matches (the pass case) and >1 on a
    // real error — never let a malformed pattern masquerade as "clean".
    if (err.status === 1) continue;
    throw err;
  }
  const hits = out.split("\n").filter((line) => line && !ALLOWLIST.some((re) => re.test(line)));
  if (hits.length > 0) {
    console.error(`FAIL: possible embedded secret (pattern ${pattern}):`);
    console.error(hits.join("\n"));
    failed = true;
  }
}

if (failed) {
  console.error(
    "\nscan-keys: remove the secret(s) above, or add a narrow allowlist entry if it is a placeholder.",
  );
  process.exit(1);
}
console.log("scan-keys: no embedded secrets in tracked files");
