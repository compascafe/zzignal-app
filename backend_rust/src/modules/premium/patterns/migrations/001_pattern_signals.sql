-- Pattern Signals table
-- Almacena todas las señales detectadas por el Pattern Detector

CREATE TABLE IF NOT EXISTS pattern_signals (
    id          BIGSERIAL PRIMARY KEY,
    pattern     VARCHAR(50) NOT NULL,
    side        VARCHAR(10) NOT NULL,
    severity    VARCHAR(10) NOT NULL,
    description TEXT NOT NULL,
    data        JSONB NOT NULL DEFAULT '{}',
    ts          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ps_ts      ON pattern_signals (ts DESC);
CREATE INDEX IF NOT EXISTS idx_ps_pattern ON pattern_signals (pattern);
CREATE INDEX IF NOT EXISTS idx_ps_side    ON pattern_signals (side);
