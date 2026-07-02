# PLAN — Arya v1

Milestones in build order. Ordering is logical sequencing only: riskiest and
most foundational first. Each milestone closes only when its acceptance
criteria pass, the full suite is green, and an atomic commit records it.

Repo layout (monorepo, established in M1):

```
src/            React + TypeScript UI
src-tauri/      Rust shell: audio, speech, sandboxing, SQLite, commands
sidecar/        TypeScript agent runtime (Vercel AI SDK + MCP SDK)
arya-api/       Rust (axum) backend proxy: providers, catalog, metering
docs/           ADRs, subsystem docs, this blueprint
```

---

## M1 — Walking skeleton
Scaffold + toolchain + one thin slice proving every layer.
- Tauri v2 app (React/TS/Vite), Rust shell with SQLite (sqlx) + migrations,
  Biome + tsc + Vitest, cargo fmt/clippy/test, single `verify` script; git init.
- Brand indirection: one constant drives app name, bundle id, URL scheme.
- Thin slice: UI button → Tauri command → row written to SQLite → read back →
  rendered; one test at each layer.
- **AC:** `verify` green; slice test passes; app window launches.

## M2 — Local speech engine (riskiest bet)
On-device ASR foundation used by both dictation and meetings.
- whisper.cpp (whisper-rs) integrated in the shell; quantized model manager
  (download, verify, select); evaluate Parakeet/sherpa-onnx as alternate
  backend behind the same trait; benchmark harness (accuracy vs reference
  fixtures, real-time factor on Apple Silicon).
- **AC:** reference WAVs transcribe within accuracy baseline; RTF < 0.5 on
  Apple Silicon for the default model; engine swappable behind a trait.

## M3 — Dictation pillar
- Swift dictation helper: global hotkey capture (Fn/modifier-only, push-to-talk
  + toggle, graze rejection), foreground-app paste via accessibility.
- Capture → local ASR → cleanup (local LLM via Ollama or dev cloud key) →
  paste; styles; email-app layout; custom dictionary; history; languages.
- Dictation HUD (draggable pill, live levels, error shake); mic permission flow.
- **AC:** PRD criterion 1 (incl. offline path); hotkey rebind persists.

## M4 — Recording & notes core
- Recording sessions (start/pause/resume/finish), mic capture to WAV, artifact
  validation, crash recovery (partial WAVs, disk-wins reconciliation).
- Energy-based turn detection; normalization; chunked batch transcription via
  M2; note generation (structured markdown, manual-notes merge) via dev model.
- Note editor, notes list, folders/projects, tabs; processing queue with
  per-step retry.
- **AC:** record → transcript with turns → editable note; kill -9 during
  recording → relaunch recovers to the same note; every failed step retryable
  from saved audio.

## M5 — System audio & meeting detection
- Signed out-of-process Swift helper: CoreAudio process taps, signal control,
  status-file IPC; mic+system source mode; per-source WAV + independent
  validation; system-audio TCC probe flow.
- Meeting detection (CoreAudio mic-activity polling of Zoom/Teams/browsers),
  floating prompt, meeting HUD, live transcript preview (ephemeral chunks).
- **AC:** PRD criterion 2 minus diarization; helper crash does not take down
  the app; macOS < 14.2 degrades to mic-only.

## M6 — Diarization & calendar (meeting differentiators)
- On-device diarization (sherpa-onnx or equivalent) mapping speakers within
  and across turns; voice-profile enrollment for recurring named speakers.
- EventKit calendar read: overlap matching, auto-titles, attendees,
  pre-meeting prompt, post-meeting filing.
- **AC:** PRD criteria 2 (speaker labels) and 3; diarization runs offline.

## M7 — Agent runtime core
- Sidecar (TypeScript, Vercel AI SDK): streaming loop, tool calling, provider
  adapters (Anthropic, OpenAI, Ollama/OpenAI-compatible); JSON-RPC contract to
  the shell (contract designed via api-and-interface-design).
- Shell spawns one sidecar per write-mode; Seatbelt write-jail (default) vs
  unrestricted; session persistence in shell SQLite.
- Chat UI: streaming, reasoning display, composer (slash commands, attachments),
  model picker with privacy tiers; approval gates (once/session/always/deny);
  secret prompts; mid-run steering.
- **AC:** PRD criterion 4 (chat + sandboxed multi-step task with approval);
  sandbox verified by a test that proves writes outside the jail fail.

## M8 — Agent ecosystem
- MCP client + management UI (add/remove, OAuth, health); skills packs
  (install/enable, review); workspace file browser/import/preview; session
  branching + compaction; scheduled routines (cron + run history); menu-bar
  status + agent HUD.
- **AC:** PRD criterion 4 (MCP + routine); branch-from-message forks correctly.

## M9 — Workspace RAG
- Local embedding model; sqlite-vec index over notes, transcripts, dictation
  history, sessions; incremental indexing; semantic search UI; agent context
  tool with citations.
- **AC:** PRD criterion 5, fully offline.

## M10 — Image generation
- Provider adapter (OpenAI images first), `/image` command + agent tool,
  inline previews, save to workspace.
- **AC:** prompt → image renders in chat and is saved locally.

## M11 — Arya API (backend proxy)
- axum service: Clerk JWKS verification (+ local dev token mode), provider
  adapters (Anthropic, OpenAI) behind traits, model catalog with pricing +
  privacy tiers + capability flags, unpriced-model rejection, request bounds,
  semantic error envelope, streaming pass-through.
- Metering primitives: hold → settle, idempotency keys, action slugs;
  structural anonymization (no filenames/titles/user ids upstream).
- Desktop switches cloud calls to the proxy; local models bypass it; repo
  prepared for open-sourcing.
- **AC:** PRD criterion 7; forced retry settles exactly once (test); contract
  is additive-only (documented).

## M12 — Accounts & billing
- Clerk sign-in in-app (browser flow, Keychain tokens, refresh resilience);
  Stripe tiers (Free/Pro/Max) + credit top-ups; balance/subscription snapshot;
  sign-in gate, funding gate, upgrade + top-up flows; per-session usage panel.
- **AC:** PRD criterion 6 end-to-end against Stripe test mode.

## M13 — Product shell & onboarding
- Onboarding (dictation practice, just-in-time permissions, privacy explainer,
  hotkey setup, resumable); full settings surface; theming on design tokens;
  notifications; issue reporting; empty states and polish passes per pillar.
- **AC:** a fresh user reaches all four pillars guided by the app alone
  (scripted walkthrough on a clean account/VM).

## M14 — Release engineering & hardening
- Signing + notarization, DMG build, Tauri updater with stable/rc channels and
  a public releases repo; key scan in CI (PRD criterion 7); security review
  (security-auditor persona), performance pass on dictation latency and RAG
  search, full QA sweep; REPORT.md.
- **AC:** PRD criterion 8; update from build N-1 → N succeeds on a clean Mac.

---

## Phase 2 (post-v1, not in this plan's scope)
Team & sharing (share-links, then workspaces), TEE attestation for Arya API,
Windows port, image editing, final brand decision if not yet made.

## Standing build rules
- Contracts (shell↔sidecar RPC, Arya API /v1) get api-and-interface-design
  treatment before implementation; Arya API is additive-only once M11 lands.
- Every milestone: narrowest tests first, then full `verify`; atomic commit
  with evidence recorded in PROGRESS.md.
- Local dev never requires Clerk/Stripe/provider keys until M11-M12 (dev mode
  uses local models + shared token, mirroring June's local mode).
