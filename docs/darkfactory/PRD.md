# PRD — Arya (codename)

> Blueprint for a commercial, privacy-first AI workspace for macOS. Feature target:
> full parity with June (github.com/open-software-network/os-june, MIT) plus five
> differentiators. This is a from-spec rewrite with our own architecture — no code
> ported from June.

## Product statement

Arya is a native macOS app that replaces an AI chat assistant, a system-wide
dictation tool, and a meeting notetaker with one private workspace, plus a
sandboxed local AI agent that works across all of it. **Audio never leaves the
Mac**: transcription, dictation, and diarization run on-device. Cloud LLMs are
optional, routed through an open-source proxy that holds the provider keys;
local LLMs are a free, first-class alternative.

## Users & success

- **Users:** privacy-conscious professionals on Apple Silicon Macs who live in
  meetings and documents — the Granola/Wispr/ChatGPT-desktop audience.
- **Commercial model:** Free / Pro / Max subscriptions with included monthly
  cloud-AI usage plus top-up credits (Stripe). Everything local (speech,
  diarization, local LLMs, notes, search) is free forever.
- **Success for v1:** a signed, notarized, auto-updating app a stranger can
  install and use through all four pillars without touching a config file, with
  billing that meters cloud usage correctly.

## Decisions (settled in discovery)

| Area | Decision |
|---|---|
| Audience | Commercial product |
| Platform | macOS 14+ (Apple Silicon primary), macOS-only for v1 |
| Stack | Tauri v2 + React/TypeScript UI + Rust native shell |
| Inference | Hybrid local-first: on-device ASR/diarization; LLMs cloud **or** local |
| Cloud LLMs | Direct providers — Anthropic + OpenAI first, extensible provider registry (no aggregator) |
| Local LLMs | Ollama + any OpenAI-compatible endpoint, surfaced in the model picker as a free/local tier |
| Agent | Our own model-agnostic runtime: TypeScript sidecar (Vercel AI SDK + official MCP SDK), spawned and Seatbelt-sandboxed by the Rust shell |
| Identity/billing | Clerk (auth/JWT) + Stripe (subscriptions, credits, metering) |
| Backend | "Arya API": open-source Rust (axum) proxy holding provider keys, verifying Clerk JWTs, metering usage; local dev mode with shared token |
| Privacy story | Local-first + open source at launch; TEE attestation is a later phase |
| Name | Codename **Arya**; brand kept swappable (single source for name / bundle id / URL scheme) |
| Storage | Local SQLite (sqlx) owned by the Rust shell; no cloud storage of user content in v1 |

## Pillars & feature scope (v1)

### 1. Dictation (system-wide)
Global push-to-talk + hands-free toggle (customizable hotkeys incl. Fn/modifier-only,
graze rejection); on-device ASR; LLM cleanup that preserves wording (styles:
standard / casual / formal); paste into the foreground app via accessibility;
email-app-aware layout; multi-language; custom dictionary; dictation history;
draggable HUD pill with live levels and error states. **Works fully offline**
when a local cleanup model is configured.

### 2. Meeting notes & recording
Bot-free capture: meeting detection via CoreAudio mic-activity polling (Zoom,
Teams, Meet/browsers) with a floating prompt; mic-only or mic+system source
modes; system audio via a signed out-of-process Swift helper (CoreAudio process
taps); one WAV per source; energy-based turn ordering; **on-device speaker
diarization with voice profiles** (differentiator); optional ephemeral live
transcript preview; batch on-device transcription → structured AI note
generation (editable, incorporates manual notes); saved-audio-first reliability
(every step retryable, crash recovery from partial WAVs); pause/resume,
auto-pause, meeting HUD; **calendar integration** (differentiator): auto-titles,
attendees, pre-meeting prompts, notes filed to the right event.

### 3. Chat & agent
Streaming chat with reasoning display; model picker mixing cloud and local
models, each labeled with privacy tier, pricing, and capability flags; rich
composer (slash commands, attachments, mentions); sessions (pin, search,
branch-from-message, compaction, auto-titles); our agent runtime with tool
calling, human approval gates (once/session/always/deny), secret prompts,
mid-run steering; Seatbelt write-jail per session with per-session unrestricted
opt-in; workspace file import/preview; MCP client + management UI; skills
(capability packs) install/enable; scheduled routines (cron) with run history;
menu-bar status + agent HUD.

### 4. Workspace intelligence (differentiator)
Local RAG: on-device embeddings over notes, transcripts, dictation history, and
agent sessions (sqlite-vec); instant semantic search UI; agent context tool that
retrieves across the whole workspace with citations.

### 5. Image generation
Text-to-image via provider adapter (OpenAI images first), `/image` command and
agent tool, inline previews, saved to workspace.

### 6. Accounts, billing, product shell
Clerk sign-in (PKCE-style flow, Keychain token storage); Free/Pro/Max tiers with
included usage + credit top-ups; metering with hold→settle semantics and
idempotency keys (no double-charging on retry); funding/sign-in gates; per-session
usage panel; onboarding with dictation practice and just-in-time permissions;
settings for every capability above; tabs, folders/projects for notes and
sessions; theming (light/dark/system) on design tokens; notifications; issue
reporting; signed + notarized DMG with auto-update (stable/rc channels).

## Acceptance criteria (v1 — each verified by test or observed runtime behavior)

1. **Dictation:** with Wi-Fi off and a local cleanup model configured, hold the
   hotkey in a third-party app, speak 5 seconds, release → cleaned text appears
   in that app. With cloud cleanup, same flow ≤ ~2s end-to-end.
2. **Meetings:** during a Zoom/Meet call, the detection prompt appears; accepting
   records mic+system; stopping yields a transcript ordered as conversation
   turns with named speaker labels (after voice-profile enrollment) and a
   structured note. Killing the app mid-recording and relaunching offers
   recovery that produces the same note without re-recording.
3. **Calendar:** a recorded meeting that overlaps a calendar event is titled
   from the event and lists its attendees.
4. **Chat/agent:** a chat streams from both a cloud model and a local Ollama
   model; the agent completes a multi-step file task inside the sandbox,
   pausing for approval before a risky action; an MCP server can be added and
   its tools used; a routine scheduled for +1 minute runs and records history.
5. **RAG:** a semantic query over ≥100 indexed items returns relevant results
   in under 1 second, fully offline; the agent answers a question using
   workspace context with citations.
6. **Billing:** sign-in via Clerk works; a metered cloud call authorizes,
   settles once (idempotent under forced retry), and decrements visible
   credits; exhausted credits show the funding gate; upgrade unlocks it.
7. **Keys:** no provider API key exists anywhere in the shipped app bundle
   (verified by scan); Arya API holds them server-side.
8. **Quality gate:** typecheck, lint, and all test suites (TS, Rust shell,
   sidecar, Arya API) pass; a signed notarized build launches clean on a
   fresh macOS 14+ machine.

## Non-goals (v1)

- Windows or Linux builds (architecture must not preclude them).
- Team features, sharing, or any server-side storage of user content (Phase 2).
- TEE attestation / confidential-VM hosting (Phase 2; design the proxy so it
  can move into one without API changes).
- Mobile apps, web app, browser extension.
- Bot-based meeting joining; speaker identification beyond locally enrolled
  voice profiles; cloud transcription of meeting audio.
- Image editing (img2img) — post-v1 candidate.
- Training or fine-tuning models.

## Language (canonical terms)

- **Dictation** vs **note transcription** — short latency-critical utterances
  vs batch transcription of a recording. Never plain "transcription".
- **Source** — one audio lane (microphone / system), each its own WAV.
- **Turn** — a detected active interval on one source, ordering the transcript.
- **Speaker** — a diarized identity within turns (Arya has real speakers; June
  did not).
- **Recording session** — one note-backed capture lifecycle; unit of recovery.
- **Sidecar** — our agent runtime process (TypeScript), spawned per write-mode.
- **Write-mode** — sandboxed (default) vs unrestricted (per-session opt-in).
- **Credits** — billing units of cloud usage ($1 = 1000 credits); local work
  never consumes credits.
- **Hold → settle** — authorize an estimated amount before a metered call,
  charge actuals (clamped, idempotent) only after upstream success.

## External dependencies (needed before the milestones that use them)

- Apple Developer ID (signing/notarization) — release engineering milestone.
- Clerk + Stripe accounts — accounts/billing milestone.
- Anthropic + OpenAI API keys, backend hosting — Arya API milestone.
- None of these block early milestones (dev builds run unsigned and local).
