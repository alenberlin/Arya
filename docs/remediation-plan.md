# Arya Remediation Plan

**Status:** Draft for approval — 2026-07-05
**Source:** Two external AI code reviews, each finding verified file-by-file against the
current tree (see session `review-verification-2026-07`). Every item below traces to a
**CONFIRMED** or **PARTIALLY-CONFIRMED** finding. Findings that verification proved
**false or overstated** are listed in [Excluded](#excluded--verified-not-real) and are
deliberately *not* planned.

## Principles

- **Ordered by dependency and risk, not time.** Phases are units of logical sequencing —
  what must land before what. No calendar/effort estimates.
- **Each phase ships behind the gate.** A phase is done only when `make verify` is green
  *and* the phase's new behavior is covered by tests that exercise it (per the repo's
  TDD discipline). No weakening of existing assertions.
- **Scope discipline.** Each item cites `file:line` and a concrete fix. No adjacent
  refactors, no gold-plating.
- **Phases 1 and 2 are independent** (agent sandbox vs. cloud API — disjoint files) and
  may be reordered or run in parallel. Default order is security-first.

## Phase overview

| Phase | Focus | Why here |
|---|---|---|
| **0** | Guardrails & gate hardening | Make the safety net real before changing behavior; unblocks testing later phases |
| **1** | Agent trust boundary (security P0) | Largest exploitable surface; both reviews' top priority; self-contained |
| **2** | Cloud API correctness + auth/billing hardening | Cloud path is broken *now*; unblocks the product's cloud features |
| **3** | Reliability & concurrency | Stops panic-wedges, process aborts, unsupervised death, memory growth |
| **4** | Data integrity | Transactional DB/recording/RAG so failures don't corrupt state |
| **5** | Frontend integrity & accessibility | Privacy-copy honesty, optimistic-UI rollback, IPC races, WCAG/focus |
| **6** | Performance & polish + coverage backfill | Confirmed low-severity items and remaining test gaps |

---

## Phase 0 — Guardrails & gate hardening

*Goal: strengthen the net so every later phase is verifiable. Kept intentionally light.*

| # | Finding | Fix | Ref |
|---|---|---|---|
| 0.1 | R1#12 | Add `sidecar/src/**` (+ `sidecar/test/**`) to Biome `includes`, or add a `verify-sidecar` lint target. CSS is currently unlinted too — add a CSS glob or accept + document. | `biome.json:9`, `Makefile:13` |
| 0.2 | R1#13 | Broaden `scan-keys` to cover `arya-api/`, `.github/`, config JSON, `docs/`, and packaged bundles; extend patterns beyond `sk-ant`/`AKIA` (Clerk secret, `ghp_`, generic bearer). | `scripts/scan-keys.mjs:7` |
| 0.3 | R1#10 (part) | Make CI fail loudly when the release workflow references a missing file (`tauri.rc.conf.json`). Either commit the RC overlay config or remove the `--config` branch. | `.github/workflows/release-macos.yml:52` |

**Done when:** `make verify` lints the sidecar; secret scan covers the release inputs; the
release workflow no longer references a nonexistent config (or the config exists).

---

## Phase 1 — Agent trust boundary (security P0)

*Goal: close the prompt-injection → exfiltration chain. The agent is the single largest
attack surface and currently relies on user vigilance at approval time.*

| # | Finding | Fix | Ref |
|---|---|---|---|
| 1.1 | C2 | `run_command`: parse args and spawn `execFile`-style (**no shell**); **ban `"always"`** scope for it; add a per-session, user-editable allowlist; truncate/sanitize the approval description (currently raw `` `Run: ${command}` `` — Unicode-trojan risk). Fix the false "always persists via the shell" docstring. | `sidecar/src/tools.ts:152`, `approvals.ts:31,5` |
| 1.2 | C3 / R1#1 | Confine `read_file` **and** `list_dir` to the workspace by default; require explicit approval to escalate to an outside path/directory. Reuse the existing hardened `resolveInWorkspace` (realpath/symlink checks) that `write_file` already uses. Add abs-path + symlink denial tests. | `sidecar/src/paths.ts:60`, `tools.ts:57` |
| 1.3 | C1 | Build an **explicit allowlist env** for spawned MCP servers (PATH, HOME, locale); strip every `*_KEY`/`*_TOKEN`/`*_SECRET` — including `ARYA_API_TOKEN` (the always-present bearer). Document the threat model. | `sidecar/src/mcp.ts:35`, `providers.ts:10` |
| 1.4 | M14 | Escape the workspace path interpolated into the Seatbelt profile (`canonical.display()` unescaped can break the policy literal). Document that the profile is write-scoped, not a network jail. **Locked: keep network in the profile** — exfil is controlled via 1.1 (no-shell `run_command` + allowlist) and 1.2 (read confinement), since Seatbelt can't express per-subprocess network and the sidecar needs network for the proxy/Ollama. | `src-tauri/src/agent/sidecar.rs:57,63` |
| 1.5 | C4 / R1#7 | Set a strict CSP (`default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'`). Split capabilities per window — the HUD overlay does not need updater/process/autostart/dialog. | `src-tauri/tauri.conf.json:41`, `capabilities/default.json:5-15` |

**Done when:** adversarial tests pass — symlink/abs-path read denial, MCP env scrub (no
secret reaches a child), `run_command` rejects `"always"` and runs without a shell; CSP is
present and the app still functions; HUD window has a minimal capability set.

---

## Phase 2 — Cloud API correctness + auth/billing hardening

*Goal: unbreak the cloud path (it silently falls back to local/untranslated today) and
harden the proxy/auth for any non-loopback deployment. Same crate/files → done together.*

### 2a — Contract correctness (unbreaks cloud)

| # | Finding | Fix | Ref |
|---|---|---|---|
| 2.1 | R1#2 | Make the proxy honor the OpenAI-compatible contract its clients expect: **pass SSE through** (don't buffer with `response.json()`), and drop the `{success,data,meta}` envelope on the LLM chat endpoints (keep it only for account/billing). **Locked: SSE passthrough (option A).** | `arya-api/src/proxy.rs:158,238` |
| 2.2 | R1#6 | Cloud translation double-fix: `translate.rs` must send the **bare** model id (proxy re-qualifies → `anthropic:anthropic:…` → `400 model_not_priced` today) and parse the *actual* response shape; surface cloud-translation failure instead of silent fallback. | `src-tauri/src/translate.rs:34,165`, `proxy.rs:68` |
| 2.3 | H9 | Make idempotency **opt-in** via a client-supplied `Idempotency-Key` header; stop conflating dedup with caching. Identical legit messages currently collapse **permanently** (charges row has no expiry) to `data:null` → empty bubble. | `arya-api/src/proxy.rs:79-96` |
| 2.4 | R1#5 | List cloud models from the proxy catalog (`/v1/models`) when `ARYA_API_URL` is set, not only when local provider keys are present. | `sidecar/src/index.ts:72` |
| 2.5 | R1#4 | **Locked: disable cleanly in proxy-only mode** (image gen only works with a direct `OPENAI_API_KEY`, absent in production). Remove the key-custody inconsistency now; defer the proxy-routed image endpoint to a later feature pass. | `sidecar/src/images.ts:16` |

### 2b — Auth / billing / transport hardening

| # | Finding | Fix | Ref |
|---|---|---|---|
| 2.6 | C5 | Strict mode parse (fatal on unknown `ARYA_API_MODE`, not silent Local fallback with `local-dev-token`); reject a non-loopback `bind` in Local mode; resolve-and-check the IP instead of `starts_with("127.")` (which over-accepts). | `arya-api/src/config.rs:29,60-76` |
| 2.7 | C5 (client) | Refuse to send `local-dev-token` when `ARYA_API_URL` is non-loopback. | `src-tauri/src/account/tokens.rs:16-21` |
| 2.8 | C6 | Constant-time token comparison (`subtle::ConstantTimeEq`). | `arya-api/src/auth.rs:78` |
| 2.9 | H8 | `Client::builder().timeout(…).redirect(Policy::none())`; bound the upstream **response** body size (guards OOM); the request body is already capped at 2 MiB. | `arya-api/src/lib.rs:57`, `proxy.rs:238` |
| 2.10 | H10 | JWKS: retry/backoff at boot (no `expect()` panic), background refresh with TTL, and refresh-on-unknown-`kid` so key rotation doesn't lock users out. | `arya-api/src/auth.rs:51-58` |
| 2.11 | M15 (part) | Log/surface when real usage exceeds the hold cap (`min(cap.max(1))` silently eats overage). *(The "double-charge" half of M15 is false — excluded.)* | `arya-api/src/metering.rs:167` |
| 2.12 | M9 | Set a read timeout on the accepted sign-in callback stream (a silent local client can wedge sign-in for the 300 s window). | `src-tauri/src/account/signin_flow.rs:70` |
| 2.13 | M8 | Build the billing URL with `url::Url::parse_with_params` instead of unencoded `format!`. | `src-tauri/src/account/commands.rs:85` |

**Done when:** a fake-SSE upstream integration test proves the sidecar streams end-to-end
through the proxy; a Clerk/JWKS fixture test covers verify + rotation; cloud translation
round-trips (or fails loudly); unknown `ARYA_API_MODE` refuses to boot; non-loopback bind
in Local mode is rejected.

---

## Phase 3 — Reliability & concurrency

*Goal: no permanent wedges, no process aborts, no unsupervised death, no unbounded growth.*

| # | Finding | Fix | Ref |
|---|---|---|---|
| 3.1 | H3 | Drop-guard (or `catch_unwind`) that always resets `busy` — a panicked pipeline currently bricks dictation until restart. | `src-tauri/src/dictation/service.rs:368,382` |
| 3.2 | H5 | Adopt `parking_lot::Mutex` (no poisoning) for FFI/Drop-critical locks; add `// SAFETY:` to the sherpa `unsafe impl Send/Sync`. The `Drop` `.expect()` on a poisoned lock aborts the process today. *(whisper.rs already handles poison — not in scope.)* | `src-tauri/src/speech/streaming.rs:57,189` |
| 3.3 | H7 | Sidecar supervision: global `unhandledRejection`/`uncaughtException` handlers; `session.end` + cancel-and-replace on `session.start`; approval TTL with a timeout event; Rust `Drop` does `kill` **+ `wait`**; supervise/restart the child with a death callback to the UI. | `sidecar/src/index.ts`, `session.ts`, `approvals.ts`, `agent/sidecar.rs:217` |
| 3.4 | M16 | Remove the `pending` map entry on request timeout (currently leaks entry + sender). | `src-tauri/src/agent/sidecar.rs:198-209` |
| 3.5 | M17 | Cap/evict the per-session turn accumulators; clear on session delete. | `src-tauri/src/agent/mod.rs:206-233` |
| 3.6 | H6 | Wrap the poller/scheduler loop bodies in `catch_unwind` with backoff; gate the `LEVEL_TICKER` on `is_recording()` (it emits at 20 Hz forever once started). | `src-tauri/src/lib.rs:212-259`, `dictation/service.rs:590` |
| 3.7 | H1 | Realtime audio: pre-allocated SPSC ring buffer with drop-oldest; no allocation or lock in the cpal callback. | `src-tauri/src/audio/mod.rs:159-164`, `recording/recorder.rs:311` |
| 3.8 | H2 | `finish()` on the hotkey thread should only flip state + emit `Processing`; move `worker.stop()`/downmix/resample (and the device-open in `begin`) onto the pipeline thread. | `src-tauri/src/dictation/service.rs:352`, `hotkey.rs:47` |

**Done when:** a panic-injection test proves `busy` resets and dictation recovers; the
sidecar survives a bad session (handler test); Drop no longer panics on a poisoned lock;
timeout no longer leaks pending entries.

---

## Phase 4 — Data integrity

*Goal: a failed write leaves recoverable state, never orphans or an empty index.*

| # | Finding | Fix | Ref |
|---|---|---|---|
| 4.1 | R1#8 | Insert `recording_sessions` + `audio_artifacts` rows in a `starting` state **inside a transaction**, then start capture, then mark `recording`. Today capture + the WAV file precede the rows → orphan on DB failure. | `src-tauri/src/recording/commands.rs:73` |
| 4.2 | R1#9 / M1 | RAG reindex: build into a temp/generation table and swap on success (or wrap in a transaction). Today it `DELETE`s all chunks then re-embeds in a loop → a mid-run Ollama failure empties search. | `src-tauri/src/rag/commands.rs:88` |
| 4.3 | M5 | Set `busy_timeout` on the pool (and consider `max_connections(1)`, matching the "single writer" doc). Write contention currently surfaces as `SQLITE_BUSY` with no retry. | `src-tauri/src/db.rs:23-28` |
| 4.4 | M7 | `delete_note`: `DELETE` the row first (cascade handles children), then best-effort file cleanup; stop swallowing removal errors with `let _ =`. | `src-tauri/src/notes.rs:196-220` |
| 4.5 | M6 | Move `std::fs` create/copy/metadata in the async attachment commands to `spawn_blocking`. | `src-tauri/src/attachments.rs:40,45,46` |

**Done when:** failure-injection tests (extend the existing crash-recovery regression suite)
prove no orphaned recording on DB failure and a non-empty index after a mid-reindex embed
error.

---

## Phase 5 — Frontend integrity & accessibility

*Goal: the UI never lies about privacy, never diverges from disk, and is keyboard/AA-usable.*

| # | Finding | Fix | Ref |
|---|---|---|---|
| 5.1 | R1#11 | Gate the "Running locally — nothing leaves your Mac" footer on `modelPrivacy(model)` (the value is already computed for the sidebar badge). | `src/agent/AgentPanel.tsx:603` |
| 5.2 | M2 | Optimistic saves must capture the previous value and roll back on backend failure. | `src/dictation/DictationPanel.tsx:152`, `src/notes/NotesWorkspace.tsx:342` |
| 5.3 | M3 (part) | Add request-token/id gating to `NotesWorkspace.openNote` so a slow response can't overwrite the wrong note. *(AgentPanel/RoutinesPanel cases were overstated — a cheap mounted-guard only.)* | `src/notes/NotesWorkspace.tsx:189-199` |
| 5.4 | M4 | Single shared account store (Context) + an `account:signed-out` listener in `App.tsx`, so the sidebar reflects sign-out without a reload. | `src/App.tsx:82`, `account/AccountPanel.tsx:63`, `AccountGate.tsx:13` |
| 5.5 | M18 (part) | Darken `--text-muted` (#9e9585 ≈ 2.8:1 on surface — fails AA) in both themes. *(`--text-secondary` passes at 5.38:1 — excluded.)* | `src/styles/tokens.css:78` |
| 5.6 | M19 (part) | Add a focus trap + focus return to the modal. *(`role="dialog"` and initial `autoFocus` already exist — those parts of the claim are false.)* | `src/ui/dialogs.tsx:18-41` |

**Done when:** component tests cover the privacy-copy switch, optimistic rollback, and the
openNote race; the modal traps Tab and restores focus on close.

---

## Phase 6 — Performance & polish + coverage backfill

*Goal: the confirmed low-severity items and the remaining test gaps.*

- **Perf (all CONFIRMED):** cache the resampler keyed on rates (`audio/resample.rs:37`);
  share `reqwest` clients via `OnceLock` in translate/cleanup/generate, matching
  `rag/embed.rs` (`translate.rs:84,145`, `cleanup/ollama.rs:30`, `recording/generate.rs:104`);
  double-checked-locking recheck in `recording/diarize.rs` *(streaming.rs is fine —
  excluded)*; `i16` normalization `/32768.0` (`audio/mod.rs:136`).
- **Tier-4 polish (CONFIRMED):** sign-in error logging; `EKEventStore` reuse in the calendar
  poller; `meeting_detect` query-size-then-fetch; processing model reuse vs. hardcoded
  `whisper-base.en`; `enroll` `created_at`; `discard_recording` dir cleanup; empty-SHA
  acceptance; `SearchPanel` key collision; `AgentPanel` `local-${Date.now()}` id;
  `HudApp` peak-hold reset; `Onboarding` step clear; `aria-current` idiom *(spec-legal, so
  cosmetic)*; vite `manualChunks`; prune unused `sqlx` `migrate` feature; `Config`
  `Debug`-skip key fields.
- **Coverage backfill (R1#14):** tests for the sidecar tool surface (`tools.ts`, `mcp.ts`,
  `images.ts`, dispatch); Clerk/JWKS path; updater/release-workflow validation; a real
  sidecar↔proxy compatibility test. (The "cloud proxy *streaming* untested" item is a
  design gap addressed in 2.1, not a coverage gap.)

**Done when:** `make verify` green; the newly-covered flows have tests; no remaining
CONFIRMED item outstanding.

---

## Excluded — verified not real

These were checked against source and are **false, inverted, or materially overstated**.
Not planned; listed so the omission is explicit, not silent.

| Claim | Why excluded |
|---|---|
| **H4** `set_var` "unsafe / won't compile" | Crate is edition 2021 → `set_var` is a safe call, compiles clean on rustc 1.94; single-threaded at that point. |
| **M10** TOCTOU in `streaming.rs::cached` | Inverted — it holds the lock across the load; no TOCTOU. Only `recording/diarize.rs` matches (benign) → folded into 6. |
| **M15** metering "double-charge" | False — charges are idempotent on a PRIMARY KEY; one hold is never shared across two keys. Only the cost-leak clamp (2.11) is real. |
| **M18** `--text-secondary` "borderline 4.6:1" | False — actually 5.38:1 (light) / 6.63:1 (dark); passes AA. Only `--text-muted` fails (5.5). |
| **M19** "no `role=\"dialog\"`" | False — present; initial focus handled via `autoFocus`. Only focus-trap/return are real (5.6). |
| **M3** AgentPanel/RoutinesPanel "setState-after-unmount" | Overstated — listeners are torn down; RoutinesPanel writes identical data. Only `openNote` is a real race (5.3). |
| **C1** "leaks ANTHROPIC/OPENAI keys" | The always-present secret is `ARYA_API_TOKEN`; provider keys live server-side (still scrubbed in 1.3). |
| **R1#4/R1#5** "leaks in production" | They fail *closed* in proxy-only mode (feature disabled). Still fixed as correctness (2.4/2.5), not as leaks. |
| **H8** redirect key-leak severity | Mitigated — provider hosts are hardcoded; still hardened in 2.9. |
| **R1#14** "cloud proxy streaming untested" | The proxy doesn't stream at all — a design gap (2.1), not a coverage gap. |

---

## Decisions (locked 2026-07-05)

1. **Proxy contract approach (2.1)** → **A: SSE passthrough.** Make the proxy
   OpenAI-compatible; clients stay unchanged.
2. **Agent network egress (1.4)** → **A: keep network, harden `run_command` + confine
   reads.** Seatbelt stays write-scoped; exfil is closed at the tool layer.
3. **Scope depth (Phase 6)** → **Everything, including the cosmetic Tier-4 tail** and the
   coverage backfill.
4. **Image generation (2.5)** → **Disable cleanly in proxy-only mode**; defer the
   proxy-routed image endpoint to a later feature pass.
