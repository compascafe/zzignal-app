-- Migration 003: Session Recorder (IDEMPOTENTE)
-- Usa IF NOT EXISTS para no fallar si las tablas ya existen (schema legacy)
-- Las columnas faltantes se reparan en las migraciones 004, 005 y 006

CREATE TABLE IF NOT EXISTS recording_sessions (
    id              SERIAL PRIMARY KEY,
    name            TEXT NOT NULL,
    scheduled_start TIMESTAMPTZ,
    scheduled_end   TIMESTAMPTZ,
    started_at      TIMESTAMPTZ,
    stopped_at      TIMESTAMPTZ,
    duration_min    INT NOT NULL DEFAULT 15,
    market_id       TEXT,
    market_title    TEXT,
    capture_mode    VARCHAR(20) NOT NULL DEFAULT 'tick',
    depth_levels    INT NOT NULL DEFAULT 20,
    strike_price    DOUBLE PRECISION,
    final_price     DOUBLE PRECISION,
    outcome_result  VARCHAR(10),
    btc_price_start DOUBLE PRECISION,
    btc_price_end   DOUBLE PRECISION,
    status          VARCHAR(20) NOT NULL DEFAULT 'recording',
    tick_count      INT NOT NULL DEFAULT 0,
    trade_count     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rs_status     ON recording_sessions(status);
CREATE INDEX IF NOT EXISTS idx_rs_started_at ON recording_sessions(started_at DESC);

CREATE TABLE IF NOT EXISTS session_snapshots (
    id              SERIAL PRIMARY KEY,
    session_id      INT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    ts              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    side            VARCHAR(10) NOT NULL,
    best_bid        DOUBLE PRECISION,
    best_bid_sz     DOUBLE PRECISION,
    best_ask        DOUBLE PRECISION,
    best_ask_sz     DOUBLE PRECISION,
    spread          DOUBLE PRECISION,
    mid_price       DOUBLE PRECISION,
    bid_volume      DOUBLE PRECISION,
    ask_volume      DOUBLE PRECISION,
    depth_bids      JSONB,
    depth_asks      JSONB,
    btc_price       DOUBLE PRECISION,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ss_session_id ON session_snapshots(session_id);
CREATE INDEX IF NOT EXISTS idx_ss_ts         ON session_snapshots(ts);
CREATE INDEX IF NOT EXISTS idx_ss_session_ts ON session_snapshots(session_id, ts DESC);

CREATE TABLE IF NOT EXISTS session_trades (
    id              SERIAL PRIMARY KEY,
    session_id      INT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    ts              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    side            VARCHAR(10) NOT NULL,
    trade_side      VARCHAR(10) NOT NULL,
    price           DOUBLE PRECISION NOT NULL,
    size            DOUBLE PRECISION NOT NULL,
    btc_price       DOUBLE PRECISION,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_st_session_id ON session_trades(session_id);
CREATE INDEX IF NOT EXISTS idx_st_ts         ON session_trades(ts);
