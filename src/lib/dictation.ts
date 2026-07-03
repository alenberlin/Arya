import { invoke } from "@tauri-apps/api/core";

export type ActivationMode = "push-to-talk" | "toggle";
export type DictationStyle = "standard" | "casual-lowercase" | "formal";

export interface DictationSettings {
  shortcut: string;
  mode: ActivationMode;
  style: DictationStyle;
  language: string | null;
  microphone: string | null;
  speechModel: string;
  cleanupModel: string | null;
  ollamaUrl: string;
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
export const getDictationStatus = () => invoke<DictationStatus>("dictation_status");
export const openAccessibilitySettings = () => invoke<void>("open_accessibility_settings");
export const listDictationHistory = () => invoke<HistoryItem[]>("list_dictation_history");
export const deleteDictationHistoryItem = (id: string) =>
  invoke<void>("delete_dictation_history_item", { id });
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
