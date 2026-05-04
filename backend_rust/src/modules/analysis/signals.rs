//! Signal Detection — Master Signal, Confluence, Gap Alerts
//!
//! Layer 2: Analysis. Evalúa condiciones de mercado y produce señales.
//! Las señales alimentan a Layer 3 (Trading) para decisiones de entrada/salida.

// ─── Master Signal ─────────────────────────────────────────────────────────

/// Master signal types (matches CSV column #51).
/// 0=none, 1=BB_BUY, 2=BB_SELL, 3-4=HUNT, 5-6=MOM, 7-8=MICRO, 9-10=CP-ONLY
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MasterSignal {
    None = 0,
    BbBuy = 1,
    BbSell = 2,
    HuntBuy = 3,
    HuntSell = 4,
    MomBuy = 5,
    MomSell = 6,
    MicroBuy = 7,
    MicroSell = 8,
    CpOnlyBuy = 9,
    CpOnlySell = 10,
}

impl MasterSignal {
    pub fn to_u8(self) -> u8 { self as u8 }
    pub fn is_buy(self) -> bool {
        matches!(self, Self::BbBuy | Self::HuntBuy | Self::MomBuy | Self::MicroBuy | Self::CpOnlyBuy)
    }
    pub fn is_active(self) -> bool { self != Self::None }
}

/// Evaluate master signal from market conditions.
///
/// Priority:
///   1. BB touch + imbalance confluence → BB_BUY/SELL
///   2. Price hunting (extreme deviation from SMA) → HUNT
///   3. Momentum threshold → MOM
///   4. Micro-price vs mid divergence → MICRO
///   5. Conformal Prediction valid but no technical confluence → CP-ONLY
pub fn evaluate_master_signal(
    price: f64, sma: f64, upper: f64, lower: f64,
    imbalance: f64, velocity: f64,
    cp_valid: bool, macd_hist: f64, rsi: f64,
) -> MasterSignal {
    // Guard: need valid BB
    if sma <= 0.0 || upper <= lower {
        if cp_valid {
            let is_up = macd_hist > 0.0 || rsi > 50.0;
            return if is_up { MasterSignal::CpOnlyBuy } else { MasterSignal::CpOnlySell };
        }
        return MasterSignal::None;
    }

    // 1. Bollinger Band touch
    let below_lower = price <= lower;
    let above_upper = price >= upper;

    if below_lower && imbalance > 0.6 {
        MasterSignal::BbBuy
    } else if above_upper && imbalance < -0.6 {
        MasterSignal::BbSell
    }
    // 2. Hunting — price > 2σ away
    else if below_lower && (lower - price) > (sma - lower) * 0.5 {
        if velocity > 0.0 { MasterSignal::HuntBuy } else { MasterSignal::None }
    } else if above_upper && (price - upper) > (upper - sma) * 0.5 {
        if velocity < 0.0 { MasterSignal::HuntSell } else { MasterSignal::None }
    }
    // 3. Momentum threshold
    else if velocity > 5.0 {
        MasterSignal::MomBuy
    } else if velocity < -5.0 {
        MasterSignal::MomSell
    }
    // 4. CP-only
    else if cp_valid {
        let is_up = macd_hist > 0.0 || rsi > 50.0;
        if is_up { MasterSignal::CpOnlyBuy } else { MasterSignal::CpOnlySell }
    }
    else {
        MasterSignal::None
    }
}

// ─── Confluence ────────────────────────────────────────────────────────────

/// Technical confluence score: how many indicators agree on direction.
/// Returns 0-4 where each bit = one indicator aligned.
pub fn confluence_score(
    trend: i8, rsi: f64, macd_hist: f64, vfi: f64,
) -> u8 {
    let mut score = 0u8;
    let up = |v: f64| v > 0.0;
    let down = |v: f64| v < 0.0;

    // Trend
    if trend == 1 { score |= 0b1000; }
    else if trend == -1 { score |= 0b0001; }

    // RSI
    if rsi > 60.0 { score |= 0b0100; }
    else if rsi < 40.0 { score |= 0b0010; }

    // MACD
    if up(macd_hist) { score |= 0b0100; }
    else if down(macd_hist) { score |= 0b0010; }

    // VFI
    if up(vfi) { score |= 0b0100; }
    else if down(vfi) { score |= 0b0010; }

    score
}

// ─── Gap Alert ─────────────────────────────────────────────────────────────

/// Detects significant price divergence between Binance and Polymarket.
/// Returns true if gap exceeds threshold (default 0.5%).
pub fn gap_alert(binance_micro: f64, poly_mid: f64, threshold_pct: f64) -> bool {
    if binance_micro <= 0.0 || poly_mid <= 0.0 { return false; }
    let gap = ((binance_micro - poly_mid).abs() / binance_micro) * 100.0;
    gap > threshold_pct
}

// ─── Fenix Signal ──────────────────────────────────────────────────────────

/// Fenix signal: delta-based direction from orderbook volume changes + velocity.
/// 0 = none, 1 = UP, 2 = DOWN.
pub fn fenix_signal(
    bid_vol: f64, ask_vol: f64, imb: f64,
    prev_bid: f64, prev_ask: f64, prev_imb: f64,
    velocity: f64,
) -> u8 {
    let delta_bid = bid_vol - prev_bid;
    let delta_ask = ask_vol - prev_ask;
    let delta_imb = imb - prev_imb;

    if velocity > 2.0 && delta_bid > 0.0 && delta_ask < 0.0 { return 1; }
    if velocity > 3.0 && delta_imb > 0.05 { return 1; }
    if velocity < -2.0 && delta_bid < 0.0 && delta_ask > 0.0 { return 2; }
    if velocity < -3.0 && delta_imb < -0.05 { return 2; }
    if velocity > 1.0 && delta_imb > 0.0 { return 1; }
    if velocity < -1.0 && delta_imb < 0.0 { return 2; }
    0
}

// ─── Spread Health ─────────────────────────────────────────────────────────

/// Check if spread is acceptable for trading.
/// Returns (ok, spread_ratio).
/// spread_ok = spread/mid < max_ratio (default 0.05 for Binance, 2.0 for Poly)
pub fn spread_health(mid: f64, spread: f64, max_ratio: f64) -> (bool, f64) {
    let ratio = if mid > 0.0 { spread / mid } else { 1.0 };
    (ratio < max_ratio, ratio)
}

// ─── Volume Gate ───────────────────────────────────────────────────────────

/// Volume gate: is there enough liquidity for a trade?
pub fn volume_gate(bid_vol: f64, ask_vol: f64, min_vol: f64) -> bool {
    bid_vol > min_vol || ask_vol > min_vol
}

// ─── Market Active ─────────────────────────────────────────────────────────

/// Is the orderbook in a tradeable state?
pub fn market_active(bid: f64, ask: f64, bid_vol: f64, ask_vol: f64) -> bool {
    (bid_vol > 5.0 || ask_vol > 5.0) && (bid > 0.0 || ask > 0.0)
}

// ─── Direction Bias ────────────────────────────────────────────────────────

/// Determine trading direction from momentum slope.
pub fn momentum_direction(prices: &[f64]) -> Option<bool> {
    if prices.len() < 4 { return None; }
    let slope: f64 = prices.windows(2).map(|w| w[1] - w[0]).sum();
    if slope.abs() < 1e-10 { None } else { Some(slope > 0.0) }
}
