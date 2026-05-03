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
//!   Spread: poly_spread > 0.02 → skip entry
//!   Momentum: direction from last 8 poly_mid slope (replaces fixed DOWN)
//!
//! CSV: fenixXX_trade (active), fenixXX_entry (entry price), fenixXX_pnl (live PnL)

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
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
    confirm_ticks: u32,  // ticks in range before entry
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
    recent_prices:   Vec<f64>,  // last MOMENTUM_WINDOW poly_mid for momentum calc
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
    sessions: Mutex<HashMap<i32, Vec<FenixSessionTrade>>>,
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
    /// skip_reason: 0=none, 1=trend blocked, 2=spread blocked, 3=volume blocked
    pub fn on_tick(&self, session_id: i32, poly_mid: f64, poly_bid: f64, poly_ask: f64,
                   predicted_bias: &str, poly_spread: f64,
                   binance_vol_100ms: f64, trades_per_second: f64,
                   poly_imbalance: f64, price_velocity: f64,
    ) -> Vec<(String, u8, f64, f64, u8)>
    {
        let mut sessions = self.sessions.lock().unwrap();
        let trades = sessions.entry(session_id).or_insert_with(|| {
            FENIX_DEFS.iter().map(|_| FenixSessionTrade::default()).collect()
        });

        let bias_up = predicted_bias.contains("UP");
        let bias_down = predicted_bias.contains("DOWN");

        let mut results = Vec::with_capacity(FENIX_DEFS.len());
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let in_range = poly_mid >= def.min && poly_mid <= def.max;
            if in_range {
                trades[i].ticks_in_range += 1;

                // ── Volume trigger: high activity → faster entry (reduce confirm by 1) ─
                let effective_confirm = if trades_per_second >= 1.5 && binance_vol_100ms > 0.5 {
                    def.confirm_ticks.saturating_sub(1).max(1)
                } else {
                    def.confirm_ticks
                };

                if !trades[i].entered && trades[i].ticks_in_range >= effective_confirm {
                    // ── Filter 4: spread gate (5% relative to mid price) ─────
                    let spread_ratio = if poly_mid > 0.0 { poly_spread / poly_mid } else { 1.0 };
                    if spread_ratio > 0.05 {
                        trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 2u8)); // skip=spread
                        continue;
                    }

                    // ── Filter 5: volume gate ─────────────────────────────
                    if binance_vol_100ms < 0.3 && trades_per_second < 1.0 {
                        trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 3u8)); // skip=volume
                        continue;
                    }

                    // ── Direction: MAJORITY, MOMENTUM, UP, DOWN ────────────
                    let mut dir_up = match def.direction {
                        "UP"       => true,
                        "DOWN"     => false,
                        "MOMENTUM" => {
                            let rp = &trades[i].recent_prices;
                            if rp.len() >= 4 {
                                let slope = rp[..].windows(2)
                                    .map(|w| w[1] - w[0])
                                    .sum::<f64>();
                                slope > 0.0
                            } else {
                                poly_mid > 0.5
                            }
                        }
                        _          => poly_mid > 0.5,
                    };

                    // ── Volume direction bias: override if strong signal ───
                    // Strong imbalance + BTC velocity aligns → confident direction
                    if poly_imbalance > 1.2 && price_velocity > 2.0 {
                        dir_up = true;  // buying pressure + BTC rising
                    } else if poly_imbalance < 0.8 && price_velocity < -2.0 {
                        dir_up = false; // selling pressure + BTC falling
                    }

                    // ── Filter 1: macro trend gate ─────────────────────────
                    if !bias_up && !bias_down {
                        // No bias → allow entry (warmup phase)
                    } else if dir_up && bias_down {
                        trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8)); // skip=trend
                        continue;
                    } else if !dir_up && bias_up {
                        trades[i].ticks_in_range = 0;
                        results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8)); // skip=trend
                        continue;
                    }

                    trades[i].entered = true;
                    trades[i].direction_up = dir_up;
                    trades[i].entry_price = if dir_up { poly_ask } else { poly_bid };
                    info!("[FenixTrading] #{} {} ENTER {}@{:.4} (spread={:.4} bias={} vol={:.2} tps={:.1} imb={:.2})",
                        session_id, def.name, if dir_up {"UP"} else {"DOWN"},
                        trades[i].entry_price, poly_spread, predicted_bias,
                        binance_vol_100ms, trades_per_second, poly_imbalance);
                }
            } else {
                if !trades[i].entered {
                    trades[i].ticks_in_range = 0;
                }
            }

            // Track recent prices for momentum (always, even if not in range)
            trades[i].recent_prices.push(poly_mid);
            if trades[i].recent_prices.len() > MOMENTUM_WINDOW {
                trades[i].recent_prices.remove(0);
            }

            let active = if trades[i].entered && !trades[i].settled { 1u8 } else { 0u8 };
            let entry = if active == 1 { trades[i].entry_price } else { 0.0 };

            let live_pnl = if active == 1 {
                if trades[i].direction_up {
                    poly_mid - trades[i].entry_price
                } else {
                    trades[i].entry_price - poly_mid
                }
            } else { 0.0 };

            results.push((def.code.to_string(), active, entry, live_pnl, 0u8));
        }
        results
    }

    /// Called at session close. Settles all open trades.
    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str) {
        let actual_up = actual_outcome.eq_ignore_ascii_case("up");
        let actual_down = actual_outcome.eq_ignore_ascii_case("down");
        let is_tie = !actual_up && !actual_down;

        let mut sessions = self.sessions.lock().unwrap();
        let trades = match sessions.remove(&session_id) {
            Some(t) => t,
            None => return,
        };

        let mut stats = self.stats.lock().unwrap();
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let trade = &trades[i];
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
