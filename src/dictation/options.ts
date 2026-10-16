// Static option lists shown in the dictation-settings UI. Kept out of the
// component file so they're trivially unit-testable and easy to localize.

import type { DictationSettings } from "../lib/dictation";

export const STYLES: { value: DictationSettings["style"]; label: string }[] = [
  { value: "standard", label: "Standard" },
  { value: "casual-lowercase", label: "Casual" },
  { value: "formal", label: "Formal" },
];

// Polished-rewrite tone (F6). Applied only when polish = Polished.
export const TONES: { value: DictationSettings["tone"]; label: string }[] = [
  { value: "neutral", label: "Neutral" },
  { value: "polite", label: "Polite" },
  { value: "friendly", label: "Friendly" },
  { value: "professional", label: "Professional" },
];

// Default cleanup level (F6). "Direct" is verbatim; the pill can still override
// this per dictation.
export const POLISH_LEVELS: { value: DictationSettings["polish"]; label: string }[] = [
  { value: "raw", label: "Direct" },
  { value: "clean", label: "Clean" },
  { value: "polished", label: "Polished" },
];

export const SPEECH_MODELS: { value: string; label: string; englishOnly?: boolean }[] = [
  { value: "whisper-large-v3-turbo-q5_0", label: "High accuracy · multilingual" },
  { value: "whisper-base.en", label: "Fast · English only", englishOnly: true },
  { value: "whisper-tiny.en", label: "Tiny · English only", englishOnly: true },
];

// ISO 639-1 codes for the ASR "speech language" hint. "" = auto-detect (the
// default) — the model detects the spoken language. Pinning one improves
// accuracy when you dictate mostly in a single language.
export const SPEECH_LANGUAGES: { code: string; label: string }[] = [
  { code: "", label: "Auto-detect" },
  { code: "en", label: "English" },
  { code: "es", label: "Spanish" },
  { code: "de", label: "German" },
  { code: "fr", label: "French" },
  { code: "it", label: "Italian" },
  { code: "pt", label: "Portuguese" },
  { code: "nl", label: "Dutch" },
  { code: "pl", label: "Polish" },
  { code: "ru", label: "Russian" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "zh", label: "Chinese" },
  { code: "ar", label: "Arabic" },
  { code: "hi", label: "Hindi" },
];
