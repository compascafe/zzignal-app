-- Migration 008: Add parent_id to recording_sessions for hierarchical sessions
-- Allows auto-splitting 4h sessions into 15-min children (parent-child model)
-- Children are independent sessions with their own strike/final/outcome

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='parent_id') THEN
        ALTER TABLE recording_sessions ADD COLUMN parent_id INT REFERENCES recording_sessions(id) ON DELETE CASCADE;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_rs_parent_id ON recording_sessions(parent_id);
