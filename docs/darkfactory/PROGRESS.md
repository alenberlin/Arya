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
| M2 — BlockNote editor + migration | pending | — |
| M3 — @-mentions + backlinks | pending | — |
| M4 — AI-transform primitive + F15/F16 | pending | — |
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
`-D warnings` clean; full frontend gate green. **Commit:** recorded at next update.

**End-to-end note:** the data + command layer is proven end-to-end (Rust
integration tests over the real migration). The *UI* surfacing of links lands in
M3 (mentions + backlinks panel); M1 is the substrate.

## Blockers (carry-forward)
_None yet._
