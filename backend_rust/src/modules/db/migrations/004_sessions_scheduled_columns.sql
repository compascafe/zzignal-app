-- Migration 004: Add scheduled columns to recording_sessions
-- La tabla ya existe sin scheduled_start/scheduled_end desde una versión anterior

-- Solo ejecutar si no existen las columnas
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_start') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_start TIMESTAMPTZ;
    END IF;

    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='recording_sessions' AND column_name='scheduled_end') THEN
        ALTER TABLE recording_sessions ADD COLUMN scheduled_end TIMESTAMPTZ;
    END IF;
END $$;

-- Si scheduled_start es NULL, asignar un valor por defecto basado en started_at
UPDATE recording_sessions
SET scheduled_start = COALESCE(scheduled_start, started_at),
    scheduled_end   = COALESCE(scheduled_end, started_at + INTERVAL '15 minutes')
WHERE scheduled_start IS NULL;

-- Hacer las columnas NOT NULL ahora que tienen datos
ALTER TABLE recording_sessions ALTER COLUMN scheduled_start SET NOT NULL;
ALTER TABLE recording_sessions ALTER COLUMN scheduled_end SET NOT NULL;

-- Modificar started_at para que sea nullable (ahora se usa scheduled_start)
ALTER TABLE recording_sessions ALTER COLUMN started_at DROP NOT NULL;
