//! Fenix Trading — Paper-trading with independent strategies
//!
//! 5 strategies, $20 virtual each, DIFFERENT confirmation gates:
//!   Fenix 35-65: [0.35, 0.65] majority → confirm 1 tick (aggressive)
//!   Fenix 30-50: [0.30, 0.50] MOMENTUM  → confirm 3 ticks
//!   Fenix 45-55: [0.45, 0.55] majority → confirm 5 ticks
//!   Fenix 40-50: [0.40, 0.50] MOMENTUM  → confirm 7 ticks (conservative)
//!   Fenix 45-50: [0.45, 0.50] MOMENTUM  → confirm 10 ticks (v.conservative)
//!
//! Filters per tick:
//!   Trend:  don't enter DOWN if predicted_bias=UP (and vice versa)
//!   Spread: >200% relative → skip
//!   Volume gate: binance_vol_100ms < 0.3 && tps < 1.0 → skip
//!   Delta signal: bid/ask volume deltas + velocity → UP/DOWN signal
//!   Momentum: last 8 poly_mid slope
//!
//! CSV: fenixXX_trade, fenixXX_entry, fenixXX_pnl, fenixXX_skip, fenix_signal

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tracing::info;

// ─── Fenix Definitions ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FenixDef {
    name:          &'static str,
    code:          &'static str,
    min:           f64,
    max:           f64,
    direction:     &'static str,
    confirm_ticks: u32,
}

static FENIX_DEFS: &[FenixDef] = &[
    FenixDef { name: "Fenix 35-65", code: "fenix35", min: 0.35, max: 0.65, direction: "MAJORITY", confirm_ticks: 1 },
    FenixDef { name: "Fenix 30-50", code: "fenix30", min: 0.30, max: 0.50, direction: "MOMENTUM", confirm_ticks: 3 },
    FenixDef { name: "Fenix 45-55", code: "fenix45", min: 0.45, max: 0.55, direction: "MAJORITY", confirm_ticks: 5 },
    FenixDef { name: "Fenix 40-50", code: "fenix40", min: 0.40, max: 0.50, direction: "MOMENTUM", confirm_ticks: 7 },
    FenixDef { name: "Fenix 45-50", code: "fenix4550", min: 0.45, max: 0.50, direction: "MOMENTUM", confirm_ticks: 10 },
];

// ─── Per-Strategy Session State ────────────────────────────────────────────

const MOMENTUM_WINDOW: usize = 8;

#[derive(Debug, Clone)]
struct FenixSessionTrade {
    entered:         bool,
    direction_up:    bool,
    entry_price:     f64,
    ticks_in_range:  u32,
    settled:         bool,
    virtual_pnl:     f64,
    correct:         bool,
    recent_prices:   Vec<f64>,
}

impl Default for FenixSessionTrade {
    fn default() -> Self {
        Self {
            entered: false, direction_up: false, entry_price: 0.0,
            ticks_in_range: 0, settled: false, virtual_pnl: 0.0, correct: false,
            recent_prices: Vec::with_capacity(MOMENTUM_WINDOW),
        }
    }
}

// ─── Per-Session Delta Tracker ──────────────────────────────────────────────

struct FenixSessionState {
    trades:        Vec<FenixSessionTrade>,
    prev_bid_vol:  f64,
    prev_ask_vol:  f64,
    prev_imb:      f64,
}

/// Compute composite delta + velocity signal: 0=none, 1=UP, 2=DOWN
fn compute_fenix_signal(
    bid_vol: f64, ask_vol: f64, imb: f64,
    prev_bid: f64, prev_ask: f64, prev_imb: f64,
    velocity: f64,
) -> u8 {
    let delta_bid = bid_vol - prev_bid;
    let delta_ask = ask_vol - prev_ask;
    let delta_imb = imb - prev_imb;

    // Strong UP: volume shifting from ask→bid + BTC rising
    if velocity > 2.0 && delta_bid > 0.0 && delta_ask < 0.0 {
        return 1;
    }
    // Strong UP: velocity alone strong + imbalance increasing
    if velocity > 3.0 && delta_imb > 0.05 {
        return 1;
    }
    // Strong DOWN: volume shifting from bid→ask + BTC falling
    if velocity < -2.0 && delta_bid < 0.0 && delta_ask > 0.0 {
        return 2;
    }
    // Strong DOWN: velocity alone strong + imbalance decreasing
    if velocity < -3.0 && delta_imb < -0.05 {
        return 2;
    }
    // Weak UP
    if velocity > 1.0 && delta_imb > 0.0 {
        return 1;
    }
    // Weak DOWN
    if velocity < -1.0 && delta_imb < 0.0 {
        return 2;
    }
    // No clear signal
    0
}

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FenixStats {
    pub name:           String,
    pub code:           String,
    pub range:          String,
    pub direction:      String,
    pub confirm_ticks:  u32,
    pub capital:        f64,
    pub balance:        f64,
    pub trades:         u64,
    pub wins:           u64,
    pub accuracy:       f64,
    pub total_pnl:      f64,
    pub avg_pnl:        f64,
    pub best_pnl:       f64,
    pub worst_pnl:      f64,
    pub sessions_tracked: u64,
    pub last_10:        Vec<bool>,
}

impl FenixStats {
    fn new(def: &FenixDef) -> Self {
        Self {
            name: def.name.into(), code: def.code.into(),
            range: format!("[{:.2}, {:.2}]", def.min, def.max),
            direction: def.direction.into(), confirm_ticks: def.confirm_ticks,
            capital: 20.0, balance: 20.0, trades: 0, wins: 0,
            accuracy: 0.0, total_pnl: 0.0, avg_pnl: 0.0,
            best_pnl: 0.0, worst_pnl: 0.0, sessions_tracked: 0,
            last_10: Vec::with_capacity(10),
        }
    }
}

// ─── Fenix Trading Manager ─────────────────────────────────────────────────

pub struct FenixTradingManager {
    sessions: Mutex<HashMap<i32, FenixSessionState>>,
    stats:    Mutex<Vec<FenixStats>>,
}

impl FenixTradingManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            stats:    Mutex::new(FENIX_DEFS.iter().map(FenixStats::new).collect()),
        }
    }

    /// On each tick: check entry for each strategy independently.
    /// Returns: Vec of (code, active, entry_price, unrealized_pnl, skip_reason)
    /// + fenix_signal: 0=none, 1=UP, 2=DOWN (delta+velocity composite)
    pub fn on_tick(&self, session_id: i32, poly_mid: f64, poly_bid: f64, poly_ask: f64,
                   predicted_bias: &str, poly_spread: f64,
                   binance_vol_100ms: f64, trades_per_second: f64,
                   poly_imbalance: f64, price_velocity: f64,
                   poly_bid_vol_all: f64, poly_ask_vol_all: f64,
    ) -> (Vec<(String, u8, f64, f64, u8)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| FenixSessionState {
            trades:        FENIX_DEFS.iter().map(|_| FenixSessionTrade::default()).collect(),
            prev_bid_vol:  poly_bid_vol_all,
            prev_ask_vol:  poly_ask_vol_all,
            prev_imb:      poly_imbalance,
        });

        // ── Delta signal ──────────────────────────────────────────────────
        let fenix_signal = compute_fenix_signal(
            poly_bid_vol_all, poly_ask_vol_all, poly_imbalance,
            state.prev_bid_vol, state.prev_ask_vol, state.prev_imb,
            price_velocity,
        );
        state.prev_bid_vol = poly_bid_vol_all;
        state.prev_ask_vol = poly_ask_vol_all;
        state.prev_imb     = poly_imbalance;

        let bias_up = predicted_bias.contains("UP");
        let bias_down = predicted_bias.contains("DOWN");

        let mut results = Vec::with_capacity(FENIX_DEFS.len());
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            // In range = real two-sided market: both sides have volume + prices exist
            let in_range = poly_bid_vol_all > 10.0 && poly_ask_vol_all > 10.0
                        && poly_bid > 0.0 && poly_ask > 0.0
                        && poly_mid >= def.min && poly_mid <= def.max;

            if in_range {
                state.trades[i].ticks_in_range += 1;

                let effective_confirm = if trades_per_second >= 1.5 && binance_vol_100ms > 0.5 {
                    def.confirm_ticks.saturating_sub(1).max(1)
                } else {
                    def.confirm_ticks
                };

                if !state.trades[i].entered && state.trades[i].ticks_in_range >= effective_confirm {
                    // ── Filter 4: spread gate (200% relative) ────────────
                    let spread_ratio = if poly_mid > 0.0 { poly_spread / poly_mid } else { 1.0 };
                    if spread_ratio > 2.0 {
                        state.trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 2u8));
                        continue;
                    }

                    // ── Filter 5: volume gate (polymarket depth, not binance) ───
                    if poly_bid_vol_all < 5.0 || poly_ask_vol_all < 5.0 {
                        state.trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 3u8));
                        continue;
                    }

                    // ── Direction: DELTA SIGNAL overrides others ────────
                    let mut dir_up = if fenix_signal == 1 {
                        true // delta+velocity says UP
                    } else if fenix_signal == 2 {
                        false // delta+velocity says DOWN
                    } else {
                        match def.direction {
                            "UP"       => true,
                            "DOWN"     => false,
                            "MOMENTUM" => {
                                let rp = &state.trades[i].recent_prices;
                                if rp.len() >= 4 {
                                    let slope = rp[..].windows(2)
                                        .map(|w| w[1] - w[0])
                                        .sum::<f64>();
                                    slope > 0.0
                                } else {
                                    poly_mid > 0.5
                                }
                            }
                            _ => poly_mid > 0.5,
                        }
                    };

                    // ── Volume direction bias (fallback override) ───────
                    if fenix_signal == 0 {
                        if poly_imbalance > 1.2 && price_velocity > 2.0 {
                            dir_up = true;
                        } else if poly_imbalance < 0.8 && price_velocity < -2.0 {
                            dir_up = false;
                        }
                    }

                    // ── Filter 1: macro trend gate ──────────────────────
                    if !bias_up && !bias_down {
                        // warmup
                    } else if dir_up && bias_down {
                        state.trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8));
                        continue;
                    } else if !dir_up && bias_up {
                        state.trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8));
                        continue;
                    }

                    state.trades[i].entered = true;
                    state.trades[i].direction_up = dir_up;
                    state.trades[i].entry_price = if dir_up { poly_ask } else { poly_bid };
                    info!("[FenixTrading] #{} {} ENTER {}@{:.4} (sgn={} spread={:.4} bias={} vol={:.2} tps={:.1} imb={:.2} bidV={:.0} askV={:.0})",
                        session_id, def.name, if dir_up {"UP"} else {"DOWN"},
                        state.trades[i].entry_price, fenix_signal,
                        poly_spread, predicted_bias,
                        binance_vol_100ms, trades_per_second, poly_imbalance,
                        poly_bid_vol_all, poly_ask_vol_all);
                }
            } else {
                if !state.trades[i].entered {
                    state.trades[i].ticks_in_range = 0;
                }
            }

            state.trades[i].recent_prices.push(poly_mid);
            if state.trades[i].recent_prices.len() > MOMENTUM_WINDOW {
                state.trades[i].recent_prices.remove(0);
            }

            let active = if state.trades[i].entered && !state.trades[i].settled { 1u8 } else { 0u8 };
            let entry = if active == 1 { state.trades[i].entry_price } else { 0.0 };
            let live_pnl = if active == 1 {
                if state.trades[i].direction_up {
                    poly_mid - state.trades[i].entry_price
                } else {
                    state.trades[i].entry_price - poly_mid
                }
            } else { 0.0 };

            results.push((def.code.to_string(), active, entry, live_pnl, 0u8));
        }
        (results, fenix_signal)
    }

    /// Called at session close. Settles all open trades.
    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str) {
        let actual_up = actual_outcome.eq_ignore_ascii_case("up");
        let actual_down = actual_outcome.eq_ignore_ascii_case("down");
        let is_tie = !actual_up && !actual_down;

        let mut sessions = self.sessions.lock().unwrap();
        let state = match sessions.remove(&session_id) {
            Some(s) => s,
            None => return,
        };

        let mut stats = self.stats.lock().unwrap();
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let trade = &state.trades[i];
            if !trade.entered { continue; }

            let pnl = if is_tie {
                0.0
            } else if trade.direction_up {
                if actual_up { 1.0 - trade.entry_price } else { -trade.entry_price }
            } else {
                if actual_down { trade.entry_price } else { -(1.0 - trade.entry_price) }
            };

            let correct = (trade.direction_up && actual_up) || (!trade.direction_up && actual_down);

            stats[i].balance += pnl;
            stats[i].trades += 1;
            if correct { stats[i].wins += 1; }
            stats[i].total_pnl += pnl;
            stats[i].accuracy = stats[i].wins as f64 / stats[i].trades as f64;
            stats[i].avg_pnl = stats[i].total_pnl / stats[i].trades as f64;
            if stats[i].trades == 1 || pnl > stats[i].best_pnl { stats[i].best_pnl = pnl; }
            if stats[i].trades == 1 || pnl < stats[i].worst_pnl { stats[i].worst_pnl = pnl; }
            stats[i].last_10.push(correct);
            if stats[i].last_10.len() > 10 { stats[i].last_10.remove(0); }

            info!("[FenixTrading] #{} {} SETTLED: {}@{}→{} pnl={:.4} bal=${:.2}",
                session_id, def.name,
                if trade.direction_up {"UP"} else {"DOWN"}, trade.entry_price,
                if correct {"✓"} else {"✗"}, pnl, stats[i].balance);
        }
        for s in stats.iter_mut() { s.sessions_tracked += 1; }
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&*self.stats.lock().unwrap()).unwrap_or_default()
    }
}
