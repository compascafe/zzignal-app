-- Migration 004: Asegurar columnas scheduled_start/scheduled_end NOT NULL
-- Idempotente: solo hace cambios si las columnas existen y tienen NULLs

DO $$
BEGIN
    -- Añadir columnas si no existen (caso: tabla legacy sin estas columnas)
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_start TIMESTAMPTZ;
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_end TIMESTAMPTZ;
    END IF;

    -- Poblar NULLs con valores por defecto
    IF EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_start IS NULL) THEN
        UPDATE recording_sessions
        SET scheduled_start = COALESCE(scheduled_start, started_at, NOW()),
            scheduled_end   = COALESCE(scheduled_end, started_at + INTERVAL '15 minutes', NOW() + INTERVAL '15 minutes')
        WHERE scheduled_start IS NULL OR scheduled_end IS NULL;
    END IF;

    -- Hacer NOT NULL (solo si no hay NULLs)
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_start IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_start SET NOT NULL;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM recording_sessions WHERE scheduled_end IS NULL) THEN
        ALTER TABLE recording_sessions ALTER COLUMN scheduled_end SET NOT NULL;
    END IF;
END $$;
