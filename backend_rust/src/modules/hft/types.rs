use serde::Serialize;
use crate::modules::core::worker::PriceLevel;

// Re-export compact state from ring_buffer
pub use crate::modules::hft::ring_buffer::{BinanceState, PriceRingBuffer};

/// Snapshot del order book de Binance (top 20 niveles) — referencia completa
#[derive(Debug, Clone, Serialize)]
pub struct BinanceDepth {
    pub last_update_id: u64,
    pub bids:           Vec<PriceLevel>,
    pub asks:           Vec<PriceLevel>,
    pub event_time:     i64,  // Binance E (ms)
    pub local_time:     i64,  // nuestra máquina (ms)
    pub btc_price:      f64,  // último precio del ticker
    pub btc_volume_24h: f64,  // volumen 24h del ticker
}

impl Default for BinanceDepth {
    fn default() -> Self {
        Self {
            last_update_id: 0,
            bids:           vec![],
            asks:           vec![],
            event_time:     0,
            local_time:     0,
            btc_price:      0.0,
            btc_volume_24h: 0.0,
        }
    }
}

/// Métricas HFT calculadas a partir de los dos libros (Binance + Polymarket)
#[derive(Debug, Clone, Serialize)]
pub struct HftMetrics {
    // ─── Binance (CEX) ──────────────────────────────────────────────
    pub btc_price_binance:    f64,
    pub binance_mid_price:    f64,
    pub binance_micro_price:  f64,
    pub binance_vbs:          f64,
    pub binance_vpin:         f64,
    pub binance_depth_ratio:  f64,
    pub binance_spread:       f64,
    pub binance_bid_vol_5:    f64,
    pub binance_ask_vol_5:    f64,
    pub binance_bid_vol_10:   f64,
    pub binance_ask_vol_10:   f64,
    pub binance_bid_vol_20:   f64,
    pub binance_ask_vol_20:   f64,
    pub binance_event_time:   i64,
    pub latency_delta:        f64,  // ms (local - binance event)

    // ─── Look-back (Ring Buffer) ───────────────────────────────────
    /// Lag entre el evento de Polymarket y el snapshot de Binance más cercano (ms)
    pub binance_lag_ms:       i64,
    /// Micro-price del snapshot de Binance en el instante más cercano al evento Poly
    pub binance_micro_price_at_t: f64,

    // ─── Polymarket (DEX) ───────────────────────────────────────────
    pub poly_mid_price:       f64,
    pub poly_micro_price:     f64,
    pub poly_spread:          f64,
    pub poly_imbalance:       f64,
    pub poly_vbs:             f64,
}
