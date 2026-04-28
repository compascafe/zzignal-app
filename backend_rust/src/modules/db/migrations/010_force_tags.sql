-- Migration 010: Force-add tag + tag_color columns (belt and suspenders)
-- Si 009 no se aplicó, esta migración lo resuelve.
-- Totalmente idempotente.

ALTER TABLE recording_sessions ADD COLUMN IF NOT EXISTS tag VARCHAR(50);
ALTER TABLE recording_sessions ADD COLUMN IF NOT EXISTS tag_color VARCHAR(20) NOT NULL DEFAULT '#3b82f6';
CREATE INDEX IF NOT EXISTS idx_rs_tag ON recording_sessions(tag);
