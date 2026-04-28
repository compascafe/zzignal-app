-- Migration 002: Order Book Snapshots + Scheduled Executions
-- Módulo BD: persistencia de order books y ejecuciones programadas

-- ─── Order Book Snapshots ─────────────────────────────────────────────────────
-- Guarda snapshots periódicos del order book UP/DOWN para análisis histórico

CREATE TABLE IF NOT EXISTS order_book_snapshots (
    id          SERIAL PRIMARY KEY,
    ts          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    side        VARCHAR(10) NOT NULL,              -- 'up' | 'down'
    best_bid    DOUBLE PRECISION,
    best_bid_sz DOUBLE PRECISION,
    best_ask    DOUBLE PRECISION,
    best_ask_sz DOUBLE PRECISION,
    spread      DOUBLE PRECISION,
    depth_bids  JSONB,                              -- top 5 bids
    depth_asks  JSONB,                              -- top 5 asks
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_obs_ts    ON order_book_snapshots(ts DESC);
CREATE INDEX IF NOT EXISTS idx_obs_side  ON order_book_snapshots(side);
CREATE INDEX IF NOT EXISTS idx_obs_ts_side ON order_book_snapshots(ts DESC, side);

-- ─── Scheduled Executions ─────────────────────────────────────────────────────
-- Programar ejecuciones de órdenes en el futuro (limit/market/scalp)

CREATE TABLE IF NOT EXISTS scheduled_executions (
    id            SERIAL PRIMARY KEY,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    scheduled_at  TIMESTAMPTZ NOT NULL,
    executed_at   TIMESTAMPTZ,
    status        VARCHAR(20) NOT NULL DEFAULT 'pending',
                                                   -- pending | executed | cancelled | failed
    side          VARCHAR(10) NOT NULL,            -- 'buy' | 'sell'
    outcome       VARCHAR(10) NOT NULL,            -- 'up' | 'down'
    order_type    VARCHAR(20) NOT NULL,            -- 'limit' | 'market' | 'scalp'
    price         DOUBLE PRECISION,                -- NULL para market
    size          DOUBLE PRECISION,
    amount_usdc   DOUBLE PRECISION,                -- para market orders
    target_price  DOUBLE PRECISION,                -- para scalp
    notes         TEXT,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_se_scheduled_at ON scheduled_executions(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_se_status       ON scheduled_executions(status);
CREATE INDEX IF NOT EXISTS idx_se_pending      ON scheduled_executions(scheduled_at) WHERE status = 'pending';
