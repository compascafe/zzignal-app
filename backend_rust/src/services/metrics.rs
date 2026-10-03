use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{FixedOffset, Utc};

use crate::controllers::worker::PriceLevel;
use crate::models::hft::{BinanceDepth, CsvRecord, EventType, PriceRingBuffer};

/// Shared tracking state for the microstructure pipeline.
pub struct TrackingState {
    /// `(timestamp_ms, price)` pairs for price-velocity computation
    /// (fallback when the ring buffer has no sample close enough).
    pub price_history: Mutex<VecDeque<(i64, f64)>>,

    /// Previous `poly_ask_vol_all` — used for spoofing detection.
    pub last_poly_ask_vol: Mutex<Option<f64>>,
}

impl Default for TrackingState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackingState {
    pub fn new() -> Self {
        Self {
            price_history: Mutex::new(VecDeque::with_capacity(128)),
            last_poly_ask_vol: Mutex::new(None),
        }
    }

    /// Record a Binance price sample for velocity tracking (500 ms window).
    pub fn record_price_sample(&self, ts_ms: i64, price: f64) {
        let mut w = self.price_history.lock().unwrap();
        w.push_back((ts_ms, price));
        let cutoff = ts_ms - 500;
        while w.front().is_some_and(|&(t, _)| t < cutoff) {
            w.pop_front();
        }
    }

    /// Compute price velocity: slope of Binance mid price over 1 s (USD/s).
    /// Primary: ring-buffer look-back. Fallback: `price_history`.
    pub fn price_velocity(&self, ring: &PriceRingBuffer, now_ms: i64, current_price: f64) -> f64 {
        let target_ts = (now_ms as u64).saturating_sub(1000);
        if let Some(past) = ring.get_closest_to(target_ts) {
            if past.timestamp > 0 && past.mid_price > 0.0 {
                let dt_ms = ((now_ms as u64).saturating_sub(past.timestamp)).max(1) as f64;
                let dp = current_price - past.mid_price;
                return dp / (dt_ms / 1000.0);
            }
        }
        // Fallback: price_history (always populated by Binance ticks)
        let hist = self.price_history.lock().unwrap();
        if hist.len() < 2 {
            return 0.0;
        }
        let cutoff = now_ms - 1000;
        let oldest = hist
            .iter()
            .filter(|(ts, _)| *ts >= cutoff)
            .min_by_key(|(ts, _)| *ts);
        let newest = hist.back();
        if let (Some((t1, p1)), Some(&(t2, p2))) = (oldest, newest) {
            let dt_ms = (t2 - t1).max(1) as f64;
            return (p2 - p1) / (dt_ms / 1000.0);
        }
        0.0
    }

    /// Store current poly ask volume for next-tick spoofing detection.
    pub fn set_last_poly_ask_vol(&self, vol: f64) {
        *self.last_poly_ask_vol.lock().unwrap() = Some(vol);
    }

    /// Get stored poly ask volume from the previous tick.
    pub fn get_last_poly_ask_vol(&self) -> Option<f64> {
        *self.last_poly_ask_vol.lock().unwrap()
    }
}

// ─── Calculation helpers ───────────────────────────────────────────────────────

pub fn depth_ratio(bid_vol: f64, ask_vol: f64) -> f64 {
    if ask_vol <= 0.0 {
        return if bid_vol > 0.0 { f64::INFINITY } else { 1.0 };
    }
    bid_vol / ask_vol
}

pub fn spread(best_bid: f64, best_ask: f64) -> f64 {
    best_ask - best_bid
}

fn sum_vol(levels: &[PriceLevel], n: usize) -> f64 {
    if n == 0 {
        levels.iter().map(|l| l.size).sum()
    } else {
        levels.iter().take(n).map(|l| l.size).sum()
    }
}

/// Spoofing detection: >50% ask-volume drop vs. the previous tick with no
/// Polymarket trade in between.
pub fn compute_spoofing(current_ask_vol: f64, tracking: &TrackingState, is_poly_trade: bool) -> u8 {
    let prev = tracking.get_last_poly_ask_vol();
    if let Some(p) = prev {
        if p > 0.0 && !is_poly_trade {
            let delta = current_ask_vol - p;
            if delta < 0.0 && (delta.abs() / p) > 0.50 {
                return 1;
            }
        }
    }
    0
}

// ─── Record builders ───────────────────────────────────────────────────────────

/// Build a BOOK_UPDATE CsvRecord from the Polymarket book and Binance depth.
pub fn build_book_update(
    binance: &BinanceDepth,
    ring: &PriceRingBuffer,
    poly_bids: &[PriceLevel],
    poly_asks: &[PriceLevel],
    tracking: &TrackingState,
    poly_event_ts: i64,
) -> CsvRecord {
    let now = Utc::now();
    let lima = FixedOffset::west_opt(5 * 3600).unwrap();
    let ts_str = now
        .with_timezone(&lima)
        .format("%Y-%m-%dT%H:%M:%S%.3f-05:00")
        .to_string();

    let bb_bid = binance.bids.first().map(|l| l.price).unwrap_or(0.0);
    let bb_ask = binance.asks.first().map(|l| l.price).unwrap_or(0.0);
    let bb_mid = if bb_bid > 0.0 && bb_ask > 0.0 {
        (bb_bid + bb_ask) / 2.0
    } else {
        0.0
    };
    let bb_bid_vol_20 = sum_vol(&binance.bids, 20);
    let bb_ask_vol_20 = sum_vol(&binance.asks, 20);
    let bb_imb = {
        let total = bb_bid_vol_20 + bb_ask_vol_20;
        if total > 0.0 {
            ((bb_bid_vol_20 - bb_ask_vol_20) / total) as f32
        } else {
            0.0
        }
    };

    // Ring-buffer look-back: cross-exchange latency (Binance depth → Poly event)
    let lag_ms = if let Some(hist) = ring.get_closest_to(poly_event_ts as u64) {
        poly_event_ts - hist.timestamp as i64
    } else {
        0
    };

    let pb_bid = poly_bids.first().map(|l| l.price).unwrap_or(0.0);
    let pb_ask = poly_asks.first().map(|l| l.price).unwrap_or(0.0);
    // Poly mid: one-sided fallback for thin order books (common on Polymarket)
    let pb_mid = if pb_bid > 0.0 && pb_ask > 0.0 {
        (pb_bid + pb_ask) / 2.0
    } else if pb_bid > 0.0 {
        pb_bid
    } else if pb_ask > 0.0 {
        pb_ask
    } else {
        0.0
    };
    let pb_sprd = spread(pb_bid, pb_ask);
    let pb_bid_vol = sum_vol(poly_bids, 0);
    let pb_ask_vol = sum_vol(poly_asks, 0);
    let pb_imb = depth_ratio(pb_bid_vol, pb_ask_vol);

    let price_vel = tracking.price_velocity(ring, now.timestamp_millis(), bb_mid);
    let spoof_flag = compute_spoofing(pb_ask_vol, tracking, false);
    tracking.set_last_poly_ask_vol(pb_ask_vol);

    CsvRecord {
        ts_local: ts_str,
        ts_exchange: binance.event_time.to_string(),
        event_type: EventType::BookUpdate,
        latencia_ms: lag_ms,
        binance_price: bb_mid,
        binance_imbalance: bb_imb,
        binance_vol_24h: binance.btc_volume_24h,
        poly_bid: pb_bid,
        poly_ask: pb_ask,
        poly_mid: pb_mid,
        poly_spread: pb_sprd,
        poly_bid_vol_all: pb_bid_vol,
        poly_ask_vol_all: pb_ask_vol,
        poly_imbalance: if pb_imb.is_finite() { pb_imb } else { 0.0 },
        price_velocity: price_vel,
        spoofing_flag: spoof_flag,
        ..Default::default()
    }
}

/// Build a TRADE CsvRecord from a Polymarket fill.
pub fn build_trade_record(
    binance: &BinanceDepth,
    ring: &PriceRingBuffer,
    poly_bids: &[PriceLevel],
    poly_asks: &[PriceLevel],
    tracking: &TrackingState,
    trade_ts: i64,
) -> CsvRecord {
    let mut rec = build_book_update(binance, ring, poly_bids, poly_asks, tracking, trade_ts);
    rec.event_type = EventType::Trade;

    let pb_ask_vol = poly_asks.iter().map(|l| l.size).sum::<f64>();
    rec.spoofing_flag = compute_spoofing(pb_ask_vol, tracking, true);
    tracking.set_last_poly_ask_vol(pb_ask_vol);
    rec
}

/// Build a BINANCE_TICK CsvRecord (CEX-only data).
pub fn build_binance_tick(
    binance: &BinanceDepth,
    ring: &PriceRingBuffer,
    tracking: &TrackingState,
    tick_ts: i64,
    tick_price: f64,
) -> CsvRecord {
    let now = Utc::now();
    let lima = FixedOffset::west_opt(5 * 3600).unwrap();
    let ts_str = now
        .with_timezone(&lima)
        .format("%Y-%m-%dT%H:%M:%S%.3f-05:00")
        .to_string();

    let bb_bid = binance.bids.first().map(|l| l.price).unwrap_or(0.0);
    let bb_ask = binance.asks.first().map(|l| l.price).unwrap_or(0.0);
    let bb_mid = if bb_bid > 0.0 && bb_ask > 0.0 {
        (bb_bid + bb_ask) / 2.0
    } else {
        tick_price
    };
    let bb_bid_vol_20 = sum_vol(&binance.bids, 20);
    let bb_ask_vol_20 = sum_vol(&binance.asks, 20);
    let bb_imb = {
        let total = bb_bid_vol_20 + bb_ask_vol_20;
        if total > 0.0 {
            ((bb_bid_vol_20 - bb_ask_vol_20) / total) as f32
        } else {
            0.0
        }
    };

    CsvRecord {
        ts_local: ts_str,
        ts_exchange: tick_ts.to_string(),
        event_type: EventType::BinanceTick,
        latencia_ms: now.timestamp_millis() - tick_ts,
        binance_price: bb_mid,
        binance_imbalance: bb_imb,
        binance_vol_24h: binance.btc_volume_24h,
        price_velocity: tracking.price_velocity(ring, now.timestamp_millis(), bb_mid),
        ..Default::default()
    }
}
