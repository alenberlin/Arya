-- Per-artifact capture offset: the out-of-process system tap starts after the
-- microphone, so a system turn's timestamps are relative to a later zero.
-- offset_ms shifts them onto the mic timeline when turns are merged, keeping a
-- meeting transcript correctly ordered.
ALTER TABLE audio_artifacts ADD COLUMN offset_ms INTEGER NOT NULL DEFAULT 0;
