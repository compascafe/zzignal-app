-- Executor: Strategies & Execution Logs

CREATE TABLE IF NOT EXISTS strategies (
    id               BIGSERIAL PRIMARY KEY,
    name             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    enabled          BOOLEAN NOT NULL DEFAULT true,
    conditions       JSONB NOT NULL DEFAULT '[]',
    action           JSONB NOT NULL DEFAULT '{}',
    cooldown_secs    INT NOT NULL DEFAULT 30,
    max_positions    INT NOT NULL DEFAULT 3,
    max_size_total   DOUBLE PRECISION NOT NULL DEFAULT 100.0,
    stop_loss_pct    DOUBLE PRECISION NOT NULL DEFAULT 5.0,
    take_profit_pct  DOUBLE PRECISION NOT NULL DEFAULT 10.0,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_executed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS execution_logs (
    id             BIGSERIAL PRIMARY KEY,
    strategy_id    BIGINT NOT NULL,
    strategy_name  TEXT NOT NULL,
    outcome        VARCHAR(10) NOT NULL,
    side           VARCHAR(10) NOT NULL,
    order_type     VARCHAR(10) NOT NULL,
    price          DOUBLE PRECISION,
    size           DOUBLE PRECISION,
    result         VARCHAR(20) NOT NULL,
    reason         TEXT,
    btc_price      DOUBLE PRECISION,
    ts             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_el_ts   ON execution_logs (ts DESC);
CREATE INDEX IF NOT EXISTS idx_el_sid  ON execution_logs (strategy_id);
