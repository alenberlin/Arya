# Spec — Dictation translation

Status: **LOCKED (2026-07-05).** Build order: after note attachments.

## 1. Goal

Dictate in one language and have the result written in another. You speak
(v1: in English), the pill shows the English words as you speak, and when the
dictation lands it is **translated** into a chosen target language before it is
pasted and stored. History keeps **both** the source and the translation so you
can review them side by side.

Your words: *"I speak in English, but the text gets written in native German …
in the pill I still see English, once it lands in the field and history it's
translated … side by side English and German."*

## 2. Non-goals (v1)

- **Dictating *in* another language** (German speech → German text). That is a
  separate effort gated by the multilingual-ASR gap (see §9); v1's source is
  English, which the current ASR already handles.
- Translating notes, agent replies, or any surface other than dictation.
- Auto-detecting the source language.
- Translating the *live preview* (the pill stays source/English; translation
  happens once, on finalize).

## 3. UX

- **Setting:** Dictation settings gains a **"Translate to"** control: `Off`
  (default) or a target language from a curated list. When set, every dictation
  is translated to that language.
- **Pill:** unchanged — the live preview shows the English words as you speak.
  (Optional later: a small `EN → DE` indicator.)
- **On finish:** the cleaned English is translated; the **translation** is what
  gets pasted into the focused app.
- **History (Recent dictations):** each row shows the **translation** as the
  primary text with the **English source** beside/under it — a side-by-side
  (source ↔ target) view.

## 4. Architecture

Translation is a distinct semantic step that runs **after** cleanup, inside the
existing delivery path — it is *not* a `TextCleaner` (those preserve words; a
translation deliberately rewrites them).

**Pipeline (in `dictation/service.rs::deliver`):**
```
raw (ASR, English)
  → cleanup (existing: Raw/Clean/Polished)          = clean  (English)
  → if translate set: translate(clean, target)      = translated (target)
  → paste  (translated if set, else clean)
  → history: store raw, clean, translated, target_lang
```

**Backend:**
- `DictationSettings` gains `translate: Option<String>` (BCP-47/ISO-639-1 target,
  e.g. `"de"`; `None` = off).
- New `translate` module with a `Translator` trait and an `OllamaTranslator`
  (reuses the Ollama chat endpoint already used by `OllamaCleaner`). System
  prompt: *"Translate the text to <target>. Output only the translation, no
  notes."* Same **graceful fallback** contract as cleanup: any failure returns
  the untranslated English so a dictation is **never lost**.
- `deliver()` computes `clean`, then `translated = clean` unless `translate` is
  set and the translator succeeds. Pastes `translated`.

**Data model** — migration `0008_dictation_translation.sql`:
```
ALTER TABLE dictation_history ADD COLUMN translated_text TEXT;   -- nullable
ALTER TABLE dictation_history ADD COLUMN target_lang     TEXT;   -- nullable
```
`clean_text` stays the English cleaned text; `translated_text`/`target_lang` are
null when translation is off.

**Frontend:**
- `DictationPanel`: a "Translate to" `<select>` (Off + curated list), saved into
  settings. `lib/dictation.ts` gains `translate: string | null`.
- `HistoryItem` gains `translatedText` + `targetLang`; the Recent-dictations rows
  render source ↔ translation side by side when present.

**No new external dependency** — reuses the Ollama/LLM path already in the tree.

## 5. Decisions to lock (my recommendation in **bold**)

1. **Source language v1 = English only.** (Dictating in other languages is the
   separate multilingual-ASR effort, §9.) → **Lock English source.**
2. **LOCKED — both providers in v1, local default.** Local Ollama (private,
   on-brand, the default) *and* cloud via the Arya API (Claude) as a per-choice
   upgrade — the same local/cloud split as the rest of the app. The `Translator`
   trait has two implementations; the choice is a setting.
3. **Pasted text = the translation only** (German in the field); **both** kept in
   history. → **Lock translation-only paste.**
4. **Target languages = a curated list** (de, es, fr, it, pt, nl, pl, ru, tr, ja,
   ko, zh, ar, hi) — the model handles the actual translation; the list is just
   the picker. → **Lock curated list** (final list TBD, easily extended).
5. **Latency:** translation adds one LLM call on finish (like `Polished`). With
   streaming, the preview is instant English and the final paste waits on the
   translation. → **Lock: acceptable, it's a deliberate mode.**
6. **RAG search** keeps indexing `clean_text` (English source) v1; indexing the
   translation is a later option. → **Lock: no RAG change v1.**

## 6. Edge cases & failure handling

- Translator unreachable / empty / model missing → paste the **English** (never
  drop the dictation); `translated_text` stays null.
- Empty ASR → existing "nothing recognized" path, unchanged.
- Very long text → single LLM call; acceptable.
- `Raw` polish + translate → translate the raw-but-dictionaried English.

## 7. Privacy

Local Ollama translation is fully on-device (on-brand). Cloud is opt-in via the
Arya API, consistent with the existing local/cloud model split.

## 8. Verification plan

- **Unit:** `OllamaTranslator` prompt building; **fallback to source on an
  unreachable server** (mirrors the existing `cleanup::ollama` test); settings
  round-trip with `translate`.
- **Schema:** migration applies; history round-trips `translated_text` +
  `target_lang`.
- **Frontend:** settings selector saves; history renders side-by-side.
- **Manual (needs a model + mic):** actual translation quality and the pill =
  English / field = German behavior.

## 9. Related but separate: the multilingual-ASR gap

Today the app is effectively **English-only** (default `whisper-base.en`;
streaming model is English; no language/model picker UI). The multilingual
`whisper-large-v3-turbo` (~99 languages) is in the catalog and the `language`
hint is already honored by the whisper engine — it just needs a UI. **Dictating
in another language** is that separate feature; **this translation feature does
not depend on it** (you speak English). Flagged so we sequence them
deliberately.

## 10. Implementation slices (each verified)

1. **Backend translate step** — `translate` + `translate_provider` settings,
   `Translator` trait with `OllamaTranslator` **and** a cloud translator (Arya
   API), `deliver()` integration, migration `0008`, history columns. Tests
   (fallback, schema round-trip).
2. **Settings UI** — "Translate to" selector + local/cloud choice in
   `DictationPanel` + TS types.
3. **History side-by-side** — source ↔ translation in Recent dictations.
4. **Fast-follows (not v1):** pill `EN → DE` indicator, indexing the translation
   for RAG, multilingual source ASR.
