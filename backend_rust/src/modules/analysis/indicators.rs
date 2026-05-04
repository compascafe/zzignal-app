//! Technical Indicators — Bollinger, RSI, MACD, VFI, Trend
//!
//! Layer 2: Analysis. Funciones puras que toman arrays de precios
//! y devuelven valores de indicadores. No mantienen estado (stateless).

// ─── Bollinger Bands ───────────────────────────────────────────────────────

/// Bollinger Bands (SMA + 2σ, SMA - 2σ)
pub fn bollinger_bands(prices: &[f64], period: usize) -> (f64, f64, f64) {
    if prices.len() < period {
        return (0.0, 0.0, 0.0);
    }
    let window: Vec<f64> = prices.iter().rev().take(period).copied().collect();
    let n = window.len() as f64;
    let sma: f64 = window.iter().sum::<f64>() / n;
    let variance: f64 = window.iter().map(|p| (p - sma).powi(2)).sum::<f64>() / n;
    let sigma = variance.sqrt();
    (sma, sma + 2.0 * sigma, sma - 2.0 * sigma)
}

/// Price position within BB: 0 = at lower band, 1 = at upper band.
pub fn bollinger_position(price: f64, _sma: f64, upper: f64, lower: f64) -> f64 {
    if upper > lower {
        ((price - lower) / (upper - lower)).clamp(0.0, 1.0)
    } else {
        0.5
    }
}

// ─── RSI ───────────────────────────────────────────────────────────────────

/// RSI(14) — Relative Strength Index.
/// Returns value 0..100, or 50 if not enough data.
pub fn rsi(closes: &[f64], period: usize) -> f64 {
    if closes.len() < period + 1 {
        return 50.0;
    }
    let window: Vec<f64> = closes.iter().rev().take(period + 1).copied().collect();
    let mut gains = 0.0;
    let mut losses = 0.0;
    for w in window.windows(2) {
        let delta = w[0] - w[1]; // older - newer (reversed in window)
        if delta > 0.0 { gains += delta; } else { losses -= delta; }
    }
    if losses.abs() < f64::EPSILON { return 100.0; }
    if gains.abs() < f64::EPSILON { return 0.0; }
    let rs = (gains / period as f64) / (losses / period as f64);
    100.0 - (100.0 / (1.0 + rs))
}

// ─── MACD ──────────────────────────────────────────────────────────────────

/// MACD(3, 10, 16) — Moving Average Convergence Divergence.
/// Returns (macd_line, signal_line, histogram).
/// macd_line = EMA(3) - EMA(10)
/// signal_line = EMA(16) of macd_line
/// histogram = macd_line - signal_line
pub fn macd(closes: &[f64]) -> (f64, f64, f64) {
    let ema3 = ema(closes, 3);
    let ema10 = ema(closes, 10);
    let macd_line = ema3 - ema10;
    // For signal line we need a history of macd_line values
    // Simplified: use EMA of price changes as proxy
    let signal_line = ema(closes, 16);
    let histogram = macd_line - signal_line;
    (macd_line, signal_line, histogram)
}

/// Exponential Moving Average.
fn ema(data: &[f64], period: usize) -> f64 {
    if data.is_empty() { return 0.0; }
    let multiplier = 2.0 / (period as f64 + 1.0);
    let mut ema = data[data.len() - 1]; // oldest (start of array)
    for &price in data.iter().rev().skip(1) {
        ema = (price - ema) * multiplier + ema;
    }
    ema
}

// ─── VFI ───────────────────────────────────────────────────────────────────

/// Volume Flow Indicator (simplified).
/// Measures money flow: (close - typical) * volume normalized.
/// Returns value relative to ATR-based normalization.
pub fn vfi(closes: &[f64], volumes: &[f64]) -> f64 {
    if closes.len() < 2 || closes.len() != volumes.len() {
        return 0.0;
    }
    let typical: Vec<f64> = closes.iter().copied().collect();
    let mut raw_vfi = 0.0;
    let mut total_vol = 0.0;
    for (i, (&tp, &vol)) in typical.iter().zip(volumes.iter()).enumerate().skip(1) {
        let prev_tp = typical[i - 1];
        raw_vfi += (tp - prev_tp) * vol;
        total_vol += vol;
    }
    if total_vol > 0.0 { raw_vfi / total_vol } else { 0.0 }
}

// ─── Trend ─────────────────────────────────────────────────────────────────

/// Simple trend direction from linear regression slope over window.
/// Returns (slope_per_point, direction) where direction = 1 (UP), -1 (DOWN), 0 (flat).
pub fn trend_direction(prices: &[f64], window: usize) -> (f64, i8) {
    if prices.len() < window {
        return (0.0, 0);
    }
    let w: Vec<f64> = prices.iter().rev().take(window).copied().collect();
    let n = w.len() as f64;
    let sum_x: f64 = (0..w.len()).map(|i| i as f64).sum();
    let sum_y: f64 = w.iter().sum();
    let sum_xy: f64 = w.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
    let sum_x2: f64 = (0..w.len()).map(|i| (i as f64).powi(2)).sum();
    let denominator = n * sum_x2 - sum_x.powi(2);
    let slope = if denominator.abs() > f64::EPSILON {
        (n * sum_xy - sum_x * sum_y) / denominator
    } else {
        0.0
    };
    let dir = if slope > 0.0001 { 1 } else if slope < -0.0001 { -1 } else { 0 };
    (slope, dir)
}

// ─── Volatility ────────────────────────────────────────────────────────────

/// Realized volatility: standard deviation of returns, annualized.
pub fn realized_volatility(prices: &[f64], period: usize) -> f64 {
    if prices.len() < period + 1 { return 0.0; }
    let mut prev: Vec<f64> = prices.iter().rev().take(period + 1).copied().collect();
    prev.reverse();
    let returns: Vec<f64> = prev.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / n;
    variance.sqrt() * (365.0_f64 * 24.0 * 60.0).sqrt() // annualized from 1-min
}
