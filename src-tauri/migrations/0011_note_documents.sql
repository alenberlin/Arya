-- F2/M2: rich block-editor content for notes. `document_json` holds the
-- BlockNote block-JSON, the editor's source of truth. The existing `body_md`
-- becomes its plaintext/markdown projection (recomputed from the blocks on every
-- save), so full-text search and the RAG index keep working unchanged. An empty
-- `document_json` means "legacy note": the editor falls back to `body_md` and
-- converts it to blocks on first open.
ALTER TABLE notes ADD COLUMN document_json TEXT NOT NULL DEFAULT '';
