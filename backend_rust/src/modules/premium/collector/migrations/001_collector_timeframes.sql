-- Collector: Order Book Multi-Timeframe Candles
-- Una vela por (interval, side, open_time) con OHLC de best_bid, best_ask, spread, mid_price

CREATE TABLE IF NOT EXISTS ob_timeframes (
    id          BIGSERIAL PRIMARY KEY,
    interval    VARCHAR(10) NOT NULL,        -- '1m','5m','15m','1h','4h','1d'
    side        VARCHAR(10) NOT NULL,        -- 'up' | 'down'
    open_time   TIMESTAMPTZ NOT NULL,

    -- Best Bid OHLC
    bid_open    DOUBLE PRECISION NOT NULL,
    bid_high    DOUBLE PRECISION NOT NULL,
    bid_low     DOUBLE PRECISION NOT NULL,
    bid_close   DOUBLE PRECISION NOT NULL,

    -- Best Ask OHLC
    ask_open    DOUBLE PRECISION NOT NULL,
    ask_high    DOUBLE PRECISION NOT NULL,
    ask_low     DOUBLE PRECISION NOT NULL,
    ask_close   DOUBLE PRECISION NOT NULL,

    -- Spread OHLC
    spread_open  DOUBLE PRECISION NOT NULL,
    spread_high  DOUBLE PRECISION NOT NULL,
    spread_low   DOUBLE PRECISION NOT NULL,
    spread_close DOUBLE PRECISION NOT NULL,

    -- Mid Price OHLC
    mid_open    DOUBLE PRECISION NOT NULL,
    mid_high    DOUBLE PRECISION NOT NULL,
    mid_low     DOUBLE PRECISION NOT NULL,
    mid_close   DOUBLE PRECISION NOT NULL,

    bid_volume  DOUBLE PRECISION NOT NULL DEFAULT 0,
    ask_volume  DOUBLE PRECISION NOT NULL DEFAULT 0,
    tick_count  INT NOT NULL DEFAULT 0,

    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (interval, side, open_time)
);

CREATE INDEX IF NOT EXISTS idx_obtf_interval_time ON ob_timeframes (interval, open_time DESC);
CREATE INDEX IF NOT EXISTS idx_obtf_side         ON ob_timeframes (side);
