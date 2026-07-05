# Arya — product brief for a UI/UX designer

A hand-off document for a designer who has never seen this software. It
explains what Arya is, who it's for, what it does, and — most importantly — the
surfaces, states, and interaction moments you'll need to design.

---

## 1. In one line

**Arya is a private AI workspace for the Mac** that combines an AI chat
assistant, a system-wide voice dictation tool, and a bot-free meeting
notetaker into one native desktop app — with a local AI agent that can work
across all of it.

## 2. The 30-second pitch

Most AI apps send everything you do to the cloud. Arya is built the other way
around: your notes, recordings, and files stay on your Mac, and speech
(transcription, dictation) happens on-device. It replaces three separate
subscriptions — a ChatGPT-style assistant, a dictation app, and a meeting
notetaker — with one calm, private, native tool, and adds an AI agent that can
act on your files and search everything you've captured. Cloud AI is optional;
free local models cover most of it.

## 3. Who it's for

Privacy-conscious professionals who live in meetings and documents — the kind
of person who currently juggles a notetaker, a dictation tool, and an AI chat
app. Comfortable on a Mac, values things that feel native and quiet rather than
flashy. Not a developer tool, not a consumer toy — a **premium productivity
app**.

## 4. The mental model (design around this)

Think of Arya as **one workspace with a few tools down the left side**, not a
collection of separate apps. The unifying idea is: *you capture things
(meetings, dictations, notes), and an AI helps you across all of it, privately.*
Everything the user creates is theirs, stored locally, and searchable.

The emotional target is **calm confidence**: it handles sensitive material
(private meetings, personal notes), so the UI should feel trustworthy,
unhurried, and in control — never noisy, gamified, or attention-grabbing.

## 5. Platform & form factor

- **macOS desktop application** (not web, not mobile). It should feel like a
  first-class Mac app — think the polish of Things, Linear's desktop app, or
  Craft.
- A **resizable main window** with a persistent left sidebar.
- Two small **floating overlays** exist too (see §8): a dictation "pill" and a
  meeting-detection prompt.
- **Light and dark themes**, following the system by default.

## 6. What a user actually does (the core jobs)

1. **Record a meeting** and get a clean, structured note without a bot joining.
2. **Dictate** by voice into any app, hands-free.
3. **Chat with an AI agent** that can work with their files and their own notes.
4. **Search everything** they've captured, by meaning.
5. **Automate** small recurring AI tasks.

## 7. The main surfaces to design (six pillars + account)

These are the left-sidebar destinations. Each is a full screen.

### Notes (the primary/home surface)
The meeting-notes workspace. Contains:
- A **recorder control** (Record, or Meeting for mic + system audio; then a
  live Stop with an elapsed timer, plus pause/resume).
- A **list of notes** (with per-note status: recording, transcribing,
  generating, ready, failed), organized into **folders**.
- A **note editor**: title, a "manual notes" area the user types into during a
  meeting, the AI-generated note body, and an expandable **transcript** with
  timestamps and speaker labels.
- Contextual **banners**: "interrupted recording found — recover / discard,"
  "meeting detected in Zoom — record it," "an event starts in 5 min," "system
  audio unavailable." These are important, occasional, and must feel calm, not
  alarming.
- A **live transcript preview** that appears while recording.

### Agent
An AI chat that can also perform actions:
- A **session list** (past conversations) and a **model picker** (each model
  tagged local/free or cloud).
- A **conversation view**: streaming messages, an expandable "reasoning" block,
  and **tool-activity rows** (the agent reading files, running commands,
  searching).
- **Approval prompts**: when the agent wants to do something risky it pauses
  and asks — *Allow once / Allow for session / Deny*. This is a signature,
  trust-critical moment; design it clearly.
- A **composer** with a send button, plus mid-run **Steer** and **Stop**
  controls while it's working, and a "branch from here" affordance on messages.

### Search
Semantic search across all captured content:
- A **search box**, a **Reindex** action, and a small **status line** (how many
  items indexed, whether the local engine is ready).
- **Ranked results**, each showing the source (a note, a transcript, a
  dictation) and a relevance score.

### Routines
Scheduled agent tasks:
- A **form** to create one (title, prompt, model, "every N minutes").
- A **list** of routines with enabled/paused state, and an expandable **run
  history** per routine.

### MCP servers
A power-user screen to connect external tools:
- A **form** (name, command, arguments) with a confirm-before-launch step.
- A **list** of connected servers with a remove action.
- Should read as advanced/optional without feeling scary.

### Dictation
Settings + history for voice typing:
- **Settings**: the hotkey, mode (push-to-talk / toggle), writing style, mic
  device, language.
- A **voice-profiles** section (enroll a voice for meeting speaker names).
- A **custom dictionary** (word replacements).
- A **history** list of past dictations.
- A **permission banner** when accessibility isn't granted.

### Account (sidebar footer)
Plan tier, credit balance and usage, upgrade/top-up, sign out. In local mode
this is minimal. Also the **theme** switch.

## 8. Special surfaces beyond the main window

- **Dictation pill** — a small always-on-top "command pill" that appears while
  dictating: live audio levels, the streaming transcript, and inline controls
  for mode, AI polish, and destination, plus a theme dot. Small, glanceable,
  draggable. It's a signature surface — see **Appendix A** for the full
  interaction spec.
- **Meeting prompt** — a floating prompt when a meeting is detected, offering
  to record.
- **Onboarding** — a short first-run flow (welcome → the privacy model →
  granting permissions → try dictation once → done). One idea per screen,
  calm, skippable.
- **Menu-bar / tray** presence for quick actions.

## 9. The UX moments that make or break it

Flag these to the designer as the hard, high-value parts:
- **Approval prompts** (agent asking permission) — must be instantly
  understandable and never feel like a nag.
- **Live/streaming states** — recording in progress, live transcript preview,
  the agent streaming a reply token by token.
- **Long-running & background work** — processing a recording, a routine
  firing, indexing. Needs clear, quiet status (badges, subtle progress), never
  a blocking spinner.
- **Recovery & failure** — "we found an interrupted recording," "processing
  failed, retry." Reassuring, not scary.
- **Permission priming** — asking for mic/accessibility/screen-recording at the
  right moment with a clear reason.
- **Empty states** — every list (notes, sessions, search, routines) needs a
  helpful first-run empty state.

## 10. Information architecture

- Persistent **left sidebar**: the six pillars, with Account + theme pinned to
  the bottom.
- The active pillar fills the rest of the window (many pillars use a
  **two-column split**: a list/controls column on the left, a detail/editor on
  the right).
- No deep nesting; it should feel flat and immediate.

## 11. Tone, brand & aesthetic direction

- **Calm, warm, professional, native-Mac.** References: Granola, Linear,
  Fellow, Craft, Things.
- Current direction (feel free to evolve): a **warm neutral palette** (stone
  greys on a soft cream ground) with a single **clay/terracotta accent**, an
  "inset card" layout (tinted sidebar, clean white content card), generous
  whitespace, subtle borders and soft shadows rather than heavy chrome.
- **Sentence case** everywhere (never ALL CAPS). Clean sans-serif (Inter-class)
  for UI, a monospace only for timestamps/technical bits.
- **No emoji as icons** — a single consistent line-icon set.
- Motion is subtle and meaningful (150–300ms), never decorative.

## 12. Principles & constraints

- **Privacy is the product** — the UI should continually reassure: "local,"
  "on-device," "private," model privacy tiers, "your data stays here." This is
  the brand, not a footnote.
- **Local-first, cloud-optional** — free local models are first-class, not a
  downgrade. Cloud is a clearly-labeled upgrade.
- **Accessible** — full keyboard support, visible focus, 4.5:1 contrast in both
  themes, respects reduced motion.
- **Quiet by default** — background work shouldn't shout; the app should feel
  like it's doing a lot without demanding attention.

## 13. Out of scope (don't design these for v1)

Windows/mobile/web versions, team collaboration or sharing, public content, a
marketing site. It's a single-user, single-Mac, private desktop app.

---

## How to use this brief

Give the designer §1–§4 for understanding, §7–§10 for the actual screens and
states to lay out, and §11–§12 for the look and rules. Ask them to design, at
minimum: the **sidebar + shell**, the **Notes** screen (list + editor +
recorder + banners), the **Agent** conversation (with the approval prompt and
tool rows), and the **Search** screen — those four carry the product. The rest
reuse the same components.

---

## Appendix A — the dictation pill (command pill) interaction spec

The pill is the face of dictation: the one surface a user watches while
speaking, so it has to feel instant, calm, and trustworthy. This appendix
specifies its anatomy, its single interaction gesture, and every state. Color
and type follow §11 — the warm cream ground and clay accent, a **Newsreader
serif for the transcript** so the words read as "voice," and sans for the
controls.

### When it appears

Floating, always-on-top, bottom-center of the screen, and only while a
dictation is active (the backend shows and hides it). It's **draggable**, and
it **grows in place** — upward as the transcript streams, downward as a control
expands. It is never a fixed size. When dictation ends it lingers briefly with
the final text, then fades.

### Anatomy (collapsed)

The control row, left to right: a small round **brand mark** that softly pulses
as a "listening heartbeat"; a **live waveform** (a few clay bars reacting to
input level); a **mode** chip; an **AI-polish** chip; a spacer; a **theme dot**;
and a **destination** chip. Below the row, the **streaming transcript** in
serif. In hands-free mode a **Stop** button and an **✕** (cancel) join the right
of the control row.

### The one gesture (the core idea)

Every control chip behaves identically, so there is nothing to learn:

- **Tap a chip** → it advances to the next value (a quick cycle) *and* the pill
  expands in place to reveal the whole set, with the new value highlighted. Keep
  tapping to keep cycling; the set stays visible so you always see where you are.
- **Tap a segment** in the revealed set → jumps straight to that value and
  collapses.
- The panel also collapses when you **start speaking** again or **tap the pill
  body**.

This unifies two instincts — the speed of a one-tap toggle and the
discoverability of a menu — into a single motion. It also fits the medium: a
floating overlay can't cleanly draw a dropdown outside its own window, so the
pill **grows in place** rather than opening a clipped popover.

### The controls

- **Mode** — for v1, **Dictate** only. *Command* (voice control) and *Rewrite*
  (edit in place) join the same cycle when they ship; the chip is built to hold
  them from day one, so adding them is no re-layout. Do **not** show them as
  disabled "coming soon" options.
- **AI polish** — three levels: **Raw** (verbatim — for code or the terminal),
  **Clean** (light mechanical tidy: filler removal, punctuation, casing — the
  fast, sub-second default), and **Polished** (a fuller AI rewrite). When
  Polished is selected, the expanded panel also offers an optional one-line
  **prompt** ("make it concise," "friendly tone"). This is the only control with
  a sub-option, and it's what makes the reveal earn its keep.
- **Destination** — shows which app the text will land in (icon + name), and
  doubles as a **tone signal**: the app implies how Clean/Polished will write
  (formal for mail, casual for chat). Informational for v1; a future "pin" could
  let the user override the target.
- **Theme dot** — a small dot that cycles **Auto → Light → Dark**. *Auto* adapts
  the pill's light/dark treatment to the app behind it so it stays legible
  anywhere; Light and Dark are manual pins. One click, the same cycle idiom as
  the chips.

### Per-app defaults (the "knows your world" behavior)

The pill doesn't start blank — it reflects the **resolved settings for whatever
app you're dictating into** (chat → Clean + casual, mail → Polished + formal).
Changing a chip applies to **the current dictation only**; an optional **pin**
makes it that app's new default going forward. This is a deliberate
differentiator: Arya already knows the user's context, so the pill should feel
pre-tuned — without silently rewriting settings on every quick tweak.

### The transcript reveal (settling)

As the user speaks, **interim words** arrive in a muted, italic treatment with a
blinking caret, then **settle** into solid ink as the recognizer finalizes them.
This makes the streaming visible and trustworthy — you can watch the words being
heard and locked in, in real time. (It depends on true streaming recognition; a
laggy re-transcription would undercut it.)

### Crowding rule

Space is tight in a ~320–440px pill. When the transient **Stop/✕** controls
appear (hands-free), the chips **collapse to icon-only** so everything fits on
one row without wrapping into a "mixing desk." Labels return when the controls
leave.

### States

- **Preparing model** — first use may need to load a model; show a quiet
  "preparing…" rather than a dead pill.
- **Listening** — heartbeat pulses, waveform reacts, transcript streams with the
  settling treatment.
- **Transcribing** — the brief moment after you stop while the tail finalizes;
  keep it near-instant.
- **Inserting** — the cleaned text is being pasted into the target app.
- **Done** — shows the final inserted text briefly, then fades.
- **Error** — a calm treatment (mic unavailable, nothing recognized);
  reassuring, not alarming, per §9.
- **Hands-free** — adds Stop/✕; chips go icon-only.

### Accessibility & motion

Full keyboard reachability and visible focus on every chip, segment, and the
dot; **4.5:1 contrast in both light and dark**; respect **reduced motion** (the
heartbeat, waveform, and settling quiet down). Because the pill is draggable,
the controls must **capture the press so they never trigger a drag**. Motion
follows §11 — subtle, 150–300ms: the pill's grow/collapse animates height, and
theme changes cross-fade.
