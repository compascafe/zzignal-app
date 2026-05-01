//! Adaptive Risk Engine with Conformal Prediction & Macro 24h Warm-up.
//!
//! Pipeline:
//!   1. Macro warm‑up: fetch 1,440 candles (1m) from Binance REST, compute
//!      SMA200‑slope, MACD(3,10,16), VFI, RSI(14).
//!   2. Conformal Prediction: residuals‑based normalcy range, 95 % quantile.
//!   3. Master signal: macro + VFI + CP + spread confluence.
//!   4. Feedback loop: Robbins‑Monro α update on session close.

use std::collections::VecDeque;

use serde::Deserialize;
use tracing::info;

// ─── Binance Kline (REST) ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BinanceKline(
    i64,    // open time
    String, // open
    String, // high
    String, // low
    String, // close
    String, // volume
    i64,    // close time
    String, // quote asset volume
    u64,    // number of trades
    String, // taker buy base volume
    String, // taker buy quote volume
    String, // ignore
);

#[derive(Debug, Clone)]
pub struct Candle1m {
    pub ts:     i64, // open time ms
    pub open:   f64,
    pub high:   f64,
    pub low:    f64,
    pub close:  f64,
    pub volume: f64,
}

// ─── Macro Indicators ─────────────────────────────────────────────────────────

/// Thread-safe context for sharing dynamic signals between threads.
/// Updated each minute by a background task, read by the tick consumer.
#[derive(Debug, Clone)]
pub struct MacroContext {
    pub dynamic_rsi:       f64,   // Rolling RSI updated each minute
    pub vfi_confidence:    f64,   // VFI volume strength ratio (0-1 normalized)
    pub db_accuracy_factor: f64,  // Risk multiplier from historical memory (1.0=neutral)
    pub weighted_bias:     String,// VFI-weighted predicted bias (UP/DOWN/NEUTRAL)
    pub last_rsi:          f64,   // Previous RSI value (for momentum cross detection)
    pub momentum_flipped:  bool,  // True if RSI crossed above 30 this session
}

impl Default for MacroContext {
    fn default() -> Self {
        Self {
            dynamic_rsi:        50.0,
            vfi_confidence:     0.5,
            db_accuracy_factor: 1.0,
            weighted_bias:      String::new(),
            last_rsi:           50.0,
            momentum_flipped:   false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MacroSnapshot {
    pub sma50:          f64,
    pub sma200:         f64,
    pub macro_slope:    f64,  // SMA200 slope (linear regression on last 50 points)
    pub macd_line:      f64,  // MACD(3,10,16) line
    pub macd_signal:    f64,
    pub macd_hist:      f64,
    pub vfi:            f64,  // Volume Flow Indicator
    pub rsi14:          f64,
    pub predicted_bias: String, // "UP" or "DOWN"
    pub cp_quantile:    f64,  // 95% empirical quantile of residuals
    pub cp_alpha:       f64,  // Robbins-Monro learning rate
    pub close_prices:   Vec<f64>, // last 200 close prices (for Bollinger+CP at runtime)
}

/// Simple moving average.
fn sma(data: &[f64], window: usize) -> f64 {
    let n = data.len().min(window);
    if n == 0 { return 0.0; }
    data[data.len() - n..].iter().sum::<f64>() / n as f64
}

/// Linear regression slope on the last `n` points of y (y = a + b*x, returns b).
fn slope(y: &[f64], n: usize) -> f64 {
    let len = y.len();
    if len < n || n < 2 { return 0.0; }
    let slice = &y[len - n..];
    let nf = n as f64;
    let sum_x: f64 = (0..n).map(|i| i as f64).sum();
    let sum_y: f64 = slice.iter().sum();
    let sum_xy: f64 = slice.iter().enumerate().map(|(i, &v)| i as f64 * v).sum();
    let sum_x2: f64 = (0..n).map(|i| (i * i) as f64).sum();
    let denom = nf * sum_x2 - sum_x * sum_x;
    if denom.abs() < 1e-12 { return 0.0; }
    (nf * sum_xy - sum_x * sum_y) / denom
}

/// Exponential moving average.
fn ema(data: &[f64], window: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(data.len());
    let alpha = 2.0 / (window as f64 + 1.0);
    for (i, &val) in data.iter().enumerate() {
        if i == 0 {
            out.push(val);
        } else {
            out.push(alpha * val + (1.0 - alpha) * out[i - 1]);
        }
    }
    out
}

/// MACD(3, 10, 16) — fast EMA, slow EMA, signal EMA, histogram.
fn macd_3_10_16(closes: &[f64]) -> (f64, f64, f64) {
    if closes.len() < 16 { return (0.0, 0.0, 0.0); }
    let fast  = ema(closes, 3);
    let slow  = ema(closes, 10);
    let macd_line: Vec<f64> = fast.iter().zip(slow.iter()).map(|(f, s)| f - s).collect();
    let signal = ema(&macd_line, 16);
    let line  = *macd_line.last().unwrap_or(&0.0);
    let sig   = *signal.last().unwrap_or(&0.0);
    (line, sig, line - sig)
}

/// RSI(14) — standard Wilder's RSI.
fn rsi14(closes: &[f64]) -> f64 {
    if closes.len() < 15 { return 50.0; }
    let mut gains = 0.0f64;
    let mut losses = 0.0f64;
    let changes: Vec<f64> = closes.windows(2).map(|w| w[1] - w[0]).collect();
    // initial average
    for &c in changes.iter().take(14) {
        if c > 0.0 { gains += c; } else { losses += -c; }
    }
    let mut avg_gain = gains / 14.0;
    let mut avg_loss = losses / 14.0;
    for &c in changes.iter().skip(14) {
        let g = if c > 0.0 { c } else { 0.0 };
        let l = if c < 0.0 { -c } else { 0.0 };
        avg_gain = (avg_gain * 13.0 + g) / 14.0;
        avg_loss = (avg_loss * 13.0 + l) / 14.0;
    }
    if avg_loss == 0.0 { return 100.0; }
    let rs = avg_gain / avg_loss;
    100.0 - 100.0 / (1.0 + rs)
}

/// Volume Flow Indicator (VFI) — simplified: cumulative (close-typical)*volume / MA(volume,N)
fn vfi(candles: &[Candle1m], period: usize, coef: f64) -> f64 {
    if candles.len() < period { return 0.0; }
    let recent = &candles[candles.len() - period..];
    let vol_ma: f64 = recent.iter().map(|c| c.volume).sum::<f64>() / period as f64;
    if vol_ma == 0.0 { return 0.0; }
    let typical_prev: Vec<f64> = recent.windows(2).map(|w| (w[0].high + w[0].low + w[0].close) / 3.0).collect();
    let typical_curr: Vec<f64> = recent[1..].iter().map(|c| (c.high + c.low + c.close) / 3.0).collect();
    let vf: f64 = typical_curr.iter().zip(typical_prev.iter()).zip(recent[1..].iter())
        .map(|((tc, tp), c)| {
            let cutoff = coef * vol_ma;
            let change = tc - tp;
            let vol = if c.volume < cutoff { c.volume } else { cutoff };
            change * vol
        })
        .sum();
    vf / vol_ma
}

// ─── Conformal Prediction Framework ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ConformalPredictor {
    /// Residuals for each indicator (stored as VecDeque, trimmed to 1440 max).
    pub residuals_macd:  VecDeque<f64>,
    pub residuals_rsi:   VecDeque<f64>,
    pub residuals_price: VecDeque<f64>,  // |close - SMA200| / SMA200
    /// Current 95% quantile thresholds.
    pub quantile_macd:   f64,
    pub quantile_rsi:    f64,
    pub quantile_price:  f64,
    /// Robbins-Monro learning rate.
    pub alpha:           f64,
    /// Number of feedback updates applied.
    pub feedback_count:  u64,
    /// Recent accuracy window (last N sessions).
    pub accuracy_window: VecDeque<bool>,
    /// Whether intervals have been auto-widened due to low accuracy.
    pub auto_widened:    bool,
}

impl ConformalPredictor {
    pub fn new() -> Self {
        Self {
            residuals_macd:  VecDeque::with_capacity(1440),
            residuals_rsi:   VecDeque::with_capacity(1440),
            residuals_price: VecDeque::with_capacity(1440),
            quantile_macd:   0.05,
            quantile_rsi:    0.05,
            quantile_price:  0.005, // 0.5% of price
            alpha:           0.1,
            feedback_count:  0,
            accuracy_window: VecDeque::with_capacity(16),
            auto_widened:    false,
        }
    }

    /// Feed a batch of residuals to calibrate quantiles.
    pub fn calibrate(&mut self, macd_res: &[f64], rsi_res: &[f64], price_res: &[f64]) {
        for &r in macd_res { self.residuals_macd.push_back(r); }
        for &r in rsi_res  { self.residuals_rsi.push_back(r); }
        for &r in price_res { self.residuals_price.push_back(r); }
        // Trim to 1440 max
        while self.residuals_macd.len() > 1440 { self.residuals_macd.pop_front(); }
        while self.residuals_rsi.len() > 1440 { self.residuals_rsi.pop_front(); }
        while self.residuals_price.len() > 1440 { self.residuals_price.pop_front(); }
        self.update_quantiles();
    }

    /// Recompute empirical 95% quantiles from stored residuals.
    fn update_quantiles(&mut self) {
        self.quantile_macd  = empirical_quantile_95(&self.residuals_macd);
        self.quantile_rsi   = empirical_quantile_95(&self.residuals_rsi);
        self.quantile_price = empirical_quantile_95(&self.residuals_price);
        if self.quantile_macd < 0.001 { self.quantile_macd = 0.001; }
        if self.quantile_rsi < 0.5 { self.quantile_rsi = 0.5; }
        if self.quantile_price < 0.0001 { self.quantile_price = 0.0001; }
    }

    /// Check if a MACD value is "abnormal" (p-value < 0.05) = signal is valid.
    pub fn is_macd_valid(&self, macd_val: f64) -> bool {
        macd_val.abs() > self.quantile_macd
    }

    /// Check if an RSI value is in an extreme regime (valid signal).
    pub fn is_rsi_extreme(&self, rsi_val: f64) -> bool {
        let dev = (rsi_val - 50.0).abs();
        dev > self.quantile_rsi
    }

    /// Compute CP uncertainty range for price (width of 95% CI in USD).
    /// Based on Pasche et al. (2026) — extreme event detection.
    pub fn uncertainty_range(&self, btc_price: f64) -> f64 {
        self.quantile_price * btc_price
    }

    /// Validate a price move against the CP interval.
    /// Returns true if the move is outside the normal range (extreme event).
    pub fn is_price_extreme(&self, price: f64, sma200: f64) -> bool {
        if sma200 == 0.0 { return false; }
        let dev = (price - sma200).abs() / sma200;
        dev > self.quantile_price
    }

    /// Robbins-Monro stochastic approximation update (Aich et al., 2025).
    /// alpha_n = alpha_0 / (1 + beta * n)
    /// Called when accuracy_success is false.
    pub fn robbins_monro_update(&mut self, beta: f64) {
        self.feedback_count += 1;
        let n = self.feedback_count as f64;
        self.alpha = 0.1 / (1.0 + beta * n);
        // Widening: reduce quantile demands (higher quantile = wider acceptance)
        self.quantile_macd  *= 1.0 + self.alpha;
        self.quantile_rsi   *= 1.0 + self.alpha;
        self.quantile_price *= 1.0 + self.alpha;
        info!("CP Robbins‑Monro update: α={:.6} n={}", self.alpha, self.feedback_count);
    }

    /// Record an accuracy result (true = correct prediction, false = wrong).
    pub fn record_accuracy(&mut self, correct: bool) {
        self.accuracy_window.push_back(correct);
        if self.accuracy_window.len() > 16 {
            self.accuracy_window.pop_front();
        }
    }

    /// Check if accuracy over last 5 sessions is below 60%.
    /// If so, widen CP intervals to be more selective.
    pub fn should_auto_widen(&self) -> bool {
        let recent: Vec<bool> = self.accuracy_window.iter().rev().take(5).copied().collect();
        if recent.len() < 5 { return false; }
        let hits = recent.iter().filter(|&&b| b).count();
        (hits as f64 / recent.len() as f64) < 0.6
    }

    /// Auto-widen CP intervals by 20%.
    pub fn auto_widen(&mut self) {
        self.quantile_macd  *= 1.2;
        self.quantile_rsi   *= 1.2;
        self.quantile_price *= 1.2;
        self.auto_widened = true;
        info!("CP auto-widened: macd_q={:.4} rsi_q={:.2} price_q={:.6}",
            self.quantile_macd, self.quantile_rsi, self.quantile_price);
    }
}

/// Compute the 95th empirical quantile (absolute values).
fn empirical_quantile_95(data: &VecDeque<f64>) -> f64 {
    if data.is_empty() { return 0.0; }
    let mut abs_vals: Vec<f64> = data.iter().map(|v| v.abs()).collect();
    abs_vals.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (0.95 * (abs_vals.len() - 1) as f64) as usize;
    abs_vals.get(idx).copied().unwrap_or(abs_vals[abs_vals.len() - 1])
}

// ─── Adaptive Risk Engine (orchestrator) ──────────────────────────────────────

/// Pre‑computed warmup data — allows HTTP fetch outside the lock.
pub struct WarmupResult {
    pub candles:       Vec<Candle1m>,
    pub snapshot:      MacroSnapshot,
    pub cp:            ConformalPredictor,
    pub sma200_series: Vec<f64>,
    pub rsi_series:    Vec<f64>,
}

/// Fetch Binance candles + compute all indicators WITHOUT holding any engine lock.
/// This is the expensive part (2–5s HTTP); the engine lock is only needed for `apply_warmup_result`.
pub async fn warmup_fetch_and_compute() -> Result<WarmupResult, String> {
    let candles = fetch_binance_klines("BTCUSDT", "1m", 1440).await?;
    if candles.len() < 200 {
        return Err(format!("Not enough candles: {} < 200", candles.len()));
    }
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();

    let mut snapshot = MacroSnapshot::default();
    let n = closes.len();

    // SMA
    snapshot.sma50  = sma(&closes, 50);
    snapshot.sma200 = sma(&closes, 200);

    // SMA200 series + slope
    let sma200_series: Vec<f64> = (0..=n.saturating_sub(200))
        .map(|i| sma(&closes[0..=i], 200))
        .collect();
    snapshot.macro_slope = slope(&sma200_series, 50.min(sma200_series.len()));

    // MACD(3,10,16)
    let (ml, ms, mh) = macd_3_10_16(&closes);
    snapshot.macd_line   = ml;
    snapshot.macd_signal = ms;
    snapshot.macd_hist   = mh;

    // VFI
    snapshot.vfi = vfi(&candles, 130, 0.2);

    // RSI(14)
    let rsi_series = compute_rsi_series(&closes, 14);
    snapshot.rsi14 = *rsi_series.last().unwrap_or(&50.0);

    // Predicted bias
    snapshot.predicted_bias = if snapshot.macro_slope > 0.0 { "UP".into() } else { "DOWN".into() };

    // Store last 200 close prices for runtime Bollinger+CP
    snapshot.close_prices = closes[closes.len().saturating_sub(200)..].to_vec();

    // Calibrate CP residuals from 24h history
    let mut cp = ConformalPredictor::new();
    if n >= 200 {
        let macd_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let (ml, _, _) = macd_3_10_16(slice);
                let prev = &closes[0..=i - 1];
                let (pl, _, _) = macd_3_10_16(prev);
                (ml - pl).abs()
            })
            .collect();
        let rsi_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let rsi = rsi14(slice);
                let prev = &closes[0..=i - 1];
                let prsi = rsi14(prev);
                (rsi - prsi).abs()
            })
            .collect();
        let price_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let sma200 = sma(slice, 200);
                if sma200 == 0.0 { return 0.0; }
                (slice.last().unwrap() - sma200).abs() / sma200
            })
            .collect();
        cp.calibrate(&macd_residuals, &rsi_residuals, &price_residuals);
    }
    snapshot.cp_quantile = cp.quantile_price;
    snapshot.cp_alpha    = cp.alpha;

    Ok(WarmupResult { candles, snapshot: snapshot.clone(), cp, sma200_series, rsi_series })
}

pub struct AdaptiveRiskEngine {
    pub macro_snap:    MacroSnapshot,
    pub cp:            ConformalPredictor,
    candles:           Vec<Candle1m>,
    /// SMA200 series (used at runtime for Bollinger+CP).
    sma200_series:     Vec<f64>,
    /// RSI series.
    rsi_series:        Vec<f64>,
}

impl AdaptiveRiskEngine {
    pub fn new() -> Self {
        Self {
            macro_snap:    MacroSnapshot::default(),
            cp:            ConformalPredictor::new(),
            candles:       Vec::new(),
            sma200_series: Vec::new(),
            rsi_series:    Vec::new(),
        }
    }

    /// Phase 1: Download 1,440 1‑min candles from Binance REST and compute all indicators.
    pub async fn warmup(&mut self) -> Result<(), String> {
        info!("AdaptiveRiskEngine: fetching 1,440 candles (1m) from Binance...");
        let result = warmup_fetch_and_compute().await?;
        self.apply_warmup_result(result);
        info!("AdaptiveRiskEngine: warm‑up complete. slope={:.6} bias={} rsi={:.2} vfi={:.4}",
            self.macro_snap.macro_slope, self.macro_snap.predicted_bias,
            self.macro_snap.rsi14, self.macro_snap.vfi);
        Ok(())
    }

    /// Apply pre-computed warmup results — fast, lock‑friendly (no I/O).
    pub fn apply_warmup_result(&mut self, result: WarmupResult) {
        self.candles = result.candles;
        self.macro_snap = result.snapshot;
        self.sma200_series = result.sma200_series;
        self.rsi_series = result.rsi_series;
        self.cp = result.cp;
    }

    /// Compute all macro indicators from close price series.
    fn compute_indicators(&mut self, closes: &[f64]) {
        let n = closes.len();

        // SMA 50 & 200
        self.macro_snap.sma50  = sma(closes, 50);
        self.macro_snap.sma200 = sma(closes, 200);

        // SMA200 slope via linear regression on last 50 SMA200 points
        self.sma200_series = (0..=n.saturating_sub(200))
            .map(|i| sma(&closes[0..=i], 200))
            .collect();
        self.macro_snap.macro_slope = slope(&self.sma200_series, 50.min(self.sma200_series.len()));

        // MACD(3,10,16)
        let (ml, ms, mh) = macd_3_10_16(closes);
        self.macro_snap.macd_line   = ml;
        self.macro_snap.macd_signal = ms;
        self.macro_snap.macd_hist   = mh;

        // VFI
        self.macro_snap.vfi = vfi(&self.candles, 130, 0.2);

        // RSI(14)
        self.rsi_series = compute_rsi_series(closes, 14);
        self.macro_snap.rsi14 = *self.rsi_series.last().unwrap_or(&50.0);

        // Predicted bias
        self.macro_snap.predicted_bias = if self.macro_snap.macro_slope > 0.0 {
            "UP".into()
        } else {
            "DOWN".into()
        };

        // Store last 200 close prices for runtime Bollinger+CP
        self.macro_snap.close_prices = closes[closes.len().saturating_sub(200)..].to_vec();
    }

    /// Calibrate CP residuals from the 24h history.
    fn calibrate_cp(&mut self, closes: &[f64]) {
        let n = closes.len();
        if n < 200 { return; }

        // Build MACD series for residuals
        let macd_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let (ml, _, _) = macd_3_10_16(slice);
                let prev = &closes[0..=i - 1];
                let (pl, _, _) = macd_3_10_16(prev);
                (ml - pl).abs()
            })
            .collect();

        // RSI residuals
        let rsi_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let rsi = rsi14(slice);
                let prev = &closes[0..=i - 1];
                let prsi = rsi14(prev);
                (rsi - prsi).abs()
            })
            .collect();

        // Price residuals: |close - SMA200| / SMA200
        let price_residuals: Vec<f64> = (200..n)
            .map(|i| {
                let slice = &closes[0..=i];
                let sma200 = sma(slice, 200);
                if sma200 == 0.0 { return 0.0; }
                (slice.last().unwrap() - sma200).abs() / sma200
            })
            .collect();

        self.cp.calibrate(&macd_residuals, &rsi_residuals, &price_residuals);
    }

    /// Evaluate the master signal for a given tick context.
    /// Returns (master_signal, cp_uncertainty_range, cp_valid_signal).
    /// master_signal: 0=none, 1=Buy, 2=Sell
    pub fn evaluate_master_signal(
        &self,
        binance_price:    f64,
        _poly_mid:        f64,
        poly_spread:      f64,
        bollinger_sma:    f64,
        bollinger_upper:  f64,
        bollinger_lower:  f64,
        poly_imbalance:   f64,
        price_velocity:   f64,
        is_feedback_adj:  bool,
    ) -> (u8, f64, u8) {
        let cp_range = self.cp.uncertainty_range(binance_price);

        // ─── Step 1: CP validation on Bollinger extremes (Pasche et al., 2026) ──
        let bb_extreme = price_below_bb(binance_price, bollinger_lower)
            || price_above_bb(binance_price, bollinger_upper);
        let cp_extreme = self.cp.is_price_extreme(binance_price, bollinger_sma);
        // Signal is only valid if BOTH BB AND CP flag an extreme event
        let cp_valid: u8 = if bb_extreme && cp_extreme { 1 } else { 0 };

        // ─── Step 2: Macro confluence ───────────────────────────────────────────
        let macro_up   = self.macro_snap.macro_slope > 0.0;
        let macro_down = self.macro_snap.macro_slope < 0.0;

        // ─── Step 3: VFI confirmation ───────────────────────────────────────────
        let vfi_up   = self.macro_snap.vfi > 0.0;
        let vfi_down = self.macro_snap.vfi < 0.0;

        // ─── Step 4: Spread check ───────────────────────────────────────────────
        let spread_ok = poly_spread <= 0.05;

        // ─── Step 5: Confluence ─────────────────────────────────────────────────
        let mut master: u8 = 0;

        let buy_conditions  = macro_up && vfi_up && spread_ok
            && poly_imbalance > 1.02 && price_velocity > 0.0;
        let sell_conditions = macro_down && vfi_down && spread_ok
            && poly_imbalance < 0.98 && price_velocity < 0.0;

        if buy_conditions && cp_valid == 1 {
            master = 1; // Buy (mean reversion: price below lower BB → expect bounce)
        } else if sell_conditions && cp_valid == 1 {
            master = 2; // Sell (price above upper BB → expect drop)
        }

        let feedback_bit: u8 = if is_feedback_adj { 1 } else { 0 };
        (master, cp_range, cp_valid | feedback_bit)
    }

    /// Compute VFI-weighted bias (60% VFI + 40% SMA slope).
    /// If VFI and SMA slope diverge beyond threshold → NEUTRAL bias.
    pub fn compute_weighted_bias(&mut self, ctx: &mut MacroContext) {
        let vfi = self.macro_snap.vfi;
        let slope = self.macro_snap.macro_slope;
        let vfi_dir = if vfi > 0.1 { 1.0 } else if vfi < -0.1 { -1.0 } else { 0.0 };
        let sma_dir = if slope > 0.0001 { 1.0 } else if slope < -0.0001 { -1.0 } else { 0.0 };

        // Normalize VFI to 0-1 confidence
        ctx.vfi_confidence = (vfi.abs() / 10.0).min(1.0);

        // Divergence detection: SMA down but VFI strongly up → smart money conflict
        let divergence = (vfi_dir * sma_dir) < 0.0 && vfi.abs() > 0.5 && slope.abs() > 0.0001;

        let weighted_score = if divergence {
            // SMA and VFI disagree → NEUTRAL (don't fight smart money)
            0.0
        } else {
            // Weighted score: 60% VFI + 40% SMA
            0.6 * vfi_dir + 0.4 * sma_dir
        };

        ctx.weighted_bias = if weighted_score > 0.15 {
            "UP".into()
        } else if weighted_score < -0.15 {
            "DOWN".into()
        } else {
            "NEUTRAL".into()
        };

        // Also update the macro snapshot's predicted_bias for CSV
        if ctx.weighted_bias != "NEUTRAL" {
            self.macro_snap.predicted_bias = ctx.weighted_bias.clone();
        }
    }

    /// Pre-trade calibration: check last N historical sessions.
    /// If accuracy < 65%, multiply CP quantile by db_accuracy_factor (penalty).
    pub fn pre_trade_calibrate(&mut self, ctx: &mut MacroContext, recent_accuracy: &[bool]) {
        let n = recent_accuracy.len();
        if n == 0 {
            ctx.db_accuracy_factor = 1.0;
            return;
        }
        let hits = recent_accuracy.iter().filter(|&&b| b).count();
        let accuracy = hits as f64 / n as f64;

        if accuracy < 0.65 {
            // Penalty: widen CP by up to 2x depending on how bad accuracy is
            let penalty = 1.0 + (0.65 - accuracy) * 3.0; // 0% acc → 2.95x, 40% acc → 1.75x
            ctx.db_accuracy_factor = penalty;
            self.cp.quantile_macd  *= penalty;
            self.cp.quantile_rsi   *= penalty;
            self.cp.quantile_price *= penalty;
            info!("Pre-trade calibration: accuracy={:.0}% < 65% → CP widened by {:.2}x", accuracy * 100.0, penalty);
        } else {
            ctx.db_accuracy_factor = 1.0;
        }
    }

    /// Update dynamic rolling RSI with a new price tick.
    /// Called each time a new Binance price arrives during a session.
    /// Returns the current dynamic RSI value.
    pub fn update_dynamic_rsi(&mut self, ctx: &mut MacroContext, price: f64) -> f64 {
        // Store price in a rolling buffer (keep last 15 prices for RSI-14)
        self.macro_snap.close_prices.push(price);
        if self.macro_snap.close_prices.len() > 200 {
            self.macro_snap.close_prices.remove(0);
        }
        // Recompute RSI if we have enough data
        if self.macro_snap.close_prices.len() >= 15 {
            ctx.last_rsi = ctx.dynamic_rsi;
            ctx.dynamic_rsi = rsi14(&self.macro_snap.close_prices);
        }
        ctx.dynamic_rsi
    }

    /// Momentum trigger: if RSI crosses above 30 from below → flip DOWN bias to UP.
    /// Returns true if bias was flipped.
    pub fn check_momentum_trigger(&mut self, ctx: &mut MacroContext) -> bool {
        if ctx.last_rsi <= 30.0 && ctx.dynamic_rsi > 30.0 && !ctx.momentum_flipped {
            ctx.momentum_flipped = true;
            if ctx.weighted_bias == "DOWN" || self.macro_snap.predicted_bias == "DOWN" {
                ctx.weighted_bias = "UP".into();
                self.macro_snap.predicted_bias = "UP".into();
                info!("Momentum trigger: RSI crossed above 30 ({}→{}) → bias flipped to UP",
                    ctx.last_rsi, ctx.dynamic_rsi);
                return true;
            }
        }
        false
    }

    /// Accessors for CSV column population.
    pub fn macro_slope(&self)    -> f64    { self.macro_snap.macro_slope }
    pub fn vfi_value(&self)      -> f64    { self.macro_snap.vfi }
    pub fn macd_hist(&self)      -> f64    { self.macro_snap.macd_hist }
    pub fn predicted_bias(&self) -> &str   { &self.macro_snap.predicted_bias }
    pub fn rsi_value(&self)      -> f64    { self.macro_snap.rsi14 }
    pub fn cp_alpha(&self)       -> f64    { self.cp.alpha }
    pub fn cp_quantile(&self)    -> f64    { self.cp.quantile_price }
    pub fn is_auto_widened(&self)-> bool   { self.cp.auto_widened }
    pub fn cp_mut(&mut self)     -> &mut ConformalPredictor { &mut self.cp }

    /// Get SMA200 slope value for CSV.
    pub fn get_slope(&self) -> f64 { self.macro_snap.macro_slope }

    /// Get predicted bias.
    pub fn get_bias(&self) -> &str { &self.macro_snap.predicted_bias }

    /// CP uncertainty range in USD for the given price.
    pub fn cp_range(&self, price: f64) -> f64 { self.cp.uncertainty_range(price) }

    /// Check if feedback has auto-widened.
    pub fn feedback_adjusted(&self) -> u8 {
        if self.cp.auto_widened { 1 } else { 0 }
    }
}

// ─── Binance REST API ─────────────────────────────────────────────────────────

fn price_below_bb(price: f64, lower: f64) -> bool { price <= lower }
fn price_above_bb(price: f64, upper: f64) -> bool { price >= upper }

/// Full RSI series (not just last value).
fn compute_rsi_series(closes: &[f64], period: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(closes.len());
    for i in period..=closes.len() {
        out.push(rsi14(&closes[0..i]));
    }
    out
}

/// Fetch klines from Binance REST API.
async fn fetch_binance_klines(symbol: &str, interval: &str, limit: u32) -> Result<Vec<Candle1m>, String> {
    let url = format!(
        "https://api.binance.com/api/v3/klines?symbol={}&interval={}&limit={}",
        symbol, interval, limit
    );
    let client = reqwest::Client::new();
    let resp = client.get(&url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Binance klines HTTP: {e}"))?;

    let body = resp.text().await.map_err(|e| format!("Binance klines body: {e}"))?;

    let klines: Vec<BinanceKline> = serde_json::from_str(&body)
        .map_err(|e| format!("Binance klines JSON: {e} — body preview: {}", &body[..body.len().min(200)]))?;

    Ok(klines.iter().map(|k| {
        Candle1m {
            ts:     k.0,
            open:   k.1.parse().unwrap_or(0.0),
            high:   k.2.parse().unwrap_or(0.0),
            low:    k.3.parse().unwrap_or(0.0),
            close:  k.4.parse().unwrap_or(0.0),
            volume: k.5.parse().unwrap_or(0.0),
        }
    }).collect())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sma() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((sma(&data, 3) - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_slope() {
        let data: Vec<f64> = (0..10).map(|i| i as f64 * 2.0).collect();
        assert!((slope(&data, 10) - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_rsi14() {
        let mut closes = vec![100.0; 20];
        for i in 10..20 { closes[i] = 101.0; }
        let rsi = rsi14(&closes);
        assert!(rsi > 70.0, "RSI should be high: {rsi}");
    }

    #[test]
    fn test_empirical_quantile() {
        let mut d = VecDeque::from(vec![-1.0, 2.0, 3.0, -4.0, 5.0]);
        let q = empirical_quantile_95(&d);
        assert!(q > 0.0);
    }

    #[test]
    fn test_macd() {
        let closes: Vec<f64> = (0..30).map(|i| 100.0 + (i as f64).sin() * 2.0).collect();
        let (l, s, h) = macd_3_10_16(&closes);
        assert!(l.is_finite() && s.is_finite());
    }
}
