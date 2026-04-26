-- Migration 005: Repara el schema de recording_sessions
-- Escenario: la tabla se creó con una versión antigua del código (pre-003)
-- y le faltan las columnas scheduled_start, scheduled_end, y otras.
-- Esta migración es idempotente: solo añade lo que falta.

DO $$
BEGIN
    -- 1. Añadir scheduled_start si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_start TIMESTAMPTZ;
    END IF;

    -- 2. Añadir scheduled_end si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_end TIMESTAMPTZ;
    END IF;

    -- 3. Añadir market_id si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='market_id') THEN
        ALTER TABLE recording_sessions ADD COLUMN market_id TEXT;
    END IF;

    -- 4. Añadir market_title si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='market_title') THEN
        ALTER TABLE recording_sessions ADD COLUMN market_title TEXT;
    END IF;

    -- 5. Añadir capture_mode si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='capture_mode') THEN
        ALTER TABLE recording_sessions ADD COLUMN capture_mode VARCHAR(20) NOT NULL DEFAULT 'tick';
    END IF;

    -- 6. Añadir depth_levels si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='depth_levels') THEN
        ALTER TABLE recording_sessions ADD COLUMN depth_levels INT NOT NULL DEFAULT 20;
    END IF;

    -- 7. Añadir strike_price si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='strike_price') THEN
        ALTER TABLE recording_sessions ADD COLUMN strike_price DOUBLE PRECISION;
    END IF;

    -- 8. Añadir final_price si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='final_price') THEN
        ALTER TABLE recording_sessions ADD COLUMN final_price DOUBLE PRECISION;
    END IF;

    -- 9. Añadir outcome_result si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='outcome_result') THEN
        ALTER TABLE recording_sessions ADD COLUMN outcome_result VARCHAR(10);
    END IF;

    -- 10. Añadir btc_price_start si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='btc_price_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN btc_price_start DOUBLE PRECISION;
    END IF;

    -- 11. Añadir btc_price_end si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='btc_price_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN btc_price_end DOUBLE PRECISION;
    END IF;

    -- 12. Añadir tick_count si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='tick_count') THEN
        ALTER TABLE recording_sessions ADD COLUMN tick_count INT NOT NULL DEFAULT 0;
    END IF;

    -- 13. Añadir trade_count si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='trade_count') THEN
        ALTER TABLE recording_sessions ADD COLUMN trade_count INT NOT NULL DEFAULT 0;
    END IF;

    -- 14. Añadir created_at si no existe
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='created_at') THEN
        ALTER TABLE recording_sessions ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
    END IF;

    -- 15. Poblar scheduled_start/scheduled_end para filas existentes
    --     Si started_at existe, úsalo. Si no, usa NOW().
    IF EXISTS (SELECT 1 FROM information_schema.columns
               WHERE table_name='recording_sessions' AND column_name='started_at') THEN
        UPDATE recording_sessions
        SET scheduled_start = COALESCE(scheduled_start, started_at, NOW()),
            scheduled_end   = COALESCE(scheduled_end, started_at + INTERVAL '15 minutes', NOW() + INTERVAL '15 minutes')
        WHERE scheduled_start IS NULL OR scheduled_end IS NULL;
    ELSE
        UPDATE recording_sessions
        SET scheduled_start = COALESCE(scheduled_start, NOW()),
            scheduled_end   = COALESCE(scheduled_end, NOW() + INTERVAL '15 minutes')
        WHERE scheduled_start IS NULL OR scheduled_end IS NULL;
    END IF;

    -- 16. Hacer NOT NULL las columnas críticas solo si todas tienen valor
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_start IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_start SET NOT NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_end IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_end SET NOT NULL;
    END IF;

END $$;

-- 17. Reparar el campo status: si es NULL, poner 'scheduled'
UPDATE recording_sessions SET status = 'scheduled' WHERE status IS NULL;

-- 18. Si la tabla session_snapshots ya existe pero con schema viejo (sin depth_bids/depth_asks),
--     añadir las columnas faltantes
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_name='session_snapshots') THEN
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
    END IF;
END $$;

-- 19. Índice único en session_trades para evitar duplicados (BUG: fills se enviaban
--     cada 5s sin deduplicación, insertando los mismos fills ~180 veces por sesión)
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_indexes WHERE indexname = 'idx_st_dedup') THEN
        CREATE UNIQUE INDEX idx_st_dedup ON session_trades (session_id, side, trade_side, price, size, ts);
    END IF;
END $$;
