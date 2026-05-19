-- Migration 011: HFT Binance Depth + Cross-Exchange Metrics
-- Añade columnas de profundidad CEX (Binance) y métricas HFT a session_snapshots
-- Crea tabla hft_snapshots para exportación CSV rápida

DO $$
BEGIN
    -- ─── Binance depth (JSONB) ───────────────────────────────────────────────
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_depth_bids') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_depth_bids JSONB;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_depth_asks') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_depth_asks JSONB;
    END IF;

    -- ─── Binance volume breakdown ────────────────────────────────────────────
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_bid_vol_5') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_bid_vol_5 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_ask_vol_5') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_ask_vol_5 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_bid_vol_10') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_bid_vol_10 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_ask_vol_10') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_ask_vol_10 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_bid_vol_20') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_bid_vol_20 DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_ask_vol_20') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_ask_vol_20 DOUBLE PRECISION;
    END IF;

    -- ─── HFT metrics ─────────────────────────────────────────────────────────
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_mid_price') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_mid_price DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_micro_price') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_micro_price DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_vbs') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_vbs DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_vpin') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_vpin DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_depth_ratio') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_depth_ratio DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_spread') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_spread DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_event_time') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_event_time BIGINT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='latency_delta') THEN
        ALTER TABLE session_snapshots ADD COLUMN latency_delta DOUBLE PRECISION;
    END IF;

    -- ─── Polymarket HFT metrics ──────────────────────────────────────────────
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='poly_micro_price') THEN
        ALTER TABLE session_snapshots ADD COLUMN poly_micro_price DOUBLE PRECISION;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='poly_vbs') THEN
        ALTER TABLE session_snapshots ADD COLUMN poly_vbs DOUBLE PRECISION;
    END IF;
END $$;

-- ─── HFT Snapshots (formato CSV-friendly para exportación rápida) ────────────

CREATE TABLE IF NOT EXISTS hft_snapshots (
    id               SERIAL PRIMARY KEY,
    ts               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    btc_price_binance DOUBLE PRECISION,
    btc_bid_vol_5    DOUBLE PRECISION,
    btc_ask_vol_5    DOUBLE PRECISION,
    poly_mid_price   DOUBLE PRECISION,
    poly_imbalance   DOUBLE PRECISION,
    latency_delta    DOUBLE PRECISION,
    session_id       INT REFERENCES recording_sessions(id) ON DELETE SET NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_hft_ts      ON hft_snapshots(ts DESC);
CREATE INDEX IF NOT EXISTS idx_hft_session ON hft_snapshots(session_id);
