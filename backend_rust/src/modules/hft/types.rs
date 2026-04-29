use serde::Serialize;
use crate::modules::core::worker::PriceLevel;

/// Snapshot del order book de Binance (top 20 niveles)
#[derive(Debug, Clone, Serialize)]
pub struct BinanceDepth {
    pub last_update_id: u64,
    pub bids:           Vec<PriceLevel>,
    pub asks:           Vec<PriceLevel>,
    pub event_time:     i64,  // Binance E (ms)
    pub local_time:     i64,  // nuestra máquina (ms)
    pub btc_price:      f64,  // último precio del ticker
    pub btc_volume_24h: f64,  // volumen 24h del ticker (para VPIN)
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
    pub btc_price_binance:    f64,  // mid-price del libro Binance
    pub binance_mid_price:    f64,  // (best_bid + best_ask) / 2
    pub binance_micro_price:  f64,  // volume-weighted mid
    pub binance_vbs:          f64,  // Volume Buy-Sell imbalance
    pub binance_vpin:         f64,  // VPIN acumulado (rolling window)
    pub binance_depth_ratio:  f64,  // bid_vol / ask_vol (top 20)
    pub binance_spread:       f64,  // ask - bid
    pub binance_bid_vol_5:    f64,
    pub binance_ask_vol_5:    f64,
    pub binance_bid_vol_10:   f64,
    pub binance_ask_vol_10:   f64,
    pub binance_bid_vol_20:   f64,
    pub binance_ask_vol_20:   f64,
    pub binance_event_time:   i64,  // ms del exchange
    pub latency_delta:        f64,  // ms (local - binance event)

    // ─── Polymarket (DEX) ───────────────────────────────────────────
    pub poly_mid_price:       f64,
    pub poly_micro_price:     f64,
    pub poly_spread:          f64,
    pub poly_imbalance:       f64,  // bid_vol / ask_vol ratio
    pub poly_vbs:             f64,
}
