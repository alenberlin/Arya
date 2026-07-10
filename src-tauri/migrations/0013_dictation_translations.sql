-- F8/M9: on-demand translations of a saved dictation. Non-destructive and
-- stackable — one row per (dictation, language), so a dictation can carry
-- several translations alongside its original text. Cascades when the dictation
-- is deleted. (Distinct from the capture-time `translated_text` column in 0009,
-- which is the single translation produced while dictating.)
CREATE TABLE dictation_translations (
    id            TEXT PRIMARY KEY,
    dictation_id  TEXT NOT NULL REFERENCES dictation_history(id) ON DELETE CASCADE,
    lang          TEXT NOT NULL,
    text          TEXT NOT NULL,
    model         TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_dictation_translations_unique
    ON dictation_translations(dictation_id, lang);
CREATE INDEX idx_dictation_translations_dictation
    ON dictation_translations(dictation_id);
