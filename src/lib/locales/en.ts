// English source catalog. Pulled out so it's obvious which strings are
// "owned" by the i18n system vs. one-off hardcoded text. Keep keys dotted-
// pathed by surface (notes.*, agent.*, dictation.*, common.*).

import type { Catalog } from "../i18n";

export const en: Catalog = {
  // Common / shared chrome
  "common.cancel": "Cancel",
  "common.save": "Save",
  "common.delete": "Delete",
  "common.close": "Close",
  "common.dismiss": "Dismiss",
  "common.search": "Search",
  "common.loading": "Loading…",

  // Notes pane
  "notes.empty.noFilter": "No notes yet. Press Record to capture one.",
  "notes.empty.filtered": "No notes match your filter.",
  "notes.list.title": "Notes",
  "notes.list.filterPlaceholder": "Filter notes by title or content…",
  "notes.list.deleteAll": "Delete all notes",
  "notes.list.new": "New note",
  "notes.list.deleteConfirmTitle": "Delete this note?",
  "notes.list.deleteConfirmMessage": "This permanently removes the note and its transcript.",
  "notes.list.deleteAllConfirmTitle": "Delete all notes?",
  "notes.list.deleteAllConfirmMessage":
    "This permanently deletes every note, along with its transcript and attachments. This cannot be undone.",
  "notes.editor.addSubpage": "Add sub-page",
  "notes.editor.translate": "Translate",
  "notes.editor.brainDump": "Brain dump",
  "notes.editor.folders.sort": "AI sort into folders",
  "notes.editor.folders.sortApplying": "Sorting notes into folders…",
  "notes.editor.folders.sortReviewTitle": "Suggested folder moves",

  // Dictation pane
  "dictation.recording": "Recording",
  "dictation.translating": "Translating to {lang}…",
  "dictation.translated": "Translated to {lang}.",
  "dictation.streamingPreview": "Live streaming preview",
  "dictation.lowPower.title": "Low-power mode",
  "dictation.lowPower.description":
    "Skip background KB reindex, idle speech engines sooner, and prefer CPU inference. Helpful on older Macs or when running on battery.",
  "dictation.noSettingsYet": "Loading dictation settings…",

  // Agent pane
  "agent.empty.title": "Create a session to chat with the agent.",
  "agent.empty.subtitle": "Local models are free and private.",
  "agent.cloudNote": "Cloud model — your prompt is sent to the provider via Arya",
  "agent.localNote": "Running locally — nothing leaves your Mac",
  "agent.model.label": "Model",
  "agent.recent.label": "Recent",
  "agent.newChat": "New chat",

  // Routines / Security / MCP — present but not exhaustive
  "routines.title": "Routines",
  "security.title": "Security & data",
  "mcp.title": "Model Context Protocol",
};