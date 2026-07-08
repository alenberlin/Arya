# ARYA — Ideas Inbox

**Purpose:** one place to capture every product idea, observation, decision, and
research finding so nothing is lost. This is a *capture + classify* doc, not a
committed plan. Items are brain-dumped as they come; we sort and prioritize
later. Nothing here is scheduled or promised.

**Started:** 2026-07-08 · **Working mode:** owner brain-dumps ideas (sometimes
unrelated to the current thread); we classify afterward.

**Status legend:** `idea` (proposed) · `researched` (backed by verified
research this session) · `observed` (real behavior the owner saw) ·
`verified-bug` (confirmed in source) · `decision-pending` (needs the owner's
call) · `later/v2` (explicitly deferred).

---

## 1. The vision — one connected "single brain"

ARYA is a **local-first, private, connected second brain** on the Mac. Voice is
the daily driver; everything captured (notes, dictations, meetings, mind maps)
becomes a **permanent, linkable, searchable node** in one database — visualized
as a graph (Galaxy). It should do **notes, dictation, and meeting minutes
exceptionally well**, all on-device, all connected. Cloud is optional and
clearly labeled.

Today's three pillars: **Notes · Agent chat (local-first) · Dictation + Meeting
minutes.** Expanding with **Galaxy** and **Mind Map**.

---

## 2. Feature ideas

### A. The connected brain (the shared spine)

- **F1 — `@`-tag anything from anywhere** `[idea]`
  While editing a note, `@`-tag another note, a dictation, a meeting, or a mind
  map. Every capture is a node; every `@` is an edge. This is the core of the
  "single brain."
  *Architecture:* a polymorphic `links` table (source/target kind+id, relation,
  origin) — the ARYA analog of AlenAI's `entity_links`. Notes mentions write
  edges; Galaxy reads them. Build this once; Notes, Galaxy, and Mind Map all ride
  it. *Relates to:* F5, F6, F11.

### B. Notes

- **F2 — Rich block editor (Notion-like)** `[idea]`
  Replace today's plain-markdown `<textarea>` with a real block editor.
  *Recommendation:* adopt **BlockNote** (MIT, ProseMirror-based, bundles offline,
  CSP-safe). Store block-JSON opaquely in SQLite; keep a plaintext projection for
  search/RAG. Verify React 18 compatibility (AlenAI runs it on React 19).
- **F3 — Nested pages + backlinks** `[idea]`
  Arbitrary page nesting (`parent_note_id`) and a **visible backlinks panel**
  (AlenAI notably lacks one — ARYA can do better).
- **F4 — Notion import** `[idea]`
  Import pages/notes from a Notion **Markdown & CSV** export (Notion exports this
  on every plan). Relations/rollups/formulas won't round-trip cleanly; plain
  pages + backlinks do.
- *Deferred for notes MVP:* databases (5 view types), comments, version history,
  templates — the heaviest parts of AlenAI's Notes, least aligned with ARYA's
  focus. Revisit later.

### C. Dictation (the daily driver)

- **F5 — Dictations saved into the brain by default** `[idea]`
  Every dictation is stored as a first-class, connected, searchable node;
  deletable one-at-a-time or all. (Already partly true — indexed into RAG.)
- **F6 — Direct ↔ Polished toggle + tone** `[idea]`
  Easy switch between **Direct** (verbatim, exact words) and **Polished** (AI
  rephrases/cleans). Polished picks a **tone: polite / friendly / professional.**
  (Raw/Clean/Polished modes already exist; tone selection is the new part.)
  *Note:* Polished quality must rival Wispr Flow's — that's their whole moat;
  prototype the local-LLM polish before committing to "exceptional."
- **F7 — Multi-language dictation + meeting minutes** `[researched]`
  Language picker; per-language models. See `docs` memory + §5 below. On-device
  parity is real for ES/DE/IT/PT (≈English), FR slightly behind (fine-tune
  closes it); **Arabic is not at parity** (be honest in UI). Cheapest big win:
  stop forcing `en` + add a language picker (fixes most European languages with
  zero new models — see B1 bug).
- **F8 — Right-click "Translate to…" on a saved dictation** `[idea]`
  Right-click a dictation in history → pick a language → the translation is
  **appended below** the original (original on top, translation below), stored
  **non-destructively** (separate record), **multiple languages stackable**, and
  **indexed for search** (German query finds the German translation). Mirrors the
  existing capture-time side-by-side translation UI. *Relates to:* F16.

### D. Meeting minutes

- **F9 — Keep the differentiators, add the niceties** `[idea]`
  ARYA already wins on bot-free **and** on-device, keeps the audio (Granola is
  text-only), speaker labels, crash-safe recovery. Gaps to consider closing:
  calendar auto-detect/pre-meeting brief, templates, action-item quality,
  cross-meeting chat (ARYA has the substrate: agent + semantic search).

### E. Galaxy (visual second brain)

- **F10 — Knowledge-graph visualization** `[idea]`
  A 2D (later 3D) force-graph of the connected brain. **No competitor has this**
  (Notion needs 3rd-party tools; Wispr/Granola nothing).
  *Architecture:* best leverage of existing substrate — nodes from
  notes/folders/dictations/meetings; edges from the `links` table (mentions) +
  **semantic** cosine over existing `rag_chunks` embeddings (use per-node top-K,
  not a threshold, to avoid "hairballs") + optional AI-suggested (with
  anti-hallucination endpoint validation). Render with `react-force-graph-2d`.
  Assemble on tab-open (no cron); needs local Ollama for the semantic pass;
  graceful fallback to structural+mention edges offline.

### F. Mind Map

- **F11 — Node canvas** `[idea]`
  Draggable nodes/edges, shapes, sticky notes, zoom/pan.
  *Recommendation:* use **React Flow (`@xyflow/react`)**, don't port AlenAI's
  ~1,500-line custom canvas. Gives arbitrary node-to-node edges (which AlenAI's
  mind map lacks), less code, cleaner. Store as opaque JSON; debounce autosave
  (AlenAI's fires a PUT per mousemove — avoid); persist the viewport.
  *Optional:* "turn this note into a mind map" via a one-shot sidecar/Ollama call.

### G. Agent

- **F12 — Multi-line composer** `[idea]`
  The agent input should be a real multi-line box (≥5 lines, like Codex / Claude
  Code). It's a single line today.
- **F13 — Local-first agent as a headline asset** `[idea]`
  Local Ollama models + sandboxed sidecar + per-program approval scoping +
  Trojan-Source/bidi stripping + TTL auto-deny. Under-marketed strength; lean on
  it.

### H. Search

- **F14 — Search everything by title AND content** `[idea]`
  One search across notes, dictations, meetings, and mind maps — matching both
  titles and full content. Semantic search exists (local embeddings); add
  explicit title/content full-text and broaden scope to all node types.

### I. Cross-cutting primitive — "AI actions on content"

Several features are the *same engine* (fetch content → LLM transform → insert
result). Build the primitive once; expose it via multiple entry points.

- **F15 — Inline `@node + instruction`** `[idea]`
  `@xyz translate to German` resolves the node's content, runs the instruction,
  inserts the result inline. Generalizes: `@meeting extract action items`,
  `@a @b compare`, summarize, rewrite. **Translate = verb #1.**
  *Design:* explicit run gesture (⌘↵ or a menu item) — no magic parsing / no
  accidental LLM calls; keep the `@` as a provenance link and insert the result
  as a new block "via @xyz"; local model default, cloud optional.
- **F16 — "Sort" / Organize a brain dump** `[idea]`
  A whole-note command that turns a messy, multi-topic dump into N coherent,
  topic-grouped, well-written paragraphs.
  *Design:* **non-destructive** (preview-and-accept, keep the original, undo);
  **auto-detect topics** (optional "into N sections"); **"reorganize, don't
  invent"** (no new claims — critical for a second brain); local model default.
  *Differentiator:* **ramble (dictate) → Sort** is a thought-capture→structure
  loop none of Notion/Wispr/Granola do end-to-end.

---

## 3. Open product decisions (owner's call)

- **D1 — Positioning (one sentence)** `[decision-pending]`
  Candidates: (a) lead with wedge + moat — *"the open-source, on-device AI
  notetaker for your Mac — bot-free meeting capture and dictation you can audit
  line by line"* (recommended); (b) *"auditable open-source private capture
  tool"*; (c) describe the whole workspace. **Note:** owner has moved to a
  build-and-expand direction; earlier "stop building" strategy framing is set
  aside.
- **D2 — The one best-in-class pillar** `[decision-pending]`
  Recommend **meeting capture + dictation**; define "best" as the **trust/control
  frontier** (bot-free + on-device + open + crash-safe), *not* the raw-accuracy
  leaderboard.
- **D3 — Image-gen stub** `[decision-pending]`
  It's dead in the shipped/proxy config (throws) but always registered
  (tools.ts:163). Recommend **gate registration on availability** so the shipped
  agent doesn't advertise it (dev-key path preserved). Alt: remove entirely / build
  a server endpoint.
- **D4 — Free/Pro/Max tier scaffolding** `[decision-pending]`
  `Tier {Free,Pro,Max}` defaults to **Pro** (billing.rs:82) and leaks "Pro plan ·
  500k credits" into the sidebar (App.tsx:169) — misleading in a free-forever MIT
  app. Recommend **prune the tier labels, keep the credit-metering guard**; freeze
  "Stripe someday."
- **D5 — Shell / 7-tab UX** `[decision-pending]`
  Keep tabs, fix hierarchy: fold **MCP (and maybe Routines) under Agent** (they're
  agent config, not peer apps); lead with Notes + Dictation. **Not** a unified
  dashboard (junk-drawer trap).
- **D6 — Monolith refactor** `[decision-pending]`
  NotesWorkspace (1036) / DictationPanel (578) / AgentPanel (504) / app.css
  (1799). Internal quality; sequence vs feature work TBD.

---

## 4. Known issues / observations to fix

- **B1 — Non-English dictation mistranslates to English** `[verified-bug]`
  Root cause: dictation language is hard-defaulted to `en`
  (`settings.rs:72`), forcing Whisper to treat all speech as English. It then
  emits approximate English for Croatian/Spanish; acoustically-strong German
  sometimes breaks through (→ German), short German complies (→ English) — hence
  the inconsistency the owner observed. **Not** the translate feature (off by
  default). Fix: stop forcing `en`; add a language picker (default auto-detect).
  The default model is already multilingual (turbo). *Relates to:* F7.

---

## 5. Competitive positioning (researched 2026-07-08, sourced)

Each competitor solves **one** of ARYA's three pillars; none is on-device or
open-source.

- **Notion** = notes + databases + team wiki. **No native graph view.** Dictation
  is only cloud AI voice input; meetings are a paywalled ($20/seat) bot-free
  add-on with cloud transcription. Cloud-only, closed-source, Electron.
- **Wispr Flow** = personal dictation only (best-in-class AI polish, but
  **over-edits** → verbatim suffers). **Not** a meeting tool, no notes.
  Cloud-only, no offline, closed-source, **trains on individual users' data by
  default.** 100+ languages, tone is English-only. $15/mo.
- **Granola** = bot-free meetings only. **Text-only (no audio playback).** "Bot-
  free" is a *consent* feature, not privacy — audio still streams to cloud ASR
  (Deepgram/AssemblyAI) + LLMs. Closed-source, no offline, trains on de-identified
  data by default, now encrypts its local DB so you can't read your own notes.
  No dictation, no Android/web. $14–35/seat.

**ARYA's durable differentiators:** (1) all three pillars in one; (2) on-device +
open-source/auditable (only one); (3) voice → permanent *connected* knowledge,
not throwaway output; (4) you own it, it's free, never trains on you, keeps your
audio. **Honest gaps:** multi-language breadth (esp. Arabic), Polished-quality
parity with Wispr, editor/database depth vs Notion, Mac-only, meeting niceties
(calendar/templates) vs Granola.

---

## 6. Technical notes for building

- **Substrate:** Tauri 2 + Rust core, React 18 frontend, TypeScript sidecar (AI
  brain, Ollama + cloud proxy), `arya-api` Rust proxy. Local SQLite (`sqlx`,
  migrations). Local embeddings (nomic-embed-text 768-dim in `rag_chunks`) +
  brute-force `vecmath::cosine`. Established wiring: migration → Rust
  `#[tauri::command]` → `src/lib/*.ts` invoke wrapper → `src/<feature>/Panel.tsx`
  → tab in App.tsx + icon → async via `emit`/`listen`.
- **ASR:** two engines — whisper.cpp (Metal, final transcript) + sherpa-onnx
  (streaming preview, currently **English-only** zipformer). Model catalog
  (`speech/models.rs`) with SHA-256-pinned downloads — adding a model = one
  `ModelSpec`. Default model = multilingual `large-v3-turbo`. Language is already
  a parameter (`TranscribeOptions.language`), just defaulted to `en`.
- **Multilingual model strategy** (ship MIT/Apache-2.0/CC-BY-4.0 only): keep
  turbo as default; the one high-value add is **NVIDIA Parakeet-TDT-0.6B-v3**
  (CC-BY-4.0, one model covers 25 EU languages, beats Whisper on DE/FR, sherpa-onnx
  export exists) — better than five per-language packs. Specialist Whisper
  fine-tunes only for **German** (primeline, Apache-2.0, 3.0%) and **French**
  (bofenghuang, MIT); both ship GGML → ride the whisper.cpp path. Arabic = a
  Whisper-*medium* fine-tune, best-effort. **Exclude** MMS / SeamlessM4T /
  original-Canary-1B / SenseVoice / Moonshine-non-English (NC or ambiguous).
  *Caveat:* live streaming preview stays English-only for now (Parakeet is
  offline); multilingual users get the whisper-ticker preview.
- **AI-transform primitive (F15/F16):** one-shot sidecar LLM RPC (template =
  existing image/agent calls): `{sourceText, instruction, targetLang?} →
  resultText`. Local Ollama default; cloud optional. Non-destructive output;
  "reorganize/translate, don't invent" guard.
- **Ship gate (real):** signing + notarization + updater keys
  (`docs/production-readiness.md`) — not billing.

---

## 7. Later / v2 (explicitly deferred)

- **V1 — Google Calendar integration** `[later/v2]`
- **V2 — Gmail integration** `[later/v2]`
- **V3 — Custom SMTP email** `[later/v2]`

---

## 8. Reference material produced this session

Deep-dives that informed the above (detail lives in the session; key conclusions
captured here and in memory):
- AlenAI **Galaxy / Mind Map / Notes** module analyses (capabilities +
  architecture) — the basis for F2–F4, F10, F11.
- ARYA architecture map (the substrate in §6).
- Competitive research on **Notion / Wispr Flow / Granola** (§5).
- Local multilingual **ASR model landscape** (§6, memory: `multilingual-dictation`).
