import { invoke } from "@tauri-apps/api/core";

export type ActivationMode = "push-to-talk" | "toggle";
export type DictationStyle = "standard" | "casual-lowercase" | "formal";
/** How much cleanup a dictation gets: verbatim / mechanical / local-LLM. */
export type Polish = "raw" | "clean" | "polished";
/** Which engine translates: local Ollama or cloud (Arya API). */
export type TranslateProvider = "local" | "cloud";

export interface DictationSettings {
  shortcut: string;
  mode: ActivationMode;
  style: DictationStyle;
  polish: Polish;
  language: string | null;
  microphone: string | null;
  speechModel: string;
  /** Stream the live preview via the online engine instead of whisper re-runs. */
  streaming: boolean;
  cleanupModel: string | null;
  ollamaUrl: string;
  /** Target language for translation (e.g. "German"); null = off. */
  translate: string | null;
  translateProvider: TranslateProvider;
  /** Ollama model for local translation; null = auto (cleanup model or default). */
  translateModel: string | null;
}

export interface DictationStatus {
  accessibilityTrusted: boolean;
  recording: boolean;
  inputDevices: string[];
}

export interface HistoryItem {
  id: string;
  rawText: string;
  cleanText: string;
  translatedText: string | null;
  targetLang: string | null;
  appBundleId: string | null;
  durationMs: number;
  asrMs: number;
  createdAt: string;
}

export interface DictionaryItem {
  id: string;
  pattern: string;
  replacement: string;
}

export const getDictationSettings = () => invoke<DictationSettings>("get_dictation_settings");
export const setDictationSettings = (settings: DictationSettings) =>
  invoke<void>("set_dictation_settings", { settings });
/** Installed Ollama model names, for the translation-model picker. */
export const listOllamaModels = () => invoke<string[]>("list_ollama_models");
export const getDictationStatus = () => invoke<DictationStatus>("dictation_status");
export const openAccessibilitySettings = () => invoke<void>("open_accessibility_settings");
export const listDictationHistory = () => invoke<HistoryItem[]>("list_dictation_history");
export const deleteDictationHistoryItem = (id: string) =>
  invoke<void>("delete_dictation_history_item", { id });
export const clearDictationHistory = () => invoke<void>("clear_dictation_history");
/** Stops a hands-free dictation (the pill's Stop button): transcribe + insert. */
export const dictationStop = () => invoke<void>("dictation_stop");
/** Cancels the current dictation (the pill's ✕): discard, no insert. */
export const dictationCancel = () => invoke<void>("dictation_cancel");
/** Sets a one-off polish level for the current dictation (not persisted). */
export const dictationSetSessionPolish = (polish: Polish) =>
  invoke<void>("dictation_set_session_polish", { polish });
/** Pins the given polish as the default for the app the dictation targets. */
export const dictationPinApp = (polish: Polish) => invoke<void>("dictation_pin_app", { polish });
/** Removes the pin for the app the current dictation targets. */
export const dictationUnpinApp = () => invoke<void>("dictation_unpin_app");
/** Downloads and loads the streaming model so the live preview is ready. */
export const dictationPrepareStreaming = () => invoke<void>("dictation_prepare_streaming");
/** Copies text to the system clipboard. */
export const copyToClipboard = (text: string) => invoke<void>("copy_to_clipboard", { text });
/** Generates a meeting-minutes note from a dictation; returns the new note id. */
export const convertDictationToNote = (id: string) =>
  invoke<string>("convert_dictation_to_note", { id });
export const listDictionaryEntries = () => invoke<DictionaryItem[]>("list_dictionary_entries");
export const createDictionaryEntry = (pattern: string, replacement: string) =>
  invoke<DictionaryItem>("create_dictionary_entry", { pattern, replacement });
export const deleteDictionaryEntry = (id: string) =>
  invoke<void>("delete_dictionary_entry", { id });

export interface SpeakerProfile {
  id: string;
  name: string;
  createdAt: string;
}

export const enrollSpeakerProfile = (name: string, seconds?: number) =>
  invoke<SpeakerProfile>("enroll_speaker_profile", { name, seconds: seconds ?? null });
export const listSpeakerProfiles = () => invoke<SpeakerProfile[]>("list_speaker_profiles");
export const deleteSpeakerProfile = (id: string) => invoke<void>("delete_speaker_profile", { id });
