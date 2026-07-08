# Arya

**A private AI workspace for the Mac.** Arya combines an AI chat assistant, a
system-wide voice dictation tool, and a bot-free meeting notetaker into one
native desktop app — with a local AI agent that can work across all of it.

Most AI apps send everything you do to the cloud. Arya is built the other way
around: your notes, recordings, and files stay on your Mac, and speech
(transcription and dictation) happens on-device. Cloud AI is optional — free
local models cover most of it.

> Status: early (v0.1.x), macOS only. This is a working desktop app under
> active development, released as open source.

## Features

- **Notes** — record a meeting (mic and/or system audio) with no bot joining,
  and get a clean, structured note with a timestamped, speaker-labeled
  transcript. Crash-safe recording with interrupted-session recovery.
- **Dictation** — hands-free voice typing into any app, with on-device
  streaming transcription and an optional AI "polish" pass (Raw / Clean /
  Polished), surfaced through a floating command pill.
- **Agent** — an AI chat that can also act: read files, run tools, and search
  your captured content, with explicit allow-once / allow-for-session / deny
  approval prompts for anything risky. Runs in a sandboxed sidecar.
- **Search** — semantic search across everything you've captured (notes,
  transcripts, dictations), backed by a local index.
- **Routines** — scheduled agent tasks that run on an interval.
- **MCP servers** — connect external tools via the Model Context Protocol.
- **Local-first, cloud-optional** — free local models (via Ollama) are
  first-class; cloud providers are a clearly-labeled, metered upgrade.

## Architecture

| Component      | Path         | Stack                                              |
| -------------- | ------------ | -------------------------------------------------- |
| Desktop shell  | `src-tauri/` | Tauri 2 + Rust (audio, ASR, storage, agent host)   |
| Frontend       | `src/`       | React 18 + TypeScript + Vite                       |
| Agent sidecar  | `sidecar/`   | TypeScript on the AI SDK (spawned per write-mode)  |
| API proxy      | `arya-api/`  | Rust — metered, OpenAI-compatible provider proxy   |

On-device speech uses whisper; local LLMs run through Ollama. The `arya-api`
crate is an optional server-side proxy that keeps provider keys off the client
and meters cloud usage.

## Building from source

Requires a recent Rust toolchain, Node.js, and [pnpm](https://pnpm.io).

```bash
pnpm install
pnpm tauri:dev          # run the app in development
pnpm package:mac        # build a distributable .app / .dmg
```

Verify the full workspace (frontend, Rust core, sidecar, arya-api):

```bash
make verify
```

Configure the optional API proxy by copying `arya-api/.env.example` to
`arya-api/.env` and filling in your own values. **No secrets or provider keys
are committed to this repository** — the example file contains placeholders
only.

## License

[MIT](LICENSE) © 2026 Alen Mikic
