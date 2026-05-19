-- Migration 006: RESCATE — Repara CUALQUIER schema de recording_sessions
-- Idempotente. Añade TODAS las columnas que puedan faltar en un schema legacy.
-- Se ejecuta DESPUÉS de 003 y 004, incluso si esas fallaron parcialmente.

DO $$
BEGIN
    -- scheduled_start
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_start TIMESTAMPTZ;
    END IF;

    -- scheduled_end
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_end TIMESTAMPTZ;
    END IF;

    -- market_id
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='market_id') THEN
        ALTER TABLE recording_sessions ADD COLUMN market_id TEXT;
    END IF;

    -- market_title
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='market_title') THEN
        ALTER TABLE recording_sessions ADD COLUMN market_title TEXT;
    END IF;

    -- capture_mode
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='capture_mode') THEN
        ALTER TABLE recording_sessions ADD COLUMN capture_mode VARCHAR(20) NOT NULL DEFAULT 'tick';
    END IF;

    -- depth_levels
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='depth_levels') THEN
        ALTER TABLE recording_sessions ADD COLUMN depth_levels INT NOT NULL DEFAULT 20;
    END IF;

    -- strike_price
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='strike_price') THEN
        ALTER TABLE recording_sessions ADD COLUMN strike_price DOUBLE PRECISION;
    END IF;

    -- final_price
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='final_price') THEN
        ALTER TABLE recording_sessions ADD COLUMN final_price DOUBLE PRECISION;
    END IF;

    -- outcome_result
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='outcome_result') THEN
        ALTER TABLE recording_sessions ADD COLUMN outcome_result VARCHAR(10);
    END IF;

    -- btc_price_start
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='btc_price_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN btc_price_start DOUBLE PRECISION;
    END IF;

    -- btc_price_end
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='btc_price_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN btc_price_end DOUBLE PRECISION;
    END IF;

    -- tick_count
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='tick_count') THEN
        ALTER TABLE recording_sessions ADD COLUMN tick_count INT NOT NULL DEFAULT 0;
    END IF;

    -- trade_count
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='trade_count') THEN
        ALTER TABLE recording_sessions ADD COLUMN trade_count INT NOT NULL DEFAULT 0;
    END IF;

    -- created_at
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='created_at') THEN
        ALTER TABLE recording_sessions ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;

    -- duration_min
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='duration_min') THEN
        ALTER TABLE recording_sessions ADD COLUMN duration_min INT NOT NULL DEFAULT 15;
    END IF;

    -- status (asegurar que existe)
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='status') THEN
        ALTER TABLE recording_sessions ADD COLUMN status VARCHAR(20) NOT NULL DEFAULT 'scheduled';
    END IF;

    -- Poblar scheduled_start/scheduled_end para filas existentes con NULLs
    UPDATE recording_sessions
    SET scheduled_start = COALESCE(scheduled_start, started_at, NOW()),
        scheduled_end   = COALESCE(scheduled_end, started_at + INTERVAL '15 minutes', NOW() + INTERVAL '15 minutes')
    WHERE scheduled_start IS NULL OR scheduled_end IS NULL;

    -- Hacer NOT NULL si ya no hay NULLs
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_start IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_start SET NOT NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_end IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_end SET NOT NULL;
    END IF;
END $$;

-- Reparar session_snapshots (columnas faltantes)
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='depth_bids') THEN
        ALTER TABLE session_snapshots ADD COLUMN depth_bids JSONB;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='depth_asks') THEN
        ALTER TABLE session_snapshots ADD COLUMN depth_asks JSONB;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='btc_price') THEN
        ALTER TABLE session_snapshots ADD COLUMN btc_price DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='bid_volume') THEN
        ALTER TABLE session_snapshots ADD COLUMN bid_volume DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='ask_volume') THEN
        ALTER TABLE session_snapshots ADD COLUMN ask_volume DOUBLE PRECISION;
    END IF;
END $$;

-- Índice de deduplicación en session_trades (si no existe)
CREATE INDEX IF NOT EXISTS idx_st_dedup ON session_trades (session_id, side, trade_side, price, size, ts);
