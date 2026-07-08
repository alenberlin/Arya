# ARYA — Connected-Brain Expansion (PRD / Blueprint)

**Status:** awaiting approval (the single Dark Factory gate) · **Date:** 2026-07-08 ·
**Branch:** remediation/review-fixes · **Source of ideas:** `docs/ideas-inbox.md` (F1–F16)

## Overriding principle (owner mandate)

**Quality is the only metric** — correctness, robustness, security, clean
architecture, maintainability. Cost and time are irrelevant and do not enter any
decision. "Phases/milestones" order work by **logical dependency and risk only**,
never by time. Simplicity is a quality metric: build exactly what's needed, and
build it excellently. No test or acceptance criterion is ever weakened to pass.

## Intent

Turn ARYA from a set of capable-but-siloed tools into **one local-first, private,
connected second brain** on the Mac. Voice is the daily driver; everything
captured — notes, dictations, meetings, mind maps — becomes a **permanent,
linkable, searchable node** in one database, visualized as a graph (Galaxy).
Notes, dictation, and meeting minutes must each be **best-in-class**, all
on-device by default, all connected.

## Users

The owner and privacy-aware professionals who use ARYA **daily and voice-first**.
Single user, single device, offline-capable. No multi-user/collab requirement.

## Scope — the 16 non-deferred features

| ID | Feature |
|---|---|
| F1 | `@`-tag anything → connected-brain `links` spine |
| F2 | Notion-like block editor (BlockNote) |
| F3 | Nested pages + visible backlinks panel |
| F4 | Import from a Notion (Markdown & CSV) export |
| F5 | Dictations saved as first-class brain nodes; delete one/all *(partly exists)* |
| F6 | Direct ↔ Polished toggle + tone (polite/friendly/professional) |
| F7 | Multi-language dictation + meeting minutes (incl. forced-`en` fix) |
| F8 | Right-click "Translate to…" a saved dictation — non-destructive, appended, searchable |
| F9 | Meeting-minutes niceties: templates, action items, cross-meeting chat (+ calendar-detect, staged) |
| F10 | Galaxy — knowledge-graph visualization |
| F11 | Mind Map — node canvas (React Flow) |
| F12 | Agent multi-line composer (≥5 lines) |
| F13 | Surface the (already-built) local-first agent security in the UI |
| F14 | Search everything (notes/dictations/meetings/mind maps) by title **and** content |
| F15 | Inline `@node + instruction` AI action |
| F16 | "Sort" — reorganize a brain dump into coherent topic sections |

## Decisions settled in Phase 0

1. **Build order:** foundation spine first (links + block editor + AI-transform primitive), then dictation daily-driver, then new surfaces, then agent/shell polish.
2. **AI compute & privacy:** **local Ollama by default** for every AI transform (polish/tone, translate, Sort, `@`-actions); **optional, clearly-labeled cloud** for quality or languages the local model handles poorly. Never train on user data.
3. **Notes editor:** **BlockNote** (MIT, ProseMirror-based, bundles offline). Verify React 18 compatibility and pin accordingly.
4. **Multilingual ASR:** keep multilingual `large-v3-turbo` as default; add **NVIDIA Parakeet-TDT-v3** (CC-BY-4.0, 25 EU languages) as a multilingual engine; add **German (primeline)** and **French (bofenghuang)** specialist fine-tunes. Ship MIT/Apache-2.0/CC-BY-4.0 weights only.

## Cross-cutting architecture

- **The `links` table** — one polymorphic edge store `(source_kind, source_id, target_kind, target_id, relation, origin, weight, created_at)` in local SQLite; the spine F1, F3, F10, F15 all read/write. Node kinds: `note | dictation | meeting | mindmap` (extensible).
- **The AI-transform primitive** — one sidecar one-shot RPC `{ sourceText, instruction, targetLang? } → resultText`, local-Ollama-default / cloud-optional, powering F6/F8/F15/F16. Output is **non-destructive** and guarded with "transform/translate/reorganize — do not invent."
- **Notes data-model evolution (backward-compatible):** add `document_json` (BlockNote block-JSON, opaque) and `parent_note_id` (nesting) to `notes`; **lazy-convert** existing `body_md` on first open; keep a plaintext projection feeding `rag_chunks` (search/RAG unaffected).
- **Established wiring pattern:** SQLite migration → Rust `#[tauri::command]` (thin, `Result<T,String>`, camelCase serde) → `src/lib/<feature>.ts` invoke wrapper → `src/<feature>/<Feature>Panel.tsx` (tokens, `.screen`/`.panel`) → tab in `App.tsx` + icon; async via `emit`/`listen`.
- **Privacy posture:** on-device by default; cloud opt-in and labeled; locked CSP (all libs bundled, offline); MIT.

## Acceptance criteria (whole project)

The project is done when all of the following are demonstrably true (each maps to milestone-level tests + runtime evidence):

1. From a note, `@`-mentioning another note / dictation / meeting / mind map creates a persisted edge in `links`, resolves the target's live title, and appears in the target's **backlinks panel**.
2. Notes are edited in a **BlockNote** block editor; existing `body_md` notes open correctly (lazy-migrated); plaintext search/RAG still works.
3. Notes **nest** (parent/child) and a Notion **Markdown+CSV export** imports as pages with working internal links.
4. `@node + instruction` (F15) and **Sort** (F16) run via the local-default AI primitive, insert results **non-destructively**, and never fabricate content beyond the source.
5. Dictation transcribes in the **spoken language** (no forced-English); a **language picker** exists with auto-detect default; the forced-`en` behavior is gone (regression test).
6. The multilingual **model shelf** installs turbo + Parakeet + DE/FR specialists via the pinned-download catalog, with a "Recommended for <language>" picker and correct licensing/attribution.
7. Dictation offers **Direct ↔ Polished** with a **tone** (polite/friendly/professional); Direct is verbatim, Polished rephrases locally by default.
8. Right-clicking a saved dictation **translates** it, appending the translation **below** the original, stored non-destructively, **multiple languages stackable**, and **searchable**.
9. Meeting minutes support **templates**, **action-item extraction**, and **cross-meeting chat** (grounded in captured meetings, with citations).
10. **Galaxy** renders a 2D force-graph of nodes + edges (mention/structural + semantic-cosine), with node selection, type filter, and search; it degrades gracefully offline (structural/mention only).
11. **Mind Map** provides a React-Flow canvas (nodes/edges/shapes/sticky notes, zoom/pan) with debounced autosave and persisted viewport.
12. The agent composer is **multi-line (≥5 rows)** with correct submit vs newline handling.
13. The app makes the **local-first agent security** legible in the UI (approvals scoping, sandbox, on-device).
14. **Search** returns results across all node types matching **title and content**.
15. `make verify` is **green** (front brand/secret-scan/biome/tsc/vitest, Rust fmt/clippy-`-D warnings`/tests, sidecar, api), and primary flows pass **runtime verification**.

## Non-goals & constraints (explicit)

- **Deferred Notes extras:** databases/table-views, comments, version history, templates-as-a-system. Out of scope.
- **v2 integrations:** Google Calendar, Gmail, custom SMTP **as products** — out of scope. F9 **calendar-detect** is staged and depends on ARYA's *existing* calendar read access; if that access isn't already present it defers with the v2 calendar work (noted in PLAN).
- **Streaming live-preview stays English-only** for now (the sherpa zipformer). Non-English users get the multilingual **whisper-ticker** preview (interval re-transcribe), not word-by-word. True low-latency multilingual streaming is later.
- **Arabic dictation is best-effort**, not at parity — surfaced honestly in the UI.
- **Honesty cleanups D3 (image-gen stub) and D4 (Pro/Max tier labels)** from `ideas-inbox.md` are **tracked separately**, not built in this loop.
- **Single-user, local, no collaboration/CRDT.** Optimistic single-writer.
- **macOS only.**

## How "done" is verified

`make verify` green + runtime verification of each primary flow (edit/link a note; dictate in a non-English language; translate a dictation; run Sort; open Galaxy; open a mind map; multi-line agent send). Evidence recorded per milestone in `PROGRESS.md`; summarized in `REPORT.md`.
