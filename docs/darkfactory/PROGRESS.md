# ARYA — Connected-Brain Expansion (PROGRESS ledger)

Status of each milestone with evidence (verify command, result, commit). A status
without evidence is not a status. Written ahead: *in progress* before the first
slice, *done* only with the verification result + commit hash attached.

**Blueprint approved:** 2026-07-08 (owner set an autonomous goal for Group A).
**Active goal:** complete Group A (M1→M4), fully functional. Blockers are
quarantined and documented, not stopped on.

| Milestone | Status | Evidence |
|---|---|---|
| M1 — links edge store | ✅ done | verify-rust green (fmt, clippy -D warnings, 115 tests incl. 8 links + note-cleanup); verify-front green (brand, scan-keys, biome, tsc, 23 vitest incl. links bindings); sidecar/api untouched |
| M2 — BlockNote editor + migration | ✅ done | verify-rust green (116 tests incl. document_json round-trip + migration 0011); verify-front green (28 tests, typecheck, biome); production build bundles BlockNote offline; live webview render deferred to on-device QA |
| M3 — @-mentions + backlinks | ✅ done | verify-rust green (117 tests incl. reconcile); verify-front green (31 tests, typecheck, biome, build); mention chips + backlinks wired; live editor render deferred to on-device QA |
| M4 — AI-transform primitive + F15/F16 | in progress | — |
| M5 — nested pages + Notion import | pending (Group B) | — |
| M6 — forced-en fix + language picker | pending (Group C) | — |
| M7 — multilingual model shelf | pending (Group C) | — |
| M8 — Direct/Polished + tone | pending (Group C) | — |
| M9 — translate a saved dictation | pending (Group C) | — |
| M10 — search everything | pending (Group D) | — |
| M11 — Galaxy 2D | pending (Group D) | — |
| M12 — Mind Map | pending (Group D) | — |
| M13 — agent multi-line composer | pending (Group E) | — |
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

### M4 — AI-transform primitive + F15/F16 — in progress
- **AI-transform primitive** (Rust `transform.rs`): `ai_transform` command,
  **local Ollama by default / cloud optional** (generalizes `translate`); the
  system prompt forbids inventing content (reorganize/rephrase/translate only).
  Tested (3): prompt shape, clean error when Ollama is down, guard present.
- **F16 (Sort)** ✅: a "Sort" action reorganizes a note's plaintext into coherent
  sections via the primitive, shown in a **non-destructive preview** (Accept
  replaces the note via a keyed remount + lazy markdown→blocks conversion;
  Discard leaves it untouched).
- **F15 (inline `@node + instruction`)**: the remaining slice — next.

**Evidence so far:** verify-rust green (120 tests, +3 transform); verify-front
green (31 tests, typecheck, biome, build). **Commit (primitive + F16):** recorded
at next update.

## Blockers (carry-forward)
_None. (Live-webview visual QA for the editor/mentions/Sort is deferred to
on-device, not a blocker.)_
