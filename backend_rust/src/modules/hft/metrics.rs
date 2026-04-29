use std::collections::VecDeque;
use std::sync::Mutex;

use crate::modules::core::worker::PriceLevel;
use crate::modules::hft::types::{BinanceDepth, HftMetrics, PriceRingBuffer};

/// Ventana deslizante para VPIN (rolling window de las últimas N muestras)
pub struct VpinWindow {
    buffer:   VecDeque<f64>,
    max_size: usize,
    sum:      f64,
}

impl VpinWindow {
    pub fn new(max_size: usize) -> Self {
        Self { buffer: VecDeque::with_capacity(max_size), max_size, sum: 0.0 }
    }

    pub fn push(&mut self, val: f64) {
        if self.buffer.len() >= self.max_size {
            if let Some(old) = self.buffer.pop_front() {
                self.sum -= old;
            }
        }
        self.buffer.push_back(val);
        self.sum += val;
    }

    pub fn vpin(&self) -> f64 {
        if self.buffer.is_empty() { return 0.0; }
        self.sum / self.buffer.len() as f64
    }
}

/// Estado global de VPIN (compartido entre capturas)
pub struct VpinState {
    pub window: Mutex<VpinWindow>,
}

impl VpinState {
    pub fn new(window_size: usize) -> Self {
        Self { window: Mutex::new(VpinWindow::new(window_size)) }
    }
}

/// Calcula micro-price (volume-weighted mid-price) de un libro de órdenes.
pub fn micro_price(best_bid: f64, best_ask: f64, bid_vol_top5: f64, ask_vol_top5: f64) -> f64 {
    let total_vol = bid_vol_top5 + ask_vol_top5;
    if total_vol <= 0.0 { return (best_bid + best_ask) / 2.0; }
    (ask_vol_top5 * best_bid + bid_vol_top5 * best_ask) / total_vol
}

/// Volume Buy-Sell imbalance (VBS). Rango: -1 (puro sell) a +1 (puro buy)
pub fn vbs(bid_vol: f64, ask_vol: f64) -> f64 {
    let total = bid_vol + ask_vol;
    if total <= 0.0 { return 0.0; }
    (bid_vol - ask_vol) / total
}

/// Depth ratio: bid_volume / ask_volume
pub fn depth_ratio(bid_vol: f64, ask_vol: f64) -> f64 {
    if ask_vol <= 0.0 { return if bid_vol > 0.0 { f64::INFINITY } else { 1.0 }; }
    bid_vol / ask_vol
}

pub fn spread(best_bid: f64, best_ask: f64) -> f64 { best_ask - best_bid }

fn sum_vol(levels: &[PriceLevel], n: usize) -> f64 {
    levels.iter().take(n).map(|l| l.size).sum()
}

/// Calcula todas las métricas HFT a partir de los snapshots de ambos libros
/// + look-back en el ring buffer de Binance.
///
/// - `binance`: snapshot actual completo de Binance (latest depth)
/// - `ring`: ring buffer con histórico de estados compactos de Binance
/// - `poly_event_ts`: timestamp local del evento de Polymarket (ms)
/// - `poly_bids`, `poly_asks`: snapshot del Polymarket
/// - `vpin_state`: estado acumulado de VPIN
pub fn compute_hft_metrics(
    binance:       &BinanceDepth,
    ring:          &PriceRingBuffer,
    poly_event_ts: i64,
    poly_bids:     &[PriceLevel],
    poly_asks:     &[PriceLevel],
    vpin_state:    &VpinState,
) -> HftMetrics {
    // ─── Binance depth ──────────────────────────────────────────────
    let bb_bid = binance.bids.first().map(|l| l.price).unwrap_or(0.0);
    let bb_ask = binance.asks.first().map(|l| l.price).unwrap_or(0.0);
    let bb_spread = spread(bb_bid, bb_ask);

    let binance_bid_vol_5  = sum_vol(&binance.bids, 5);
    let binance_ask_vol_5  = sum_vol(&binance.asks, 5);
    let binance_bid_vol_10 = sum_vol(&binance.bids, 10);
    let binance_ask_vol_10 = sum_vol(&binance.asks, 10);
    let binance_bid_vol_20 = sum_vol(&binance.bids, 20);
    let binance_ask_vol_20 = sum_vol(&binance.asks, 20);

    let binance_vbs_val         = vbs(binance_bid_vol_20, binance_ask_vol_20);
    let binance_depth_ratio_val = depth_ratio(binance_bid_vol_20, binance_ask_vol_20);
    let btc_price_binance       = if bb_bid > 0.0 && bb_ask > 0.0 { (bb_bid + bb_ask) / 2.0 } else { 0.0 };
    let binance_micro_price     = micro_price(bb_bid, bb_ask, binance_bid_vol_5, binance_ask_vol_5);

    // VPIN
    {
        let mut window = vpin_state.window.lock().unwrap();
        window.push(binance_vbs_val.abs());
    }
    let binance_vpin_val = vpin_state.window.lock().unwrap().vpin();

    let latency_delta = (binance.local_time - binance.event_time) as f64;

    // ─── Look-back: ring buffer ─────────────────────────────────────
    let (binance_lag_ms, binance_micro_price_at_t) = if let Some(hist_state) = ring.get_closest_to(poly_event_ts as u64) {
        let lag = poly_event_ts - hist_state.timestamp as i64;
        (lag, hist_state.micro_price)
    } else {
        (0, binance_micro_price)
    };

    // ─── Polymarket depth ───────────────────────────────────────────
    let pb_bid = poly_bids.first().map(|l| l.price).unwrap_or(0.0);
    let pb_ask = poly_asks.first().map(|l| l.price).unwrap_or(0.0);
    let poly_spread_val = spread(pb_bid, pb_ask);
    let poly_mid_price  = if pb_bid > 0.0 && pb_ask > 0.0 { (pb_bid + pb_ask) / 2.0 } else { 0.0 };

    let poly_bid_vol   = sum_vol(poly_bids, 5);
    let poly_ask_vol   = sum_vol(poly_asks, 5);
    let poly_imbalance = depth_ratio(poly_bid_vol, poly_ask_vol);
    let poly_vbs_val   = vbs(poly_bid_vol, poly_ask_vol);
    let poly_micro     = micro_price(pb_bid, pb_ask, poly_bid_vol, poly_ask_vol);

    HftMetrics {
        btc_price_binance,
        binance_mid_price:    btc_price_binance,
        binance_micro_price,
        binance_vbs:          binance_vbs_val,
        binance_vpin:         binance_vpin_val,
        binance_depth_ratio:  binance_depth_ratio_val,
        binance_spread:       bb_spread,
        binance_bid_vol_5,
        binance_ask_vol_5,
        binance_bid_vol_10,
        binance_ask_vol_10,
        binance_bid_vol_20,
        binance_ask_vol_20,
        binance_event_time:   binance.event_time,
        latency_delta,
        binance_lag_ms,
        binance_micro_price_at_t,
        poly_mid_price,
        poly_micro_price:     poly_micro,
        poly_spread:          poly_spread_val,
        poly_imbalance,
        poly_vbs:             poly_vbs_val,
    }
}
