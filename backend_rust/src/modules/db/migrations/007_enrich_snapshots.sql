-- Migration 007: Enrich session_snapshots with multi-depth volumes and derived metrics
-- Añade columnas para análisis HFT profesional

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='bid_volume_5') THEN
        ALTER TABLE session_snapshots ADD COLUMN bid_volume_5 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='ask_volume_5') THEN
        ALTER TABLE session_snapshots ADD COLUMN ask_volume_5 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='bid_volume_10') THEN
        ALTER TABLE session_snapshots ADD COLUMN bid_volume_10 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='ask_volume_10') THEN
        ALTER TABLE session_snapshots ADD COLUMN ask_volume_10 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='imbalance_ratio') THEN
        ALTER TABLE session_snapshots ADD COLUMN imbalance_ratio DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='up_probability') THEN
        ALTER TABLE session_snapshots ADD COLUMN up_probability DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='down_probability') THEN
        ALTER TABLE session_snapshots ADD COLUMN down_probability DOUBLE PRECISION;
    END IF;
END $$;
