#!/usr/bin/env node
// E2E runner for the dev_hooks module: spawns the local debug build with
// ARYA_DEV_* env vars and asserts the expected tracing line lands on stderr
// within a bounded timeout. The live-system counterpart to `cargo test
// --lib`: a green run here proves a hook is wired through the real Tauri
// runtime, not just compiled.
//
// Usage:
//   node scripts/e2e-dev-hooks.mjs                # all hooks that don't need
//                                                 #   hardware / network
//   node scripts/e2e-dev-hooks.mjs --only close   # one named hook
//
// Each scenario has a stable name, a list of env vars, a stderr regex that
// must appear, and a timeout (ms). A scenario passes when its regex matches
// at least once before the timeout fires; the runner then SIGTERMs the
// binary so the next scenario (or the next CI run) starts clean.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");
const BIN = resolve(ROOT, "src-tauri/target/debug/arya");

if (!existsSync(BIN)) {
  console.error(
    `missing debug binary at ${BIN}\n` +
      `run \`cargo build --manifest-path src-tauri/Cargo.toml\` first.`,
  );
  process.exit(2);
}

// All hooks below emit through the `tracing` subscriber installed in
// lib.rs::run, which writes to stderr. dev_hooks is `#[cfg(debug_assertions)]`
// so this script is only meaningful on a debug build (CI does that).
const SCENARIOS = [
  {
    name: "calendar",
    env: { ARYA_DEV_CALENDAR: "1" },
    expect: /dev calendar: access=/,
    timeoutMs: 15_000,
  },
  {
    name: "close-test",
    env: { ARYA_DEV_CLOSE_TEST: "1" },
    expect: /dev close-test: visible_after_show=Some\(true\)/,
    timeoutMs: 20_000,
  },
  {
    // ARYA_DEV_RECOVER=1 needs no leftover sessions — the scan should
    // complete and report an empty list (or any list, both are fine).
    name: "recover",
    env: { ARYA_DEV_RECOVER: "1" },
    expect: /dev recover: scan /,
    timeoutMs: 20_000,
  },
];

function parseArgs(argv) {
  let only = null;
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === "--only") only = argv[++i];
  }
  return { only };
}

async function runScenario(scenario) {
  return new Promise((resolveRun) => {
    const child = spawn(BIN, [], {
      env: { ...process.env, ...scenario.env, RUST_LOG: "info" },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let matched = false;
    let captured = "";
    const onChunk = (chunk) => {
      const text = chunk.toString();
      captured += text;
      if (!matched && scenario.expect.test(text)) {
        matched = true;
        // Give the binary a beat to log the rest of its hook output
        // (the close-test hook in particular has a trailing visible_after
        // log we want to capture before tearing down).
        setTimeout(() => child.kill("SIGTERM"), 250);
      }
    };
    child.stdout.on("data", onChunk);
    child.stderr.on("data", onChunk);

    const timer = setTimeout(() => {
      if (!matched) child.kill("SIGTERM");
    }, scenario.timeoutMs);

    child.on("exit", (code, signal) => {
      clearTimeout(timer);
      resolveRun({
        name: scenario.name,
        matched,
        code,
        signal,
        captured: captured.slice(-2_000),
      });
    });
  });
}

function color(s, c) {
  if (!process.stdout.isTTY) return s;
  return `\x1b[${c}m${s}\x1b[0m`;
}

async function main() {
  const { only } = parseArgs(process.argv);
  const scenarios = only
    ? SCENARIOS.filter((s) => s.name === only)
    : SCENARIOS;
  if (only && scenarios.length === 0) {
    console.error(
      `unknown scenario: ${only}\n` +
        `known: ${SCENARIOS.map((s) => s.name).join(", ")}`,
    );
    process.exit(2);
  }
  const results = [];
  for (const s of scenarios) {
    process.stdout.write(`-> ${s.name}: `);
    const r = await runScenario(s);
    if (r.matched) {
      process.stdout.write(color("PASS", 32) + "\n");
    } else {
      process.stdout.write(
        color("FAIL", 31) + ` (timeout waiting for ${s.expect})\n`,
      );
    }
    if (!r.matched) {
      // Surface the last bit of stderr so the failure is debuggable from
      // CI logs without re-running locally.
      process.stdout.write("---\n" + r.captured + "---\n");
    }
    results.push(r);
  }
  const failed = results.filter((r) => !r.matched);
  if (failed.length) {
    console.error(color(`${failed.length} scenario(s) failed`, 31));
    process.exit(1);
  }
  console.log(color(`${results.length} scenario(s) passed`, 32));
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});