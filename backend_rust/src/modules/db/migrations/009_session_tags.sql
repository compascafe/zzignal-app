-- Migration 009: Add tag + tag_color to recording_sessions
-- Allows labeling sessions with a custom tag and color for organization

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='tag') THEN
        ALTER TABLE recording_sessions ADD COLUMN tag VARCHAR(50);
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='tag_color') THEN
        ALTER TABLE recording_sessions ADD COLUMN tag_color VARCHAR(20) NOT NULL DEFAULT '#3b82f6';
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_rs_tag ON recording_sessions(tag);
