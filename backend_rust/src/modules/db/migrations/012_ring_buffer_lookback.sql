-- Migration 012: Look-back Ring Buffer columns
-- Añade columnas para el look-back del ring buffer de Binance:
--   binance_lag_ms         = diferencia temporal entre Poly y Binance (ms)
--   binance_micro_price_at_t = micro-price en el instante histórico más cercano

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_lag_ms') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_lag_ms BIGINT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='session_snapshots' AND column_name='binance_micro_price_at_t') THEN
        ALTER TABLE session_snapshots ADD COLUMN binance_micro_price_at_t DOUBLE PRECISION;
    END IF;

    -- hft_snapshots también recibe las nuevas columnas
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='hft_snapshots' AND column_name='binance_lag_ms') THEN
        ALTER TABLE hft_snapshots ADD COLUMN binance_lag_ms BIGINT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name='hft_snapshots' AND column_name='binance_micro_price_at_t') THEN
        ALTER TABLE hft_snapshots ADD COLUMN binance_micro_price_at_t DOUBLE PRECISION;
    END IF;
END $$;
