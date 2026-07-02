# PROGRESS — Arya v1

Ledger for the autonomous build loop. A milestone is *done* only with evidence:
the verification command, its passing result, and the commit hash.

| Milestone | Status | Evidence |
|---|---|---|
| M1 Walking skeleton | done | `make verify` green (Biome, tsc, vitest 2/2, cargo fmt/clippy `-D warnings`, cargo test 2/2); `tauri dev` launched, process stable, `arya.db` + migrations created at runtime; commit 7507ab7 |
| M2 Local speech engine | done | `make verify` exit 0 (11 Rust unit tests + frontend suite); M2 bench (`cargo test --release --test speech_bench -- --ignored`): whisper-base.en on M4 Max, WER 0.000 (budget ≤0.15), RTF 0.023 (budget <0.5), SHA-256-verified model download; commit follows M2 |
| M3 Dictation pillar | pending | — |
| M4 Recording & notes core | pending | — |
| M5 System audio & meeting detection | pending | — |
| M6 Diarization & calendar | pending | — |
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
