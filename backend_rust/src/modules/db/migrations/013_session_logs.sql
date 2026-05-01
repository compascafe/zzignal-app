-- Migration 013: Adaptive Risk Engine — Session Feedback Logs
-- Stores per-session macro predictions, actual outcomes, and CP calibration state
-- for the Robbins-Monro feedback loop (Aich et al., 2025)

CREATE TABLE IF NOT EXISTS session_logs (
    id                  SERIAL PRIMARY KEY,
    session_id          INT NOT NULL REFERENCES recording_sessions(id) ON DELETE CASCADE,
    predicted_bias      VARCHAR(10) NOT NULL,       -- "UP" or "DOWN" from macro warm-up
    macd_at_start       DOUBLE PRECISION,            -- MACD(3,10,16) histogram at session start
    rsi_at_start        DOUBLE PRECISION,            -- RSI(14) at session start
    vfi_at_start        DOUBLE PRECISION,            -- VFI at session start
    macro_slope         DOUBLE PRECISION,            -- SMA200 slope from linear regression
    btc_price_at_start  DOUBLE PRECISION,            -- BTC price when session began
    actual_outcome      VARCHAR(10),                 -- "up", "down", "tie" — NULL while pending
    accuracy_success    BOOLEAN,                     -- NULL while pending, true/false on complete
    cp_quantile         DOUBLE PRECISION DEFAULT 0.05, -- current CP 95% confidence quantile
    cp_alpha            DOUBLE PRECISION DEFAULT 0.1,  -- Robbins-Monro learning rate α
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sl_session   ON session_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_sl_accuracy  ON session_logs(accuracy_success)
    WHERE accuracy_success IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sl_created   ON session_logs(created_at DESC);
