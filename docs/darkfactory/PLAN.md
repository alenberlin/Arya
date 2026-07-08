# ARYA — Connected-Brain Expansion (PLAN)

Phased milestones in **dependency + risk order** (foundation spine first, per Phase 0).
Each milestone lists its goal, thin slices, acceptance criteria (test- or
behavior-verifiable), and dependencies. No time estimates — ordering is logical only.
Sub-slices are built and verified one at a time per `incremental-implementation`.

Legend: **AC** = acceptance criteria (must pass) · **dep** = depends on.

---

## Group A — Foundation spine

### M1 — The `links` edge store (walking skeleton of the connected brain)
The riskiest/most-foundational bet; everything connected depends on it.
- Slices: SQLite migration for `links (id, source_kind, source_id, target_kind, target_id, relation, origin, weight, created_at)` with indexes on source and target and a uniqueness constraint on the edge tuple; Rust `links` module with `create_link`, `links_from(kind,id)`, `links_to(kind,id)`, `delete_link` commands; `src/lib/links.ts` wrappers + types; unit tests (Rust in-memory pool + TS invoke mocks).
- AC: creating an edge between two existing notes persists it, reading back returns it, deleting removes it; duplicate edges are idempotent; foreign-agnostic (a dangling target is allowed but resolvable); Rust + TS tests green.
- dep: none.

### M2 — BlockNote editor for Notes (data-model evolution)
Replace the plain `body_md` textarea with a real block editor; risky because it touches existing note data.
- Slices: add BlockNote (verify React 18 pin) with offline bundling under the CSP; add `document_json` column; render/edit block-JSON; **lazy-migrate** legacy `body_md` → blocks on first open; recompute the plaintext projection on save so `rag_chunks`/search are unchanged; debounced autosave reusing the existing 600ms pattern (single-writer, no 409 machinery).
- AC: a new note round-trips block-JSON; an existing `body_md` note opens correctly and is preserved; plaintext projection + RAG search still return it; autosave + optimistic rollback verified; `make verify` green.
- dep: none (parallel-safe with M1).

### M3 — `@`-mention linking + backlinks panel (F1, F3-links)
Make the connected brain real in the editor.
- Slices: BlockNote `@`-mention menu resolving nodes across `note | dictation | meeting | mindmap` (live title/icon resolve); on save, reconcile the note's outbound edges into `links` (delete-by-source + reinsert, fault-isolated so a save never fails on link errors); render mentions as provenance chips; **backlinks panel** listing inbound edges with jump-to.
- AC: `@`-mentioning any node type persists a `mention` edge and renders a chip; deleting/re-editing reconciles edges; the target's backlinks panel shows the source and navigates to it; broken targets render "deleted"; tests cover reconcile + render.
- dep: M1, M2.

### M4 — AI-transform primitive + F15 + F16
One engine, two entry points; local-default per Phase 0.
- Slices: sidecar one-shot RPC `ai.transform { sourceText, instruction, targetLang? } → resultText` (local Ollama default; optional labeled cloud via the existing proxy) with a "transform/translate/reorganize — do not invent" system guard; **F15** inline `@node + instruction` — explicit run gesture (⌘↵ or menu item, never auto-fire), resolve node content, insert result as a new block "via @node", keep the mention as provenance; **F16** whole-note **Sort** — cluster a dump into coherent topic sections with a **non-destructive preview → accept** (original preserved, undo).
- AC: `@xyz translate to German` inserts a German block below without touching the source; Sort produces topic-grouped sections in a preview the user accepts or discards; both run locally by default and fall back to source on model failure (never lose content); cloud path is opt-in and labeled; tests cover the RPC contract + non-destructive insert + failure fallback.
- dep: M1, M3.

## Group B — Notes completion

### M5 — Nested pages + Notion import (F3-nesting, F4)
- Slices: `parent_note_id` + tree build (recursive CTE for subtree ops) + sidebar nesting; Notion **Markdown+CSV** importer (unzip, create page shells, convert Markdown→blocks, rewrite internal links to ARYA notes; databases/relations degrade to plain pages/text since databases are a non-goal); progress + tolerant per-item error handling.
- AC: notes nest and reorder; a real Notion export imports as a page tree with working internal links; malformed items are skipped, not fatal; import report shows counts; tests cover the Markdown→block conversion + link rewrite.
- dep: M2, M3.

## Group C — Dictation daily-driver

### M6 — Kill forced-English + language picker (F7 core, B1, F5)
Isolated, high-value; the observed mistranslation fix.
- Slices: stop defaulting `language: Some("en")`; default to auto-detect (`None`) with an explicit **language picker** (global + per-profile); ensure dictations are first-class `dictation` nodes in the graph and deletable one/all; regression test that non-English audio is not forced to English.
- AC: dictating in Spanish/German/Italian/Croatian yields in-language text (no forced English); the picker overrides auto-detect and is honored end-to-end (preview + final); a regression test pins that `en` is not force-set; delete-one/all works.
- dep: none (independent; may run alongside Group A/B).

### M7 — Multilingual model shelf (F7 models)
- Slices: catalog `ModelSpec` entries (SHA-256 pinned) for **Parakeet-TDT-v3** (sherpa-onnx path) and **German (primeline)** + **French (bofenghuang)** GGML fine-tunes (whisper.cpp path); a language→model map + "Recommended for <language>" picker; license/attribution surfacing (MIT/Apache/CC-BY only); non-English **whisper-ticker** preview fallback (streaming stays English-only).
- **Risk/escape hatch:** if Parakeet-on-sherpa-onnx does not convert/run cleanly on-device (flagged as needs-verification), fall back to multilingual turbo + the DE/FR GGML fine-tunes (lower-risk, proven path) and mark the Parakeet sub-slice blocked — do not thrash.
- AC: selecting a language downloads/uses the recommended model (verified checksum); DE/FR fine-tunes measurably improve those languages vs turbo on a small held set; attribution present; offline fallback works; tests cover catalog integrity + language→model mapping.
- dep: M6.

### M8 — Direct ↔ Polished + tone (F6)
- Slices: a Direct (verbatim) vs Polished toggle in the dictation UI; Polished routes through the M4 AI-transform primitive with a tone parameter (polite/friendly/professional); extends the existing Raw/Clean/Polished path; local-default.
- AC: Direct output is verbatim; Polished rephrases in the chosen tone; switching is one action; runs locally by default, cloud optional/labeled; tests cover mode routing + tone prompt selection.
- dep: M4, M6.

### M9 — Right-click translate a saved dictation (F8)
- Slices: `dictation_translations (id, dictation_id, lang, text, model, created_at)` (non-destructive, multi-language); right-click → language submenu → translate via the primitive; render original-on-top / translation-below (reuse the existing side-by-side pattern); index translations for search.
- AC: translating appends below the original without mutating it; multiple languages stack; a query in the target language finds the translation; deleting the dictation cascades its translations; tests cover storage + search indexing.
- dep: M4, M6.

## Group D — Search & new surfaces

### M10 — Search everything by title + content (F14)
- Slices: hybrid search over all node types (notes/dictations/meetings/mindmaps) combining the existing semantic index with explicit title+content full-text; unified result model + ranking; scope filters by node type.
- AC: a query matches by both title and content across every node type; results link to the node; ranking is sensible on a fixture set; tests cover the query paths.
- dep: M2 (note text projection), M6/M9 (dictation text), M1 (node identity).

### M11 — Galaxy knowledge graph (F10)
- Slices: a Rust `galaxy_graph()` command assembling nodes (notes/folders/dictations/meetings) + edges from `links` (mention/structural) + **semantic** cosine over `rag_chunks` (per-node **top-K**, not threshold, to avoid hairballs); `react-force-graph-2d` panel (offline-bundled) with node select, type filter, search, degree sizing; assemble on tab-open + in-memory cache invalidated on reindex; graceful offline fallback (structural/mention only).
- Staged defaults (adjustable at approval): **2D first**; defer 3D, AI-suggested edges, and client-side clustering to a follow-up slice.
- AC: Galaxy renders nodes + edges for a populated brain; selecting a node highlights its neighborhood; type filter + search work; with Ollama off it still renders structural/mention edges; perf is acceptable at a realistic node count; tests cover graph assembly (dedup/merge, top-K).
- dep: M1, M2, M6 (nodes + links exist).

### M12 — Mind Map (F11)
- Slices: React Flow (`@xyflow/react`, offline-bundled) canvas — nodes/edges/shapes/sticky notes, zoom/pan; opaque JSON storage in a `mindmaps` table; **debounced** autosave (no per-mousemove writes) + **persisted viewport**; optional node→note `links` edge so mind-map nodes appear in Galaxy.
- AC: create/edit/save/reload a map at saved positions; arbitrary node-to-node edges supported; autosave is debounced; viewport persists; a node linked to a note shows in Galaxy; tests cover CRUD + serialization.
- dep: M1 (for the optional node→note links).

## Group E — Agent & shell polish

### M13 — Agent multi-line composer (F12)
- Slices: replace the single-line input with an auto-growing multi-line composer (≥5 rows visible), Enter=submit / Shift+Enter=newline (or ⌘Enter=submit — settle in build), preserving existing send/stream wiring.
- AC: the composer shows ≥5 lines, grows/shrinks with content, submits and newlines per the chosen key model; existing agent streaming unaffected; component test covers key handling.
- dep: none.

### M14 — Surface local-first security (F13) + shell hierarchy tidy (D5)
- Slices: make the existing agent security legible (approval scoping / sandbox / on-device indicators — copy + a small panel or affordance, no new security mechanics); fold **MCP** (and consider **Routines**) under **Agent** so the sidebar leads with Notes + Dictation rather than "six peers." (D5 is a recommended default; adjustable at approval.)
- AC: the security posture is visible without reading code; the sidebar hierarchy leads with the pillars; no regression to agent/mcp/routines behavior; contrast/token tests pass.
- dep: none (touches shell; do late to avoid churn).

---

## Phase 2 — Integration & review (after all milestones done/blocked)

- Full `make verify` + runtime verification of every primary flow.
- `code-review` across the change; for this release-grade expansion also run the `security-auditor` (input/import/AI-RPC/paths), `test-engineer` (coverage, reproduction tests), and `performance-auditor` (Galaxy render, editor, search) personas.
- Fix every Critical/Important finding, re-verify; record Suggestions in `PROGRESS.md`.
- Write `REPORT.md` (each PRD AC with evidence, how to run, non-goals honored, anything blocked).

## Notes on autonomy for this plan

- Independent milestones (M6, M13) may proceed even if a Group-A item is mid-flight.
- The one flagged technical risk is **Parakeet-on-sherpa-onnx** (M7) — it has a defined fallback and is quarantined by revert if it fails past budget, without blocking the rest.
- Blueprint approval is the green light for the loop's atomic per-milestone commits; **pushing/deploying always asks**.
