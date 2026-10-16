// Pure formatting / presentation helpers used across the notes UI. Pulled out
// of `NotesWorkspace` so it can focus on composition + state. None of these
// touch React state, Tauri APIs, or anything beyond their inputs.

import type { JSX } from "react";

export const STATUS_LABEL: Record<string, string> = {
  idle: "",
  recording: "Recording",
  transcribing: "Transcribing…",
  generating: "Generating…",
  ready: "",
  failed: "Failed",
};

/** Short type badge from a filename (e.g. "PDF"). */
export function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0
    ? name
        .slice(dot + 1)
        .toUpperCase()
        .slice(0, 4)
    : "FILE";
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Static set of bar heights/timings for the live recording waveform. */
export const WAVE = [
  { h: "70%", d: "0s", t: "0.9s" },
  { h: "100%", d: "0.1s", t: "1.1s" },
  { h: "50%", d: "0.05s", t: "0.8s" },
  { h: "85%", d: "0.15s", t: "1s" },
  { h: "40%", d: "0.2s", t: "0.95s" },
  { h: "65%", d: "0.08s", t: "1.05s" },
  { h: "90%", d: "0.12s", t: "0.85s" },
  { h: "55%", d: "0.04s", t: "1.15s" },
];

export function formatElapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

export function formatWhen(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function statusChip(status: string): JSX.Element | null {
  if (status === "recording") {
    return (
      <span className="status-chip status--rec note-list-status">
        <span className="dot-pulse" />
        REC
      </span>
    );
  }
  if (status === "transcribing" || status === "generating") {
    return (
      <span className="status-chip status--processing note-list-status">
        {STATUS_LABEL[status]}
      </span>
    );
  }
  if (status === "failed") {
    return <span className="status-chip status--failed note-list-status">Failed</span>;
  }
  return null;
}

/** F16: turn a messy, multi-topic brain dump into coherent, grouped sections. */
export const SORT_INSTRUCTION =
  "Reorganize this text into a few coherent, well-written sections, each focused " +
  "on a single topic. Group related points together and give each section a short " +
  "heading. Preserve every idea, but do not add any information that is not already " +
  "in the text.";

/** Auto-title: name an untitled note from its content once there's enough to
 * go on. Never invents facts — just names what's already there. */
export const AUTO_TITLE_INSTRUCTION =
  "Read this note and reply with ONLY a short, specific title for it (3-7 " +
  "words) that captures what it's actually about. No quotes, no punctuation " +
  "at the end, no preamble, no explanation — just the title text itself.";

/** Below this many characters of body markdown, there isn't enough context
 * yet for a meaningful title — wait for more before asking the model. */
export const AUTO_TITLE_MIN_CHARS = 24;
export const AUTO_TITLE_DEBOUNCE_MS = 2500;

/** Strip quoting/trailing punctuation a model tends to add despite being
 * asked not to, and cap length so a rambling reply can't become the title. */
export function sanitizeAutoTitle(raw: string): string {
  const cleaned = raw
    .trim()
    .replace(/^["'“”‘’]+|["'“”‘’]+$/g, "")
    .replace(/[.!?]+$/, "")
    .replace(/\s+/g, " ")
    .trim();
  return cleaned.length > 80 ? `${cleaned.slice(0, 79)}…` : cleaned;
}

/** How many folder chips sit inline before the rest fold into "More". */
export const MAX_VISIBLE_FOLDERS = 3;
