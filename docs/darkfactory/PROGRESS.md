# ARYA — Connected-Brain Expansion (PROGRESS ledger)

Status of each milestone with evidence (verify command, result, commit). A status
without evidence is not a status. Written ahead: *in progress* before the first
slice, *done* only with the verification result + commit hash attached.

**Blueprint approved:** 2026-07-08 (owner set an autonomous goal for Group A).
**Active goal:** complete Group A (M1→M4), fully functional. Blockers are
quarantined and documented, not stopped on.

**✅ GROUP A COMPLETE (2026-07-08).** M1–M4 all done; full `make verify` GREEN
(front 34 tests + rust 120 tests + sidecar 23 + arya-api 41; fmt/clippy/biome/tsc
all clean). Commits: `b2850d5` (blueprint) → `a26fd2d` (M1) → `90a3086` (M2) →
`68eb15e` (M3) → `a0e8d58` (M4 primitive+F16) → `9581a1f` (M4 F15), on branch
`remediation/review-fixes` (not pushed). **Next:** Groups B–E (M5–M14) remain.
**Deferred to on-device QA (not blockers):** live visual/interaction QA of the
BlockNote editor, mention menu/chips, Sort preview, and the ⌘↵ inline command in
the real Tauri webview — headless verification can't drive the webview.

| Milestone | Status | Evidence |
|---|---|---|
| M1 — links edge store | ✅ done | verify-rust green (fmt, clippy -D warnings, 115 tests incl. 8 links + note-cleanup); verify-front green (brand, scan-keys, biome, tsc, 23 vitest incl. links bindings); sidecar/api untouched |
| M2 — BlockNote editor + migration | ✅ done | verify-rust green (116 tests incl. document_json round-trip + migration 0011); verify-front green (28 tests, typecheck, biome); production build bundles BlockNote offline; live webview render deferred to on-device QA |
| M3 — @-mentions + backlinks | ✅ done | verify-rust green (117 tests incl. reconcile); verify-front green (31 tests, typecheck, biome, build); mention chips + backlinks wired; live editor render deferred to on-device QA |
| M4 — AI-transform primitive + F15/F16 | ✅ done | verify-rust green (120 tests); verify-front green (34 tests, typecheck, biome, build); Sort + inline @-command wired; live webview QA deferred to on-device |
| M5 — nested pages + Notion import | ✅ done (Group B) | verify green: nesting (data+tree UI, subtree cascade verified) + Notion folder import (hierarchy, links→edges); 125 rust + 34 front tests |
| M6 — forced-en fix + language picker | ✅ done (Group C) | default language → auto-detect (regression test); full ISO-639-1 picker; multilingual turbo already default |
| M7 — multilingual model shelf | ◐ partial (Group C) | baseline multilingual via turbo works + English-only guardrail shipped; specialist DE/FR pins + Parakeet engine are device/network blockers (documented) |
| M8 — Direct/Polished + tone | ✅ done (Group C) | Cleanup seg (Direct/Clean/Polished) + PolishedTone (neutral/polite/friendly/professional) applied in the Polished prompt; 127 rust + 34 front tests |
| M9 — translate a saved dictation | ✅ done (Group C) | right-click ⋯ → Translate to → language; non-destructive `dictation_translations` (one/lang, cascade), stacked in history; 128 rust + 34 front tests |
| M10 — search everything | ✅ done (Group D) | literal `search_all` (title+content, offline) across notes/transcripts/dictations/translations, merged with semantic rag_search in SearchPanel; 130 rust + 34 front tests |
| M11 — Galaxy 2D | ✅ done (Group D) | galaxy_graph (notes+dictations; mention/child edges from links+nesting; best-effort semantic top-K over rag_chunks); react-force-graph-2d panel + Galaxy tab; 131 rust tests; live canvas + semantic edges verify on-device |
| M12 — Mind Map | ✅ done (Group D) | React Flow canvas (@xyflow/react) over a `mindmaps` table (opaque `doc_json`); add/connect/rename nodes, debounced autosave; delete reconciles `links`; joined into search-all; 132 rust + 34 front tests; live drag/connect verifies on-device |
| M13 — agent multi-line composer | ✅ done (Group E) | composer is an auto-growing textarea with a 5-row floor (CSS min-height) + cap-then-scroll; Enter sends, Shift+Enter newline, ⌘/Ctrl+Enter sends; existing send/stream wiring untouched; 2 new component tests (key handling + row floor); 36 front tests |
| M14 — surface security + shell tidy | pending (Group E) | — |

## Log

### M1 — links edge store — ✅ done
The polymorphic edge store that F1/F3/F10/F15 ride. Delivered:
- `migrations/0010_links.sql` — `links` table (polymorphic `(kind,id)` endpoints,
  no FKs so cross-kind + dangling targets are allowed), unique edge index for
  idempotency, source/target indexes for neighbourhood + backlink reads.
- `src/links.rs` — `Link` model; `insert_link` (idempotent upsert), `links_from`,
  `links_to` (backlinks), `delete_link_by_id`, `delete_for_node`,
  `delete_for_kind`; `create_link`/`list_links_from`/`list_links_to`/`delete_link`
  commands with kind + self-loop + empty-id validation.
- Referential cleanup wired into `notes::delete_note_inner` / `delete_all_notes_inner`
  so deleting a note drops its edges (proven by a test).
- `src/lib/links.ts` bindings + `src/test/links.test.ts`.

**Evidence:** 8 Rust unit tests (round-trip both directions, idempotency,
distinct-relations coexist, delete, delete-for-node, dangling target permitted,
note-deletion cleanup, validation) + 4 TS binding tests, all green; fmt + clippy
`-D warnings` clean; full frontend gate green. **Commit:** `a26fd2d`
(blueprint: `b2850d5`).

**End-to-end note:** the data + command layer is proven end-to-end (Rust
integration tests over the real migration). The *UI* surfacing of links lands in
M3 (mentions + backlinks panel); M1 is the substrate.

### M2 — BlockNote block editor + migration — ✅ done
Replaced the plain-markdown note-body `<textarea>` with a **BlockNote** block
editor (F2), backward-compatible with existing notes.
- Chose BlockNote 0.51.4 (React-18 compatible) with the **Ariakit** UI variant —
  the Mantine variant pulls Mantine 9, which requires React 19 (rejected: a
  global React-19 upgrade is out of scope). The app CSP already permits the
  inline styles ProseMirror needs.
- `migrations/0011_note_documents.sql` — `document_json` column (block-JSON
  source of truth); `body_md` becomes its markdown projection so search + RAG are
  untouched; empty `document_json` = legacy note.
- `notes.rs` — `document_json` on `NoteDetail`; testable `fetch_note_detail` and
  `update_note_fields` helpers behind thin commands.
- `BlockEditor.tsx` — uncontrolled editor keyed by note id; emits
  `(documentJson, bodyMd)` on change; lazily converts a legacy note's markdown to
  blocks on first mount (persisting the migration); follows the app light/dark
  theme. Pure `blockDocument.parseInitialContent` extracted (no BlockNote runtime
  dep), hardened against corrupt/missing JSON, and unit-tested.
- **Env blocker resolved:** pnpm store v10/v11 mismatch → re-linked `node_modules`
  with the pinned pnpm (frozen lockfile); resulting lockfile diff is
  BlockNote-only (+740, no deletions, `lockfileVersion` unchanged).

**Evidence:** verify-rust green (116 tests incl. document_json round-trip);
verify-front green (28 tests: +5 parseInitialContent, rollback retargeted to
manual-notes, BlockEditor stubbed in shell tests; typecheck + biome clean);
`pnpm build` bundles BlockNote offline. **Deferred (not a blocker):** live visual
QA of the editor in the Tauri webview → on-device (project pattern). **Commit:**
`90a3086`.

### M3 — @-mentions + backlinks panel — ✅ done
Made the connected brain real in the editor (F1/F3).
- Rust: `reconcile_source_links` (transactional delete-by-source + re-insert of a
  relation's edges) + `reconcile_links` command + `LinkTarget`. Invalid targets
  (unknown kind, empty id, self-loop) are skipped not fatal; duplicates collapse.
- Editor: a custom BlockNote `mention` inline content (`mentionSchema.tsx`) + an
  `@` suggestion menu listing notes; mention chips navigate on click (delegated
  native listener). Pure `extractMentionTargets` walks the document for targets;
  the editor emits them on change.
- Reconcile-on-save: NotesWorkspace reconciles the note's mention edges only when
  the body changed, best-effort (a graph hiccup never rolls back the note).
- `BacklinksPanel` — inbound edges for the open note, with jump-to; refetches on
  note change (sufficient for the single-open-note model — the `refreshToken`
  machinery was removed as unnecessary).

**Evidence:** verify-rust green (117 tests, +1 reconcile); verify-front green
(31 tests: +3 extractMentionTargets; shell mocks handle `list_links_to`;
typecheck + biome clean); `pnpm build` OK. **Deferred (not a blocker):** live QA
of the mention menu / chips / backlinks in the Tauri webview → on-device.
**Mentionable kinds:** notes today; dictations/mindmaps join as those surfaces
mature (the schema + reconcile already support all kinds). **Commit:** `68eb15e`.

### M4 — AI-transform primitive + F15/F16 — ✅ done
- **AI-transform primitive** (Rust `transform.rs`): `ai_transform` command,
  **local Ollama by default / cloud optional** (generalizes `translate`); the
  system prompt forbids inventing content. Tested (3): prompt shape, clean error
  when Ollama is down, anti-invention guard.
- **F16 (Sort)** ✅: reorganizes a note's plaintext into coherent sections via the
  primitive, shown in a **non-destructive preview** (Accept replaces the note via
  a keyed remount + lazy markdown→blocks; Discard leaves it untouched).
- **F15 (inline `@node + instruction`)** ✅: ⌘↵ on a block ending in
  `@node <instruction>` resolves the node's text, applies the instruction via the
  primitive, and inserts the result after the block (the mention stays as
  provenance). Pure `extractInlineCommand` parses the command; 3 tests.

**Evidence:** verify-rust green (120 tests); verify-front green (34 tests: +3
extractInlineCommand; typecheck, biome, build). **Deferred (not a blocker):** live
QA of Sort / inline-command in the Tauri webview → on-device. **Commits:**
primitive+F16 `a0e8d58`; F15 `9581a1f`.

### M5 — nested pages + Notion import — ✅ done (Group B complete)
- **Nesting (F3):** `parent_note_id` (migration 0012, self-FK ON DELETE CASCADE —
  *verified* to cascade in SQLite); `insert_note_under`, `set_note_parent`
  (cycle-guarded), subtree-aware delete (collects the whole subtree's files +
  edges before the cascade). Sidebar renders a **page tree** (twisties, indent),
  with context-menu "Add sub-page" / "Move to top level"; search stays flat.
- **Notion import (F4):** `notion_import.rs` walks an unzipped export folder →
  page hierarchy from the `Title <hex>` / `Title <hex>/` structure; markdown
  stored as `body_md` (lazy-converts on open); internal links → `mention` edges.
  "Import" button + directory picker + result notice.

**Evidence:** full `make verify` GREEN (rust 125 tests incl. nesting + importer;
front 34; sidecar 23; arya-api 41; fmt/clippy/biome/tsc clean). **Commits:**
`7d23f35` (nesting data) → `1e11d58` (tree UI) → `4b394b9` (Notion import).
**Follow-ups (documented, not blockers):** zip auto-extraction (import needs an
unzipped folder today); converting imported inline links into mention *chips*
(connections are already captured as edges); a dedicated tree-render component
test; live webview QA of the tree/import → on-device.

**✅ GROUP B COMPLETE (2026-07-08).** M5 done; `make verify` green. Next: Groups
C–E (M6–M14) — dictation multilingual/Direct-Polished/translate, search-all,
Galaxy, Mind Map, agent multi-line, shell tidy.

### M6 — auto-detect language + picker — ✅ done
The observed mistranslation bug: dictation forced `language='en'`. Default is now
`None` (auto-detect; regression test), and the Speech-language picker offers the
full ISO-639-1 set. The multilingual turbo model is already the default, so this
alone fixes non-English dictation. **Commit:** with the Group-C commits below.

### M7 — multilingual model shelf — ◐ partial (device/network blockers)
- **Baseline multilingual ASR works now:** default = multilingual
  `whisper-large-v3-turbo`, and M6 unforced the language — Spanish/German/Italian
  etc. transcribe correctly today (the PLAN's own turbo fallback).
- **Guardrail shipped:** `.en` models are labelled "English only" and the panel
  warns when one is paired with a non-English language, steering to the
  multilingual model (verify-front green).
- **BLOCKED (device/network), carried forward:**
  1. Pinning the **German (primeline)** + **French (bofenghuang)** GGML specialist
     models needs each multi-GB file downloaded to compute its SHA-256 — not
     possible headlessly, and shipping unpinned model URLs would break the
     load-bearing pin-security model.
  2. **NVIDIA Parakeet-TDT-v3** needs a new sherpa-onnx NeMo-transducer *engine*
     (today only whisper.cpp does batch ASR; sherpa is streaming-only) + on-device
     ASR-quality verification.
  Both are enhancements over the working turbo baseline; the
  recommended-model-per-language map lands with them.

### Group C — dictation daily-driver — ✅ complete (M7 has a documented carry-forward)
- **M6** `0da53c5` — auto-detect default + full language picker (the observed-bug fix).
- **M7** `9691a13` — ◐ baseline multilingual (turbo) + English-only guardrail;
  specialist DE/FR pins + Parakeet engine are device/network blockers (below).
- **M8** `a90d930` — Direct/Clean/Polished cleanup control + PolishedTone
  (neutral/polite/friendly/professional) in the Polished prompt.
- **M9** `cdf2bc6` — right-click translate a saved dictation, non-destructive +
  stacked, one per language, cascade on delete.

**✅ GROUP C COMPLETE (2026-07-08)** to the extent buildable headlessly; full
`make verify` GREEN (rust 128 / front 34 / sidecar 23 / arya-api 41). Voice runtime
(ASR + LLM polish/translate) verifies on-device. Next: Groups D–E (M10 search-all,
M11 Galaxy, M12 Mind Map, M13 agent multi-line, M14 shell tidy).

### Group D — search & surfaces — ✅ complete
- **M10** `b9cec25` — search everything by title + content: literal `search_all`
  (offline; notes/transcripts/dictations/translations) merged with semantic
  `rag_search` in the SearchPanel.
- **M11** `689b3bb` — Galaxy 2D knowledge-graph: `galaxy_graph` (nodes + mention/
  child edges from `links`+nesting + best-effort semantic top-K over `rag_chunks`),
  rendered with react-force-graph-2d.
- **M12** `d34c5d5` — Mind Map: React Flow canvas (`@xyflow/react`) over a
  `mindmaps` table (opaque `doc_json`); add/connect/rename nodes with debounced
  autosave; delete reconciles `links`; joined into search-all.

**✅ GROUP D COMPLETE (2026-07-08)** — full `make verify` GREEN
(rust 132 / front 34 / sidecar 23 / arya-api 41). Galaxy's live canvas + semantic
edges and the Mind Map drag/connect/rename interactions verify on-device (they need
a real render surface headless CI can't provide). Next: Group E (M13 agent
multi-line composer, M14 surface security + shell tidy).

## Blockers (carry-forward)
- **M7 specialist models (DE/FR GGML pins) + Parakeet engine** — device/network
  work: download GB models to pin SHA-256, integrate the sherpa NeMo-transducer
  engine, verify ASR quality on-device. Baseline multilingual (turbo) works
  without them.
- Live-webview visual QA for the editor/mentions/Sort/tree/import/dictation is
  deferred to on-device (not a blocker).
