-- Migration 003: Session Recorder
-- Graba sesiones completas del order book por tick para análisis HFT y datasets

-- ─── Recording Sessions ───────────────────────────────────────────────────────
-- Una sesión = captura completa de UP + DOWN durante N minutos

CREATE TABLE recording_sessions (
    id              SERIAL PRIMARY KEY,
    name            TEXT NOT NULL,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    stopped_at      TIMESTAMPTZ,
    duration_min    INT NOT NULL DEFAULT 15,

    market_id       TEXT,
    market_title    TEXT,

    capture_mode    VARCHAR(20) NOT NULL DEFAULT 'tick',  -- 'tick' | 'interval'
    depth_levels    INT NOT NULL DEFAULT 20,              -- niveles de profundidad guardados

    strike_price    DOUBLE PRECISION,                     -- precio de apertura (BTC)
    final_price     DOUBLE PRECISION,                     -- precio de cierre (BTC)
    outcome_result  VARCHAR(10),                          -- 'up' | 'down' | 'tie'

    btc_price_start DOUBLE PRECISION,
    btc_price_end   DOUBLE PRECISION,

    status          VARCHAR(20) NOT NULL DEFAULT 'recording',  -- recording | stopped | completed

    tick_count      INT NOT NULL DEFAULT 0,
    trade_count     INT NOT NULL DEFAULT 0,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_rs_status       ON recording_sessions(status);
CREATE INDEX idx_rs_started_at   ON recording_sessions(started_at DESC);

-- ─── Session Snapshots ────────────────────────────────────────────────────────
-- Cada tick del book (UP o DOWN) durante una sesión activa

CREATE TABLE session_snapshots (
    id              SERIAL PRIMARY KEY,
    session_id      INT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    ts              TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    side            VARCHAR(10) NOT NULL,              -- 'up' | 'down'
    best_bid        DOUBLE PRECISION,
    best_bid_sz     DOUBLE PRECISION,
    best_ask        DOUBLE PRECISION,
    best_ask_sz     DOUBLE PRECISION,
    spread          DOUBLE PRECISION,
    mid_price       DOUBLE PRECISION,                   -- (best_bid + best_ask) / 2

    bid_volume      DOUBLE PRECISION,                   -- suma de sizes en depth_bids
    ask_volume      DOUBLE PRECISION,                   -- suma de sizes en depth_asks

    depth_bids      JSONB,                              -- top N niveles: [{p,s},...]
    depth_asks      JSONB,

    btc_price       DOUBLE PRECISION,                   -- precio BTC en ese instante

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_ss_session_id   ON session_snapshots(session_id);
CREATE INDEX idx_ss_ts           ON session_snapshots(ts);
CREATE INDEX idx_ss_session_ts   ON session_snapshots(session_id, ts DESC);

-- ─── Session Trades ───────────────────────────────────────────────────────────
-- Cada fill individual que ocurre durante una sesión

CREATE TABLE session_trades (
    id              SERIAL PRIMARY KEY,
    session_id      INT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    ts              TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    side            VARCHAR(10) NOT NULL,              -- 'up' | 'down' (outcome)
    trade_side      VARCHAR(10) NOT NULL,              -- 'buy' | 'sell'
    price           DOUBLE PRECISION NOT NULL,
    size            DOUBLE PRECISION NOT NULL,

    btc_price       DOUBLE PRECISION,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_st_session_id   ON session_trades(session_id);
CREATE INDEX idx_st_ts           ON session_trades(ts);
