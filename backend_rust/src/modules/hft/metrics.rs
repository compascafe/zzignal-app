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

    // ─── Session baselines for normalized price_gap_ratio ─────────────────
    /// BTC price at session start (set on first poly event of session).
    pub session_btc_start:     Mutex<Option<f64>>,
    /// Poly mid price at session start (set on first poly event of session).
    pub session_poly_mid_start: Mutex<Option<f64>>,

    // ─── Bollinger Bands: rolling window of last 200 Binance mid prices ───
    pub bollinger_window:      Mutex<VecDeque<f64>>,
    /// Running stats for bollinger window (Welford's).
    pub(super) bollinger_stats: Mutex<RunningStats>,

    // ─── Average trades/sec for vol_burst detection ───────────────────────
    pub(super) trades_ps_stats: Mutex<RunningStats>,

    // ─── Session-level volatility average (for high_volatility_event) ─────
    pub(super) session_vol_stats: Mutex<RunningStats>,

    // ─── Conformal Prediction: calibration set (N=1000) ───────────────────
    /// Absolute residuals |binance_price - bollinger_sma| for non-conformity scores.
    pub calibration_errors: Mutex<VecDeque<f64>>,
}

impl TrackingState {
    pub fn new() -> Self {
        Self {
            vol_window:          Mutex::new(Vec::with_capacity(64)),
            last_big_move:       Mutex::new(None),
            last_binance_price:  Mutex::new(None),
            vol_stats:           Mutex::new(RunningStats::new()),
            trade_ts_window:     Mutex::new(VecDeque::with_capacity(256)),
            price_history:       Mutex::new(VecDeque::with_capacity(128)),
            last_poly_ask_vol:   Mutex::new(None),
            last_poly_mid:       Mutex::new(None),
            last_poly_mid_ts:    Mutex::new(None),
            session_btc_start:      Mutex::new(None),
            session_poly_mid_start: Mutex::new(None),
            bollinger_window:    Mutex::new(VecDeque::with_capacity(256)),
            bollinger_stats:     Mutex::new(RunningStats::new()),
            trades_ps_stats:     Mutex::new(RunningStats::new()),
            session_vol_stats:   Mutex::new(RunningStats::new()),
            calibration_errors:  Mutex::new(VecDeque::with_capacity(1024)),
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

    /// Compute price velocity: slope of Binance mid price over 1s window (USD/s).
    /// Uses the RingBuffer for direct look-back — no separate price history needed.
    pub fn price_velocity(&self, ring: &PriceRingBuffer, now_ms: i64, current_price: f64) -> f64 {
        let target_ts = (now_ms as u64).saturating_sub(1000);
        if let Some(past) = ring.get_closest_to(target_ts) {
            if past.timestamp > 0 && past.mid_price > 0.0 {
                let dt_ms = ((now_ms as u64).saturating_sub(past.timestamp)).max(1) as f64;
                let dp = current_price - past.mid_price;
                return dp / (dt_ms / 1000.0); // USD per second
            }
        }
        0.0
    }

    /// Lazy-init session baselines for normalized price_gap_ratio.
    /// Called on the first poly event of a new recording session.
    pub fn init_session_baselines(&self, btc_price: f64, poly_mid: f64) {
        let mut btc = self.session_btc_start.lock().unwrap();
        let mut poly = self.session_poly_mid_start.lock().unwrap();
        if btc.is_none() {
            *btc = Some(btc_price);
            *poly = Some(poly_mid);
            info!("[SESSION BASELINE] BTC start: {:.2} | Poly mid start: {:.6}", btc_price, poly_mid);
        }
    }

    /// Reset session baselines (called when all sessions stop).
    pub fn reset_session_baselines(&self) {
        *self.session_btc_start.lock().unwrap() = None;
        *self.session_poly_mid_start.lock().unwrap() = None;
    }

    // ─── Bollinger Bands (200-tick rolling window) ────────────────────────

    /// Push a Binance mid price into the bollinger rolling window (max 200).
    pub fn push_bollinger_price(&self, price: f64) {
        if price <= 0.0 { return; }
        let mut w = self.bollinger_window.lock().unwrap();
        w.push_back(price);
        if w.len() > 200 { w.pop_front(); }
        // Recompute running stats — Welford's is incremental, so just push
        self.bollinger_stats.lock().unwrap().push(price);
    }

    /// Compute Bollinger Bands + realized volatility + position.
    /// Returns (sma, upper, lower, std_dev, realized_volatility, position).
    /// position: 1=above upper, -1=below lower, 0=inside.
    pub fn bollinger_bands_full(&self, price: f64) -> (f64, f64, f64, f64, f64, i8) {
        let stats = self.bollinger_stats.lock().unwrap();
        if stats.count < 20 {
            return (0.0, 0.0, 0.0, 0.0, 0.0, 0);
        }
        let sma = stats.mean;
        let std = stats.std_dev();
        let upper = sma + 2.0 * std;
        let lower = sma - 2.0 * std;
        let position = if price > upper { 1i8 } else if price < lower { -1i8 } else { 0i8 };
        (sma, upper, lower, std, std, position)
    }

    /// Track session-level volatility average for high_volatility_event detection.
    pub fn push_session_volatility(&self, vol: f64) {
        self.session_vol_stats.lock().unwrap().push(vol);
    }

    /// High volatility event: current vol > 2x session average (warmup: 5 samples).
    pub fn is_high_volatility(&self, current_vol: f64) -> u8 {
        let stats = self.session_vol_stats.lock().unwrap();
        if stats.count < 5 { return 0; }
        if current_vol > stats.mean * 2.0 { 1u8 } else { 0u8 }
    }

    /// Master signal: combines bollinger position + extreme imbalance + velocity direction.
    /// Returns (signal: 0/1/2, label).
    /// 1 = Buy:  touching lower band AND imbalance > 0.8 AND velocity > 0
    /// 2 = Sell: touching upper band AND imbalance < -0.8 AND velocity < 0
    pub fn master_signal(
        &self, price: f64, imbalance: f32, price_vel: f64,
    ) -> (u8, String) {
        let stats = self.bollinger_stats.lock().unwrap();
        if stats.count < 20 { return (0, String::new()); }
        let sma = stats.mean;
        let std = stats.std_dev();
        drop(stats);

        let upper = sma + 2.0 * std;
        let lower = sma - 2.0 * std;
        let pos: i8 = if price > upper { 1 } else if price < lower { -1 } else { 0 };

        if pos == -1 && imbalance > 0.8 && price_vel > 0.0 {
            (1, "MASTER_BUY".into())
        } else if pos == 1 && imbalance < -0.8 && price_vel < 0.0 {
            (2, "MASTER_SELL".into())
        } else {
            (0, String::new())
        }
    }

    /// Record trades_per_second for average tracking (vol_burst detection).
    pub fn push_tps_sample(&self, tps: f64) {
        self.trades_ps_stats.lock().unwrap().push(tps);
    }

    /// Average trades_per_second over recorded samples.
    pub fn avg_tps(&self) -> f64 {
        let stats = self.trades_ps_stats.lock().unwrap();
        if stats.count < 5 { return 0.0; }
        stats.mean
    }

    // ─── Signal computation ───────────────────────────────────────────────

    /// Compute mean reversion signal from Bollinger bands.
    /// Returns (signal: 0/1/2, label).
    /// 1 = Short: price touches upper band AND binance_imbalance < 0
    /// 2 = Long:  price touches lower band AND binance_imbalance > 0
    pub fn mean_reversion_signal(&self, price: f64, imbalance: f32) -> (u8, String) {
        let (sma, upper, lower, _, _, _) = self.bollinger_bands_full(price);
        if sma == 0.0 { return (0, String::new()); }

        if price >= upper && imbalance < -0.1 {
            return (1, "BOLLINGER_SHORT".into());
        }
        if price <= lower && imbalance > 0.1 {
            return (2, "BOLLINGER_LONG".into());
        }
        (0, String::new())
    }

    /// Compute technical confluence signal.
    /// Returns (signal: 0/1, label).
    /// All conditions must align:
    ///   1. Trend: price above SMA (trend_dir=1) or below SMA (trend_dir=-1)
    ///   2. Vol burst: current_tps > 2x avg_tps
    ///   3. Extreme imbalance: |imbalance| > 0.8
    ///   4. Momentum confirms: price_velocity direction matches trend
    pub fn technical_confluence_signal(
        &self, price: f64, imbalance: f32, current_tps: f64, price_vel: f64,
    ) -> (u8, String) {
        let (sma, _, _, _, _, _) = self.bollinger_bands_full(price);
        if sma == 0.0 || current_tps <= 0.0 { return (0, String::new()); }

        // 1. Trend position
        let trend_dir: i8 = if price > sma * 1.001 { 1 } else if price < sma * 0.999 { -1 } else { 0 };
        if trend_dir == 0 { return (0, String::new()); }

        // 2. Vol burst: tps > 2x average
        let avg = self.avg_tps();
        if avg <= 0.0 || current_tps <= avg * 2.0 { return (0, String::new()); }

        // 3. Extreme imbalance
        if imbalance.abs() <= 0.8 { return (0, String::new()); }

        // 4. Momentum confirms direction
        let vel_ok = (trend_dir > 0 && price_vel > 0.0) || (trend_dir < 0 && price_vel < 0.0);
        if !vel_ok { return (0, String::new()); }

        (1, "TECH_CONFLUENCE".into())
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

    // ─── Conformal Prediction (95% confidence, α=0.05) ────────────────────

    /// Push absolute residual |binance_price - bollinger_sma| into calibration set (N=1000).
    pub fn push_calibration_error(&self, error: f64) {
        let mut w = self.calibration_errors.lock().unwrap();
        w.push_back(error);
        if w.len() > 1000 { w.pop_front(); }
    }

    /// Compute (1-α) quantile from calibration errors.
    /// Returns (uncertainty_range_in_usd, cp_valid_signal).
    /// cp_valid_signal = 1 if current_deviation <= quantile (within confidence interval).
    pub fn conformal_validate(&self, current_deviation: f64, alpha: f64) -> (f64, u8) {
        let errors = self.calibration_errors.lock().unwrap();
        let n = errors.len();
        if n < 20 { return (0.0, 0u8); }

        let mut sorted: Vec<f64> = errors.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = (((n as f64 + 1.0) * (1.0 - alpha)).ceil() as usize).min(n) - 1;
        let quantile = sorted[idx];

        let valid = if current_deviation <= quantile { 1u8 } else { 0u8 };
        (quantile, valid)
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
    let price_vel    = tracking.price_velocity(ring, now.timestamp_millis(), bb_mid);
    let (liq_delta, spoof_flag) = compute_liquidity_delta(pb_ask_vol, tracking, false);
    let (gap_pct, gap_flag)     = compute_price_gap(bb_mid, pb_mid, tracking);
    let tape_flag    = check_volume_spike(bn_vol_100, tracking, pb_sprd);
    if gap_flag > 0 { check_gap_alert(gap_pct); }

    // ─── Bollinger Bands & Signals ────────────────────────────────────────
    let (bb_sma, bb_upper, bb_lower, bb_std, realized_vol, bb_pos) = tracking.bollinger_bands_full(bb_mid);
    tracking.push_session_volatility(bb_std);
    let high_vol = tracking.is_high_volatility(bb_std);

    // ─── Conformal Prediction: gate signals with risk validation ──────────
    let abs_err = (bb_mid - bb_sma).abs();
    tracking.push_calibration_error(abs_err);
    let (cp_range, cp_valid) = tracking.conformal_validate(abs_err, 0.05);

    let (mr_signal, mut signal_label) = tracking.mean_reversion_signal(bb_mid, bb_imb);
    tracking.push_tps_sample(trades_ps);
    let (tc_signal, tc_label) = tracking.technical_confluence_signal(bb_mid, bb_imb, trades_ps, price_vel);
    // Master signal only activates if CP validation passes
    let (master_sig, master_label) = if cp_valid > 0 {
        tracking.master_signal(bb_mid, bb_imb, price_vel)
    } else {
        (0u8, String::new())
    };
    if tc_signal > 0 {
        signal_label = if signal_label.is_empty() { tc_label } else { format!("{}|{}", signal_label, tc_label) };
    }
    if mr_signal > 0 && signal_label.is_empty() {
        signal_label = match mr_signal { 1 => "BOLLINGER_SHORT".into(), 2 => "BOLLINGER_LONG".into(), _ => String::new() };
    }
    if master_sig > 0 {
        signal_label = if signal_label.is_empty() { master_label } else { format!("{}|{}", signal_label, master_label) };
    }
    let trend_dir = if bb_sma > 0.0 {
        if bb_mid > bb_sma * 1.001 { 1i8 } else if bb_mid < bb_sma * 0.999 { -1i8 } else { 0i8 }
    } else { 0i8 };

    // Lazy-init session baselines on first poly event
    tracking.init_session_baselines(bb_mid, pb_mid);

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
        absorption_ratio:    0.0,
        price_gap_ratio:     gap_pct,
        spoofing_flag:       spoof_flag,
        tape_speed_flag:     tape_flag,
        gap_alert_flag:      gap_flag,
        bollinger_sma:       bb_sma,
        bollinger_upper:     bb_upper,
        bollinger_lower:     bb_lower,
        mean_reversion_signal: mr_signal,
        technical_confluence:  tc_signal.max(mr_signal.min(1)),
        trend_direction:     trend_dir,
        signal_label:        signal_label,
        realized_volatility: realized_vol,
        high_volatility_event: high_vol,
        bollinger_position:  bb_pos,
        master_signal:       master_sig,
        cp_uncertainty_range: cp_range,
        cp_valid_signal:     cp_valid,
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
    ring:          &PriceRingBuffer,
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
        price_velocity:      tracking.price_velocity(ring, now.timestamp_millis(), bb_mid),
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

/// Compute normalized price gap ratio: percentage divergence between Binance and Poly
/// since session start. Uses session-start baselines to normalize across different scales.
///
/// Formula: ((binance_price / btc_start) - (poly_mid / poly_mid_start)) * 100
///
/// Positive = Poly is leading (up more) or Binance is lagging.
/// Negative = Binance is leading (up more) or Poly is lagging.
/// Returns (gap_pct, gap_alert_flag).
pub fn compute_price_gap(
    binance_price: f64,
    poly_mid: f64,
    tracking: &TrackingState,
) -> (f64, u8) {
    // Use last known poly_mid if current is 0 or invalid
    let effective_poly = if poly_mid <= 0.0 {
        tracking.last_poly_mid.lock().unwrap().unwrap_or(0.0)
    } else {
        poly_mid
    };
    if effective_poly <= 0.0 {
        return (0.0, 0u8);
    }

    let btc_start = tracking.session_btc_start.lock().unwrap().unwrap_or(binance_price);
    let poly_start = tracking.session_poly_mid_start.lock().unwrap().unwrap_or(effective_poly);

    if btc_start <= 0.0 || poly_start <= 0.0 {
        return (0.0, 0u8);
    }

    // Use a minimum denominator of 1e-10 to avoid division-by-zero on flat markets
    let btc_pct_move  = (binance_price - btc_start) / btc_start.max(1e-10);
    let poly_pct_move = (effective_poly - poly_start) / poly_start.max(1e-10);
    let gap_pct = (poly_pct_move - btc_pct_move) * 100.0;

    let alert = if gap_pct.abs() > 0.05 { 1u8 } else { 0u8 };
    (gap_pct, alert)
}

/// Compute absorption ratio: poly_trade_volume / |Δpoly_mid|.
/// High ratio = lots of volume traded but price barely moved (absorption).
/// When delta price is zero, returns the trade volume directly (not inflated).
pub fn compute_absorption_ratio(
    trade_vol: f64,
    poly_mid: f64,
    previous_mid: Option<f64>,
) -> f64 {
    let prev = match previous_mid {
        Some(p) if p > 0.0 => p,
        _ => return trade_vol,
    };
    let dp = (poly_mid - prev).abs();
    if dp < f64::EPSILON {
        return trade_vol;
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
