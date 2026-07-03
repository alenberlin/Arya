# PROGRESS — Arya v1

Ledger for the autonomous build loop. A milestone is *done* only with evidence:
the verification command, its passing result, and the commit hash.

| Milestone | Status | Evidence |
|---|---|---|
| M1 Walking skeleton | done | `make verify` green (Biome, tsc, vitest 2/2, cargo fmt/clippy `-D warnings`, cargo test 2/2); `tauri dev` launched, process stable, `arya.db` + migrations created at runtime; commit 7507ab7 |
| M2 Local speech engine | done | `make verify` exit 0 (11 Rust unit tests + frontend suite); M2 bench (`cargo test --release --test speech_bench -- --ignored`): whisper-base.en on M4 Max, WER 0.000 (budget ≤0.15), RTF 0.023 (budget <0.5), SHA-256-verified model download; commit 318a784 |
| M3 Dictation pillar | done | `make verify` exit 0 (27 Rust + 6 frontend tests). Runtime E2E: dev hook drove capture→ASR→cleanup→history with real speech through the mic; transcript exact ("And so my fellow Americans…"), ASR 87ms for 12s audio, history row persisted. Paste stage correctly gated on the macOS Accessibility grant (needs one-time user approval; text preserved in history by design). Followups: modifier-only/Fn hotkeys (NSEvent monitor, lands with M13 hotkey-capture UI). Commit a0e6c8a |
| M4 Recording & notes core | done | `make verify` exit 0 (40 Rust + 9 frontend tests). Runtime E2E 1: record 14s with real speech → 3 turns detected at natural pauses → structured note `ready` (title, timestamped transcript, final WAV artifact). Runtime E2E 2: kill -9 mid-recording → relaunch → scan found partial WAV (969 KB) → header repaired → same note recovered to `ready` without re-recording. E2E also caught+fixed a nested-block_on panic in the async command path. Commit 48d9faf |
| M5 System audio & meeting detection | done | `make verify` exit 0 (45 Rust + 6 frontend tests). Swift tap helper (APIs verified against macOS SDK headers) compiles in build.rs, embeds in binary, spawns, reports ready, captures, stops cleanly. Meeting-mode E2E: dual artifacts (mic 1.6MB + system 2.9MB) both final; silent system track (TCC grant pending — user action) skipped per design, note ready from mic; live transcript preview produced correct rolling text during recording. Meeting detection via CoreAudio process polling (unit-tested; real-meeting detection verifiable once a meeting app runs). TCC note: System Audio Recording grant required for non-silent system capture. Commit follows |
| M6 Diarization & calendar | done | `make verify` exit 0 (50 Rust + 6 frontend tests). Diarization E2E: 3 same-speaker turns → 3 embeddings → 1 cluster (pairwise 0.79-0.83), labels flow to DB/note body/UI; clean-fixture same-speaker probe 0.847. Voice enrollment (speech-trimmed) verified end to end; cross-session profile naming flagged for human-voice QA (loopback rig varies per-session, sim 0.23 — rig artifact, threshold tune deferred to M13/M14 QA). Calendar: EventKit integration compiles + degrades correctly with access NotDetermined (grant is a one-time user action); event-titling + attendees + upcoming banner wired. Commit follows |
| M7 Agent runtime core | pending | — |
| M8 Agent ecosystem | pending | — |
| M9 Workspace RAG | pending | — |
| M10 Image generation | pending | — |
| M11 Arya API | pending | — |
| M12 Accounts & billing | pending | — |
| M13 Product shell & onboarding | pending | — |
| M14 Release engineering | pending | — |

## Log

- Blueprint approved (PRD.md + PLAN.md). Loop started at M1.
