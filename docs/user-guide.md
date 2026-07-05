# Arya — quick start guide

Arya is a private AI workspace for your Mac. Everything is stored locally, and
speech (transcription, dictation, diarization) runs on-device. Cloud AI models
are optional; local models (via Ollama) are free and work offline.

The left sidebar has six pillars plus your account. Here's what each does.

---

## Notes — meeting notes & recording

Your capture-and-notes workspace.

- **Record** — starts a microphone recording. When you stop, Arya transcribes
  the audio on-device, orders it into conversation turns, and writes a
  structured markdown note. First use asks for **Microphone** permission.
- **Meeting** — records the microphone *plus* system audio (the other side of
  a call), so it captures the whole conversation without a bot joining. Needs
  the **Screen & System Audio Recording** permission. If that's not granted it
  quietly falls back to microphone-only.
- **Automatic meeting detection** — when Zoom, Teams, Meet, or a browser starts
  using your mic, a banner offers to record. A calendar event that overlaps
  auto-titles the note and lists attendees (needs **Calendar** access).
- **Live preview** — while recording, a rolling transcript preview appears in
  the note (it's ephemeral; the real transcript is written when you stop).
- **The note** — each note has a title, a "Manual notes" box (type your own
  notes during the meeting; they're folded into the generated note), the
  AI-generated body, and an expandable **Transcript** with timestamps and
  speaker labels.
- **Speaker names** — turns are labeled by voice. Enroll voices under
  **Dictation → Voice profiles** and Arya uses real names instead of "Speaker 1".
- **Folders** — group notes. Use "All notes" or a folder to filter; "+ Folder"
  to make one.
- **Recovery** — if the app is killed mid-recording, a banner offers to
  **Recover** the audio into a note (nothing is lost) or **Discard** it.
- **Retry** — if a note fails to process, open it and press Retry; it re-runs
  from the saved audio.

---

## Agent — chat and a local AI agent

A conversational AI that can also *do* things on your Mac.

- **New session** — pick a model from the dropdown. Local models (Ollama) are
  labeled "local, free" and run offline; cloud models appear if the backend is
  configured with keys.
- **Chat** — type and send. Responses stream in, with the model's reasoning
  shown in an expandable block.
- **Tools** — the agent can read and write files in its own workspace, run
  shell commands, generate images, and **search your workspace** (your notes,
  transcripts, and dictations). Tool activity shows inline.
- **Approvals** — risky actions (running a command, writing outside the safe
  area) pause and ask you: **Allow once**, **Allow for session**, or **Deny**.
  Nothing risky happens without your say-so.
- **Sandbox** — by default the agent runs in a kernel-enforced write-jail: it
  can only write inside its workspace, never the rest of your disk.
- **Steer / Stop** — while it's working you can type a nudge and press Steer to
  redirect it, or Stop to halt.
- **Branch** — "Branch here" on any message forks the conversation from that
  point so you can explore an alternative.
- **/image** — type `/image a red bicycle` in the composer to generate an
  image directly (needs a cloud image model configured).

---

## Search — ask across your whole workspace

Semantic search over everything you've captured.

- **Reindex** — build (or rebuild) the local search index over your notes,
  meeting transcripts, dictation history, and agent sessions. Run this once,
  and again after you've added a lot of new content.
- **Search** — type a natural-language question ("when's the budget meeting?",
  "what did we decide about the launch?"). It finds passages by *meaning*, not
  just keywords, and ranks them with the source they came from.
- Fully on-device (uses your local embedding model). The agent uses this same
  index through its search_workspace tool to ground its answers in your data.

---

## Routines — scheduled agent tasks

Have the agent run something on a repeating schedule.

- **Add a routine** — give it a title, the prompt you want the agent to run, a
  model, and an interval ("every 60 minutes").
- Arya runs it automatically on that interval, starting a fresh agent session
  each time.
- **History** shows each run and its result. **Pause/Resume** to toggle it,
  **Delete** to remove it.
- Example: "Every morning, summarize yesterday's meeting notes" (on a local
  model, this costs nothing).

---

## MCP servers — connect external tools

MCP (Model Context Protocol) servers are external programs that give the agent
extra tools — a filesystem browser, a web API, a database, etc.

- **Add a server** — enter a name, the command to launch it, and any arguments
  (for example `npx` with the args for a published MCP server). Arya asks you
  to confirm before launching it, since it's an external process that gains
  tool access to the agent.
- Once connected, its tools appear to the agent (namespaced), and — like the
  built-in tools — calling them pauses for your approval.
- **Remove** disconnects and forgets the server.
- Only add servers you trust; their tools can act on your behalf.

---

## Dictation — talk, and it types for you

System-wide voice-to-text that works in any app.

- **How** — hold the hotkey (default **Ctrl + Alt + D**) anywhere, speak, and
  release. Arya transcribes on-device, cleans it up, and pastes the text into
  whatever app you're in. Needs the **Accessibility** permission to paste.
- **Modes** — *push-to-talk* (hold the key) or *toggle* (press once to start,
  again to stop).
- **Styles** — Standard, Casual lowercase, or Formal (expands contractions).
  Cleanup preserves your words — it fixes filler, punctuation, and casing but
  never rewrites or summarizes.
- **Email-aware** — when you dictate into Mail/Outlook/etc., it lays the text
  out as an email.
- **Dictionary** — add "heard as → replace with" entries so names and jargon
  come out spelled correctly.
- **History** — every dictation is saved (and searchable); delete any entry.
- **Voice profiles** — enroll a voice (speak for ~6 seconds) so meeting
  transcripts label that person by name.
- **Offline** — with a local cleanup model configured it works with no
  network at all.

---

## Account & appearance

- **Account** (sidebar footer) — your plan and credit balance. Local models and
  on-device speech never use credits. Cloud usage would meter credits once the
  backend and billing are configured; in local mode you're always signed in.
- **Theme** (sidebar footer) — System, Light, or Dark. "System" follows macOS.

---

## Permissions Arya may ask for

All optional; each pillar degrades gracefully without its grant.

| Permission | Enables | When asked |
|---|---|---|
| Microphone | Recording & dictation | First recording/dictation |
| Accessibility | Dictation pasting into apps | Dictation settings → Open System Settings |
| Screen & System Audio Recording | Meeting mode (system audio) | First meeting recording |
| Calendar | Meeting titles & attendees | Calendar prompt |

## What's local vs. cloud

- **Always local & free:** recording, transcription, diarization, dictation,
  workspace search, and any agent/notes work done with an Ollama model.
- **Cloud (optional):** frontier chat/agent models and image generation, routed
  through the open-source proxy that holds the keys — none ship in the app.
