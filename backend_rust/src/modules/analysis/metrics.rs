//! Analysis Metrics — Cómputo de métricas de mercado
//!
//! Recibe datos crudos (orderbook + binance) y produce métricas.
//! Todas las funciones son puras o toman referencias inmutables.

use crate::modules::core::worker::PriceLevel;
use crate::modules::data::poly_orderbook::TopOfBook;

// ─── Orderbook Metrics ─────────────────────────────────────────────────────

/// Calcula el mid price con fallback one-sided para books finos.
/// Si ambos lados existen: (bid + ask) / 2.
/// Si solo uno: ese precio.
pub fn poly_mid(best_bid: f64, best_ask: f64) -> f64 {
    if best_bid > 0.0 && best_ask > 0.0 {
        (best_bid + best_ask) / 2.0
    } else if best_bid > 0.0 {
        best_bid
    } else if best_ask > 0.0 {
        best_ask
    } else {
        0.0
    }
}

/// Spread absoluto y porcentual.
pub fn poly_spread(best_bid: f64, best_ask: f64) -> (f64, f64) {
    let abs_spread = if best_bid > 0.0 && best_ask > 0.0 {
        best_ask - best_bid
    } else {
        0.0
    };
    let mid = poly_mid(best_bid, best_ask);
    let pct = if mid > 0.0 { abs_spread / mid } else { 0.0 };
    (abs_spread, pct)
}

/// Suma de volúmenes de N niveles (0 = todos).
pub fn sum_vol(levels: &[PriceLevel], n: usize) -> f64 {
    if n == 0 {
        levels.iter().map(|l| l.size).sum()
    } else {
        levels.iter().take(n).map(|l| l.size).sum()
    }
}

/// Ratio de imbalance: bid_vol / ask_vol.
pub fn depth_ratio(bid_vol: f64, ask_vol: f64) -> f64 {
    if ask_vol <= 0.0 {
        if bid_vol > 0.0 { f64::INFINITY } else { 1.0 }
    } else {
        bid_vol / ask_vol
    }
}

/// Price velocity: cambio de precio por segundo (USD/s).
pub fn price_velocity(price: f64, prev_price: f64, dt_secs: f64) -> f64 {
    if dt_secs > 0.0 { (price - prev_price) / dt_secs } else { 0.0 }
}

/// Absorption ratio: trade_vol / |Δmid|.
/// Alto → el mercado absorbió el trade sin mover precio.
pub fn absorption_ratio(trade_vol: f64, delta_mid: f64) -> f64 {
    if delta_mid.abs() > 0.0 { trade_vol / delta_mid.abs() } else { 0.0 }
}

/// Price gap ratio: divergencia % entre Binance y Polymarket.
pub fn price_gap_ratio(binance_price: f64, poly_mid: f64) -> f64 {
    if binance_price > 0.0 {
        ((binance_price - poly_mid).abs() / binance_price) * 100.0
    } else {
        0.0
    }
}

/// Liquidity delta: cambio en ask volume respecto al tick anterior.
pub fn liquidity_delta(current_ask_vol: f64, prev_ask_vol: f64) -> f64 {
    current_ask_vol - prev_ask_vol
}

/// Compute all top-of-book metrics from a TopOfBook snapshot.
pub fn compute_top_metrics(tob: &TopOfBook) -> OrderbookMetrics {
    OrderbookMetrics {
        best_bid: tob.best_bid,
        best_ask: tob.best_ask,
        mid: poly_mid(tob.best_bid, tob.best_ask),
        spread: tob.spread,
        bid_vol: tob.bid_vol,
        ask_vol: tob.ask_vol,
        imbalance: tob.imbalance,
        ts_unix_ms: tob.ts_unix_ms,
    }
}

// ─── Market Pressure ───────────────────────────────────────────────────────

/// Market Pressure metrics: effective spread, pressure index, volume skew.
/// Filters out dust orders (vol < MIN_VOL) to find where real liquidity sits.
pub struct PressureMetrics {
    pub bid_floor:     f64,  // lowest bid price with meaningful volume
    pub ask_ceiling:   f64,  // highest ask price with meaningful volume
    pub band:          f64,  // effective spread: ceiling - floor
    pub index:         f64,  // (mid - floor) / band: 0=DOWN pressure, 1=UP pressure
    pub skew:          f64,  // (bid_vol - ask_vol) / total within band: + buy, - sell
}

/// Compute pressure metrics from full orderbook depth.
///
/// Steps:
///   1. Find bid_floor = lowest bid price with size >= min_vol (default 10)
///   2. Find ask_ceiling = highest ask price with size >= min_vol
///   3. band = ask_ceiling - bid_floor (if valid, else 0)
///   4. index = (mid - bid_floor) / band (clamped to [0,1])
///   5. skew = (bid_vol_in_band - ask_vol_in_band) / total_vol_in_band
pub fn compute_pressure(
    bids: &[PriceLevel], asks: &[PriceLevel],
    mid: f64, min_vol: f64,
) -> PressureMetrics {
    // Bid floor: lowest (worst) price that still has meaningful size
    let bid_floor = bids.iter()
        .filter(|l| l.size >= min_vol)
        .map(|l| l.price)
        .fold(f64::NAN, f64::min); // lowest price = furthest from 1.0

    // Ask ceiling: highest (worst) price that still has meaningful size
    let ask_ceiling = asks.iter()
        .filter(|l| l.size >= min_vol)
        .map(|l| l.price)
        .fold(f64::NAN, f64::max); // highest price = furthest from 0.0

    if bid_floor.is_nan() || ask_ceiling.is_nan() || ask_ceiling <= bid_floor {
        return PressureMetrics {
            bid_floor: if bid_floor.is_nan() { 0.0 } else { bid_floor },
            ask_ceiling: if ask_ceiling.is_nan() { 1.0 } else { ask_ceiling },
            band: 0.0, index: 0.5, skew: 0.0,
        };
    }

    let band = ask_ceiling - bid_floor;

    // Pressure index: where is mid within the band?
    let index = if band > 0.0 {
        ((mid - bid_floor) / band).clamp(0.0, 1.0)
    } else {
        0.5
    };

    // Volume skew within the band
    let bid_vol_in_band: f64 = bids.iter()
        .filter(|l| l.price >= bid_floor)
        .map(|l| l.size)
        .sum();
    let ask_vol_in_band: f64 = asks.iter()
        .filter(|l| l.price <= ask_ceiling)
        .map(|l| l.size)
        .sum();
    let total = bid_vol_in_band + ask_vol_in_band;
    let skew = if total > 0.0 {
        (bid_vol_in_band - ask_vol_in_band) / total
    } else {
        0.0
    };

    PressureMetrics {
        bid_floor, ask_ceiling, band, index, skew,
    }
}

// ─── Metric Snapshots ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OrderbookMetrics {
    pub best_bid:    f64,
    pub best_ask:    f64,
    pub mid:         f64,
    pub spread:      f64,
    pub bid_vol:     f64,
    pub ask_vol:     f64,
    pub imbalance:   f64,
    pub ts_unix_ms:  i64,
}

impl Default for OrderbookMetrics {
    fn default() -> Self {
        Self {
            best_bid: 0.0, best_ask: 0.0, mid: 0.0, spread: 0.0,
            bid_vol: 0.0, ask_vol: 0.0, imbalance: 1.0, ts_unix_ms: 0,
        }
    }
}

/// Snapshot completo de análisis para un tick.
/// Capa intermedia entre Data (Layer 1) y Trading (Layer 3).
#[derive(Debug, Clone)]
pub struct AnalysisSnapshot {
    pub poly_up:   OrderbookMetrics,
    pub poly_down: OrderbookMetrics,
    pub btc_price: f64,
    pub btc_vol_100ms: f64,
    pub btc_vol_24h:  f64,
    pub btc_micro:    f64,
    pub btc_imbalance: f64,
    pub velocity:     f64,
    pub absorption:   f64,
    pub gap_ratio:    f64,
    pub trades_per_second: f64,
    pub latencia_ms:  i64,
    pub ts_unix_ms:   i64,
}

impl Default for AnalysisSnapshot {
    fn default() -> Self {
        Self {
            poly_up: OrderbookMetrics::default(),
            poly_down: OrderbookMetrics::default(),
            btc_price: 0.0, btc_vol_100ms: 0.0, btc_vol_24h: 0.0,
            btc_micro: 0.0, btc_imbalance: 0.0,
            velocity: 0.0, absorption: 0.0, gap_ratio: 0.0,
            trades_per_second: 0.0,
            latencia_ms: 0, ts_unix_ms: 0,
        }
    }
}
