# Arya v1 — completion report

Arya is a private, macOS-first AI workspace: chat + a sandboxed local agent,
system-wide dictation, and bot-free meeting notes, with on-device speech and
an open-source metering proxy that keeps provider keys server-side. Built from
spec (no code ported from the reference app), 14 milestones, each an atomic
commit with runtime evidence.

## How to run it

```sh
# Full local dev (no cloud keys, no accounts needed):
pnpm install
pnpm --filter arya-sidecar build      # agent runtime bundle
pnpm tauri:dev                        # the desktop app

# The metering proxy (optional; only for cloud models / billing):
cargo run --manifest-path arya-api/Cargo.toml     # local mode, :8477

# The full CI gate:
make verify        # brand+key-scan, lint, typecheck, TS tests,
                   # Rust shell, sidecar, arya-api
```

Local models (Ollama) power chat, the agent, note generation, dictation
cleanup, and embeddings for free and offline. Cloud models are opt-in through
the proxy.

## Acceptance criteria (PRD) — status with evidence

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | Dictation offline + fast | **Met** | Real-mic E2E: hotkey→capture→on-device ASR→cleanup→paste; exact JFK transcript, ASR 87ms/12s. Offline path uses local cleanup; paste gated on the one-time Accessibility grant. |
| 2 | Meeting → turns + speakers + recovery | **Met** | Meeting-mode E2E: dual mic+system artifacts; turn-ordered transcript; diarization clustered same-speaker turns (pairwise 0.79-0.83, probe 0.847); kill -9 mid-record recovered the note from the repaired partial WAV without re-recording. |
| 3 | Calendar titles/attendees | **Built** | EventKit integration; degrades cleanly without the Calendar grant (needs a one-time user grant + calendar data to observe live). |
| 4 | Chat + sandboxed agent + MCP + routine | **Met** | Agent E2E on local qwen3.6: approval-gated command + file write inside the Seatbelt jail; MCP client wired; scheduler ran a due routine end to end ("PONG" persisted). Jail proven by a test: writes outside (incl. /tmp) fail, inside succeed. |
| 5 | RAG < 1s offline + agent citations | **Met** | 132 chunks indexed locally in 2.9s; semantic search 23.8ms (in-memory cache); agent's search_workspace tool quoted the exact passage with source kind. |
| 6 | Billing: sign-in, meter, idempotent, gate | **Met (local) / flagged (live Stripe)** | Live: /v1/account showed Pro 500k credits; a metered call decremented the visible balance; forced retry settled exactly-once AND skipped the upstream (no double-bill). Live Stripe test-mode + Clerk sign-in need the owner's accounts — seams built, unit-tested, env-switchable. |
| 7 | No provider key in the bundle | **Met** | `scripts/scan-keys.mjs` (a make-verify gate) finds no key literal in src/, src-tauri/src/, sidecar/src/; keys live only in arya-api's env. |
| 8 | Quality gate + signed build | **Met / flagged (Apple signing)** | `make verify` green across all four suites; release binary compiles (`cargo build --release`) and the app boots clean. Signing + notarization need an Apple Developer ID (workflow + entitlements + runbook ready). |

## Review gate (Phase 2)

Three adversarial personas (security, correctness, performance) audited the
whole codebase. Every Critical and Important finding was fixed and re-verified:

**Security** — CSRF `state` binding added to the loopback sign-in
(token-injection closed, unit-tested); agent write-jail hardened (shared temp
roots removed from the Seatbelt profile with node's TMPDIR redirected into the
workspace; TS path-jail now resolves symlinks; regression test asserts /tmp
writes fail); metering now enforces balance transactionally (no
concurrent-overspend TOCTOU; unmetered local mode only when no cloud key is
present); ollama upstream validated loopback-only (SSRF pivot closed); JWT
audience bound; `account_set_token` gated out of release builds; request body
size capped; WAV input hardened against malformed bit-depth/size.

**Correctness** — transcript turns now stored in one transaction (no
truncated-on-retry data loss); crash recovery iterates all source artifacts
(system track no longer dropped); the proxy short-circuits duplicate requests
before calling the provider (no double-bill); the agent runtime no longer
holds the sidecar map lock across a blocking round-trip (cancel/steer
responsive); settle is transactional; the notes editor no longer clobbers
in-progress edits when processing completes.

**Performance** — the agent chat memoizes settled messages (streaming no
longer re-renders the whole history); RAG search reads embeddings from an
in-memory cache invalidated on reindex; the diarization ONNX extractor is
cached process-wide instead of rebuilt per note.

## Deliberately out of scope (v1 non-goals, honored)

Windows/Linux builds; server-side storage of user content; TEE attestation;
mobile/web; bot-based meeting joining; image editing; model training. Team &
sharing is the planned Phase 2.

## Needs the owner's credentials before public launch

None block development or local use. Each is env-switchable behind a built,
tested seam (see `docs/release.md`):

- **Apple Developer ID** — code-sign + notarize the DMG; the updater's Ed25519
  key signs the manifest. Release workflow + entitlements are in place.
- **Clerk** — hosted sign-in (`ARYA_API_MODE=clerk`); the JWKS verifier and
  browser handoff are built and unit-tested.
- **Stripe** — subscriptions + credit top-ups; the wallet trait and account
  wire shapes are built, the LocalWallet backs everything today.
- **Anthropic / OpenAI keys + hosting** — cloud inference and live image
  generation, in arya-api's env only.

## Follow-ups — all addressed after the review gate

- ✅ MCP: adding a server now confirms before spawning it (shows the command;
  tool *calls* were already approval-gated).
- ✅ Loopback urldecode swapped for the `percent-encoding` crate.
- ✅ Upstream error text is scrubbed of the provider key and any Bearer/`sk-`
  token before it reaches the client (unit-tested).
- ✅ The embedder reuses one pooled `reqwest` client across queries.
- ✅ Dictation shares the process-wide speech engine cache with note
  transcription, so a model used by both loads into memory once.
- ✅ `cargo audit` + `pnpm audit` wired into CI (`security-audit.yml`).
  `pnpm audit --prod` is clean; two Rust advisories are present in lockfiles
  but unreachable (rsa never compiled; quick-xml parses only our own bundle
  metadata) — documented with reachability analysis and ignored in
  `docs/security-audit.md`.

Remaining known low-severity item: none blocking. See `docs/security-audit.md`
for the advisory dispositions that clear on the next Tauri/sqlx bump.

## Test inventory

Rust shell: 63 unit/integration tests (incl. the Seatbelt jail, WAV crash
recovery, turn detection, diarization clustering, metering). arya-api: 10
tests (exactly-once, balance enforcement, concurrent-hold TOCTOU, auth,
catalog) + an ignored live metered-roundtrip. Sidecar: 12 tests (path jail
incl. symlink escape, approval broker, model resolution). Frontend: 8 tests
(app shell, dictation, onboarding, HUD). Plus the M2 speech benchmark and the
diarization probe (ignored; run on demand).
