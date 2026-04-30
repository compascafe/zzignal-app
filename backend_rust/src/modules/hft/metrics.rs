use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::Utc;
use tracing::info;

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

/// Estado de tracking compartido: volumen deslizante, detección de big moves y spikes,
/// más métricas HFT avanzadas: tape speed, price velocity, liquidity delta, gap ratio.
pub struct TrackingState {
    pub vol_window:          Mutex<Vec<(i64, f64)>>,
    pub last_big_move:       Mutex<Option<i64>>,
    pub last_binance_price:  Mutex<Option<f64>>,
    pub(super) vol_stats:    Mutex<RunningStats>,

    // ─── Advanced HFT: tape speed & velocity ─────────────────────────────
    /// Timestamps (ms) of Binance trade events for trades_per_second rolling window.
    pub trade_ts_window:     Mutex<VecDeque<i64>>,
    /// (timestamp_ms, price) pairs for price_velocity computation (500ms window).
    pub price_history:       Mutex<VecDeque<(i64, f64)>>,

    // ─── Advanced HFT: liquidity & absorption ────────────────────────────
    /// Previous poly_ask_vol_all for liquidity delta.
    pub last_poly_ask_vol:   Mutex<Option<f64>>,
    /// Previous poly_mid_price for absorption ratio.
    pub last_poly_mid:       Mutex<Option<f64>>,
    /// Last poly_mid timestamp (ms) for absorption ratio denominator.
    pub last_poly_mid_ts:    Mutex<Option<i64>>,
}

impl TrackingState {
    pub fn new() -> Self {
        Self {
            vol_window:         Mutex::new(Vec::with_capacity(64)),
            last_big_move:      Mutex::new(None),
            last_binance_price: Mutex::new(None),
            vol_stats:          Mutex::new(RunningStats::new()),
            trade_ts_window:    Mutex::new(VecDeque::with_capacity(256)),
            price_history:      Mutex::new(VecDeque::with_capacity(128)),
            last_poly_ask_vol:  Mutex::new(None),
            last_poly_mid:      Mutex::new(None),
            last_poly_mid_ts:   Mutex::new(None),
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

    /// Record a Binance trade timestamp for trades_per_second computation.
    pub fn record_binance_trade(&self, ts_ms: i64) {
        let mut w = self.trade_ts_window.lock().unwrap();
        w.push_back(ts_ms);
        // Prune entries older than 1 second
        let cutoff = ts_ms - 1000;
        while w.front().map_or(false, |&t| t < cutoff) {
            w.pop_front();
        }
    }

    /// Count Binance trades in the last rolling 1 second.
    pub fn trades_per_second(&self, now_ms: i64) -> f64 {
        let mut w = self.trade_ts_window.lock().unwrap();
        let cutoff = now_ms - 1000;
        while w.front().map_or(false, |&t| t < cutoff) {
            w.pop_front();
        }
        w.len() as f64
    }

    /// Record a Binance price sample for velocity tracking.
    pub fn record_price_sample(&self, ts_ms: i64, price: f64) {
        let mut w = self.price_history.lock().unwrap();
        w.push_back((ts_ms, price));
        // Prune older than 500ms
        let cutoff = ts_ms - 500;
        while w.front().map_or(false, |&(t, _)| t < cutoff) {
            w.pop_front();
        }
    }

    /// Compute price velocity: slope of price over the last 500ms window (USD/s).
    /// Uses simple Δprice / Δtime between oldest and newest sample.
    pub fn price_velocity(&self, now_ms: i64) -> f64 {
        let mut w = self.price_history.lock().unwrap();
        let cutoff = now_ms - 500;
        while w.front().map_or(false, |&(t, _)| t < cutoff) {
            w.pop_front();
        }
        if w.len() < 2 {
            return 0.0;
        }
        let first = w.front().unwrap();
        let last = w.back().unwrap();
        let dt_ms = (last.0 - first.0).max(1) as f64;
        let dp = last.1 - first.1;
        dp / (dt_ms / 1000.0) // Convert to USD/sec
    }

    /// Store current poly ask volume for next-tick liquidity delta computation.
    pub fn set_last_poly_ask_vol(&self, vol: f64) {
        *self.last_poly_ask_vol.lock().unwrap() = Some(vol);
    }

    /// Get stored poly ask volume from previous tick.
    pub fn get_last_poly_ask_vol(&self) -> Option<f64> {
        *self.last_poly_ask_vol.lock().unwrap()
    }

    /// Store current poly mid price + timestamp for absorption ratio.
    pub fn set_last_poly_mid(&self, price: f64, ts_ms: i64) {
        *self.last_poly_mid.lock().unwrap() = Some(price);
        *self.last_poly_mid_ts.lock().unwrap() = Some(ts_ms);
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

    // ─── Advanced HFT Metrics (computed before struct to allow state storage) ─
    let trades_ps    = tracking.trades_per_second(now.timestamp_millis());
    let price_vel    = tracking.price_velocity(now.timestamp_millis());
    let (liq_delta, spoof_flag) = compute_liquidity_delta(pb_ask_vol, tracking, false);
    let (gap_pct, gap_flag)     = compute_price_gap(mic_at_t, pb_mid);
    let tape_flag    = check_volume_spike(bn_vol_100, tracking, pb_sprd);
    if gap_flag > 0 { check_gap_alert(gap_pct); }

    // Store current poly state for next-tick delta computations
    tracking.set_last_poly_ask_vol(pb_ask_vol);
    tracking.set_last_poly_mid(pb_mid, now.timestamp_millis());

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
        trades_per_second:   trades_ps,
        price_velocity:      price_vel,
        poly_liquidity_delta: liq_delta,
        absorption_ratio:    0.0, // No trade volume in book update
        price_gap_ratio:     gap_pct,
        spoofing_flag:       spoof_flag,
        tape_speed_flag:     tape_flag,
        gap_alert_flag:      gap_flag,
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
    // Recompute with trade context
    let pb_ask_vol = poly_asks.iter().map(|l| l.size).sum::<f64>();
    let pb_mid = if let (Some(b), Some(a)) = (poly_bids.first(), poly_asks.first()) {
        (b.price + a.price) / 2.0
    } else { 0.0 };
    let (_delta, spoof) = compute_liquidity_delta(pb_ask_vol, tracking, true);
    rec.poly_liquidity_delta = _delta;
    rec.spoofing_flag = spoof;
    rec.absorption_ratio = compute_absorption_ratio(
        trade_size, pb_mid, tracking.last_poly_mid.lock().unwrap().clone(),
    );
    // Store current state for next tick
    tracking.set_last_poly_ask_vol(pb_ask_vol);
    let now_ms = chrono::Utc::now().timestamp_millis();
    tracking.set_last_poly_mid(pb_mid, now_ms);
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

    let bb_vol_100 = tracking.vol_100ms(now.timestamp_millis());
    CsvRecord {
        ts_local:            now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        ts_exchange:         tick_ts.to_string(),
        event_type:          EventType::BinanceTick,
        latencia_ms:         now.timestamp_millis() - tick_ts,
        binance_price:       bb_mid,
        binance_micro_price: bb_mic,
        binance_imbalance:   bb_imb,
        binance_vol_100ms:   bb_vol_100,
        binance_vol_24h:     binance.btc_volume_24h,
        trades_per_second:   tracking.trades_per_second(now.timestamp_millis()),
        price_velocity:      tracking.price_velocity(now.timestamp_millis()),
        tape_speed_flag:     check_volume_spike(bb_vol_100, tracking, 0.0),
        ..Default::default()
    }
}

// ─── Advanced HFT Metric Computation ──────────────────────────────────────

/// Compute liquidity delta: difference in poly ask volume vs previous tick.
/// Returns (delta, spoofing_flag).
/// Spoofing detected when: volume drops >30% from previous AND no poly trade occurred.
pub fn compute_liquidity_delta(
    current_ask_vol: f64,
    tracking: &TrackingState,
    is_poly_trade: bool,
) -> (f64, u8) {
    let prev = tracking.get_last_poly_ask_vol();
    let delta = match prev {
        Some(p) if p > 0.0 => current_ask_vol - p,
        _ => 0.0,
    };
    // Detect spoofing: >30% volume drop with no Polymarket trade matching
    let spoofing = if let Some(p) = prev {
        if p > 0.0 && delta < 0.0 && (delta.abs() / p) > 0.30 && !is_poly_trade {
            1u8
        } else { 0u8 }
    } else { 0u8 };

    (delta, spoofing)
}

/// Compute price gap ratio: (binance_micro_price - poly_mid_price) / binance_micro_price * 100.
/// Positive = Polymarket is below Binance (lagging behind).
/// Returns (gap_pct, gap_alert_flag).
pub fn compute_price_gap(binance_micro: f64, poly_mid: f64) -> (f64, u8) {
    if binance_micro <= 0.0 || poly_mid <= 0.0 {
        return (0.0, 0u8);
    }
    let gap_pct = ((binance_micro - poly_mid) / binance_micro) * 100.0;
    let alert = if gap_pct.abs() > 0.05 { 1u8 } else { 0u8 };
    (gap_pct, alert)
}

/// Compute absorption ratio: poly_trade_volume / |Δpoly_mid|.
/// High ratio = lots of volume traded but price barely moved (absorption).
/// Returns 0.0 when delta price is zero (infinite absorption).
pub fn compute_absorption_ratio(
    trade_vol: f64,
    poly_mid: f64,
    previous_mid: Option<f64>,
) -> f64 {
    let prev = match previous_mid {
        Some(p) if p > 0.0 => p,
        _ => return 0.0,
    };
    let dp = (poly_mid - prev).abs();
    if dp < f64::EPSILON {
        return if trade_vol > 0.0 { trade_vol / 0.0001 } else { 0.0 };
    }
    trade_vol / dp
}

/// Check for high volume spike and emit console alert.
/// Returns tape_speed_flag (1 if spike detected).
pub fn check_volume_spike(
    vol_100ms: f64,
    tracking: &TrackingState,
    poly_spread: f64,
) -> u8 {
    let stats = tracking.vol_stats.lock().unwrap();
    let is_spike = stats.count >= 10 && vol_100ms > stats.mean + 3.0 * stats.std_dev();
    drop(stats);
    if is_spike {
        info!("\x1b[36m[INSIGHT] High Volume Spike en Binance: {:.2} BTC/100ms | Poly Spread: {:.6}\x1b[0m", vol_100ms, poly_spread);
        1u8
    } else {
        0u8
    }
}

/// Emit yellow console alert when price gap ratio exceeds threshold.
pub fn check_gap_alert(gap_pct: f64) {
    if gap_pct.abs() > 0.05 {
        info!("\x1b[33m[INSIGHT] Gap detectado - Polymarket con retraso: {:.4}%\x1b[0m", gap_pct);
    }
}
