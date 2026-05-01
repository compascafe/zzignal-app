-- Migration 014: Extend session_logs with VFI confidence, DB accuracy factor, and dynamic RSI

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_logs' AND column_name='vfi_confidence') THEN
        ALTER TABLE session_logs ADD COLUMN vfi_confidence DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_logs' AND column_name='db_accuracy_factor') THEN
        ALTER TABLE session_logs ADD COLUMN db_accuracy_factor DOUBLE PRECISION DEFAULT 1.0;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_logs' AND column_name='dynamic_rsi_end') THEN
        ALTER TABLE session_logs ADD COLUMN dynamic_rsi_end DOUBLE PRECISION;
    END IF;
END $$;
