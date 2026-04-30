use std::sync::Mutex;

use chrono::Utc;

use crate::modules::core::worker::PriceLevel;
use crate::modules::hft::types::{BinanceDepth, CsvRecord, EventType, PriceRingBuffer};

/// Welford's online algorithm para running mean + std deviation del volumen.
pub(super) struct RunningStats {
    count: u64,
    mean:  f64,
    m2:    f64,  // sum of squared differences
}

impl RunningStats {
    fn new() -> Self { Self { count: 0, mean: 0.0, m2: 0.0 } }

    fn push(&mut self, val: f64) {
        self.count += 1;
        let delta   = val - self.mean;
        self.mean  += delta / self.count as f64;
        let delta2  = val - self.mean;
        self.m2    += delta * delta2;
    }

    fn std_dev(&self) -> f64 {
        if self.count < 2 { return 0.0; }
        (self.m2 / (self.count - 1) as f64).sqrt()
    }

    fn is_spike(&self, val: f64, n_sigmas: f64) -> bool {
        if self.count < 10 { return false; } // need warmup
        val > self.mean + n_sigmas * self.std_dev()
    }
}

/// Estado de tracking compartido: volumen deslizante, detección de big moves y spikes.
pub struct TrackingState {
    pub vol_window:       Mutex<Vec<(i64, f64)>>,
    pub last_big_move:    Mutex<Option<i64>>,
    pub last_binance_price: Mutex<Option<f64>>,
    pub(super) vol_stats:  Mutex<RunningStats>,
}

impl TrackingState {
    pub fn new() -> Self {
        Self {
            vol_window:         Mutex::new(Vec::with_capacity(64)),
            last_big_move:      Mutex::new(None),
            last_binance_price: Mutex::new(None),
            vol_stats:          Mutex::new(RunningStats::new()),
        }
    }

    pub fn push_volume(&self, ts_ms: i64, vol: f64) {
        if vol <= 0.0 { return; }
        self.vol_window.lock().unwrap().push((ts_ms, vol));
        self.vol_stats.lock().unwrap().push(vol);
    }

    pub fn vol_100ms(&self, now_ms: i64) -> f64 {
        let mut w = self.vol_window.lock().unwrap();
        let cutoff = now_ms - 100;
        w.retain(|(ts, _)| *ts >= cutoff);
        w.iter().map(|(_, v)| *v).sum()
    }

    /// Registra precio. Si delta > $1.00, marca big_move.
    pub fn track_price(&self, price: f64, now_ms: i64) {
        let mut last = self.last_binance_price.lock().unwrap();
        if let Some(prev) = *last {
            if (price - prev).abs() > 1.0 {
                *self.last_big_move.lock().unwrap() = Some(now_ms);
            }
        }
        *last = Some(price);
    }

    /// Flag de trading informado: 1 si hubo big move (>$1.00) O spike de volumen (>2σ) en últimos 100ms.
    pub fn is_informed(&self, now_ms: i64) -> u8 {
        // Check big move
        let bm = self.last_big_move.lock().unwrap();
        if let Some(ts) = *bm {
            if now_ms - ts <= 100 { return 1; }
        }
        // Check volume spike: último volumen > mean + 2σ
        let vol_now = self.vol_100ms(now_ms);
        let stats = self.vol_stats.lock().unwrap();
        if stats.is_spike(vol_now, 2.0) {
            return 1;
        }
        0
    }
}

// ─── Funciones de cálculo ─────────────────────────────────────────────────

pub fn micro_price(best_bid: f64, best_ask: f64, bid_vol_top5: f64, ask_vol_top5: f64) -> f64 {
    let total_vol = bid_vol_top5 + ask_vol_top5;
    if total_vol <= 0.0 { return (best_bid + best_ask) / 2.0; }
    (ask_vol_top5 * best_bid + bid_vol_top5 * best_ask) / total_vol
}

pub fn depth_ratio(bid_vol: f64, ask_vol: f64) -> f64 {
    if ask_vol <= 0.0 { return if bid_vol > 0.0 { f64::INFINITY } else { 1.0 }; }
    bid_vol / ask_vol
}

pub fn spread(best_bid: f64, best_ask: f64) -> f64 { best_ask - best_bid }

fn sum_vol(levels: &[PriceLevel], n: usize) -> f64 {
    if n == 0 { levels.iter().map(|l| l.size).sum() }
    else { levels.iter().take(n).map(|l| l.size).sum() }
}

/// Construye un CsvRecord de tipo BOOK_UPDATE a partir de los snapshots de ambos libros.
pub fn build_book_update(
    binance:        &BinanceDepth,
    ring:           &PriceRingBuffer,
    poly_bids:      &[PriceLevel],
    poly_asks:      &[PriceLevel],
    tracking:       &TrackingState,
    poly_event_ts:  i64,
) -> CsvRecord {
    let now = Utc::now();

    let bb_bid  = binance.bids.first().map(|l| l.price).unwrap_or(0.0);
    let bb_ask  = binance.asks.first().map(|l| l.price).unwrap_or(0.0);
    let bb_mid  = if bb_bid > 0.0 && bb_ask > 0.0 { (bb_bid + bb_ask) / 2.0 } else { 0.0 };
    let _bb_sprd = spread(bb_bid, bb_ask);
    let bb_bid_vol_5  = sum_vol(&binance.bids, 5);
    let bb_ask_vol_5  = sum_vol(&binance.asks, 5);
    let bb_bid_vol_20 = sum_vol(&binance.bids, 20);
    let bb_ask_vol_20 = sum_vol(&binance.asks, 20);
    let bb_mic = micro_price(bb_bid, bb_ask, bb_bid_vol_5, bb_ask_vol_5);
    let bb_imb = {
        let total = bb_bid_vol_20 + bb_ask_vol_20;
        if total > 0.0 { ((bb_bid_vol_20 - bb_ask_vol_20) / total) as f32 } else { 0.0 }
    };

    // Look-back en ring buffer
    let (lag_ms, mic_at_t, bn_vol_100) = if let Some(hist) = ring.get_closest_to(poly_event_ts as u64) {
        (poly_event_ts - hist.timestamp as i64, hist.micro_price, hist.binance_vol_100ms)
    } else {
        (0, bb_mic, 0.0)
    };

    let pb_bid  = poly_bids.first().map(|l| l.price).unwrap_or(0.0);
    let pb_ask  = poly_asks.first().map(|l| l.price).unwrap_or(0.0);
    let pb_mid  = if pb_bid > 0.0 && pb_ask > 0.0 { (pb_bid + pb_ask) / 2.0 } else { 0.0 };
    let pb_sprd = spread(pb_bid, pb_ask);
    let pb_bid_vol = sum_vol(poly_bids, 0);
    let pb_ask_vol = sum_vol(poly_asks, 0);
    let pb_imb = depth_ratio(pb_bid_vol, pb_ask_vol);

    CsvRecord {
        ts_local:            now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        ts_exchange:         binance.event_time.to_string(),
        event_type:          EventType::BookUpdate,
        latencia_ms:         lag_ms,
        binance_price:       bb_mid,
        binance_micro_price: mic_at_t,
        binance_imbalance:   bb_imb,
        binance_vol_100ms:   bn_vol_100,
        binance_vol_24h:     binance.btc_volume_24h,
        poly_bid:            pb_bid,
        poly_ask:            pb_ask,
        poly_mid:            pb_mid,
        poly_spread:         pb_sprd,
        poly_bid_vol_all:    pb_bid_vol,
        poly_ask_vol_all:    pb_ask_vol,
        poly_imbalance:      if pb_imb.is_finite() { pb_imb } else { 0.0 },
        trade_side:          String::new(),
        trade_price:         0.0,
        trade_size:          0.0,
        is_informed:         tracking.is_informed(now.timestamp_millis()),
        ..Default::default()
    }
}

/// Construye un CsvRecord de tipo TRADE a partir de un fill de Polymarket.
pub fn build_trade_record(
    binance:       &BinanceDepth,
    ring:          &PriceRingBuffer,
    poly_bids:     &[PriceLevel],
    poly_asks:     &[PriceLevel],
    tracking:      &TrackingState,
    trade_ts:      i64,
    trade_side:    &str,
    trade_price:   f64,
    trade_size:    f64,
) -> CsvRecord {
    let mut rec = build_book_update(binance, ring, poly_bids, poly_asks, tracking, trade_ts);
    rec.event_type  = EventType::Trade;
    rec.trade_side  = trade_side.to_string();
    rec.trade_price = trade_price;
    rec.trade_size  = trade_size;
    rec
}

/// Construye un CsvRecord de tipo BINANCE_TICK (solo datos del CEX).
pub fn build_binance_tick(
    binance:       &BinanceDepth,
    tracking:      &TrackingState,
    tick_ts:       i64,
    tick_price:    f64,
    _tick_volume:   f64,
) -> CsvRecord {
    let now = Utc::now();

    let bb_bid  = binance.bids.first().map(|l| l.price).unwrap_or(0.0);
    let bb_ask  = binance.asks.first().map(|l| l.price).unwrap_or(0.0);
    let bb_mid  = if bb_bid > 0.0 && bb_ask > 0.0 { (bb_bid + bb_ask) / 2.0 } else { tick_price };
    let bb_bid_vol_5  = sum_vol(&binance.bids, 5);
    let bb_ask_vol_5  = sum_vol(&binance.asks, 5);
    let bb_bid_vol_20 = sum_vol(&binance.bids, 20);
    let bb_ask_vol_20 = sum_vol(&binance.asks, 20);
    let bb_mic = micro_price(bb_bid, bb_ask, bb_bid_vol_5, bb_ask_vol_5);
    let bb_imb = {
        let total = bb_bid_vol_20 + bb_ask_vol_20;
        if total > 0.0 { ((bb_bid_vol_20 - bb_ask_vol_20) / total) as f32 } else { 0.0 }
    };

    CsvRecord {
        ts_local:            now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        ts_exchange:         tick_ts.to_string(),
        event_type:          EventType::BinanceTick,
        latencia_ms:         now.timestamp_millis() - tick_ts,
        binance_price:       bb_mid,
        binance_micro_price: bb_mic,
        binance_imbalance:   bb_imb,
        binance_vol_100ms:   tracking.vol_100ms(now.timestamp_millis()),
        binance_vol_24h:     binance.btc_volume_24h,
        ..Default::default()
    }
}
