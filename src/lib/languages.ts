/**
 * Target languages offered wherever the app translates — dictation, notes, and
 * agent chats — kept in one place so the choices stay consistent across
 * surfaces. Note/chat translation runs through the generic AI transform, so the
 * instruction that drives it lives here too.
 */
export const TRANSLATE_LANGUAGES = [
  "German",
  "Spanish",
  "French",
  "Italian",
  "Portuguese",
  "Dutch",
  "Polish",
  "Russian",
  "Turkish",
  "Japanese",
  "Korean",
  "Chinese",
  "Arabic",
  "Hindi",
];

/** The instruction that turns the generic AI transform into a translation. */
export const translateInstruction = (lang: string) =>
  `Translate the following text into ${lang}. Preserve meaning, tone, and any ` +
  `markdown structure. Output only the translation — no preamble, no commentary.`;
