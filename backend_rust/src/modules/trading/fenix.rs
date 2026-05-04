//! Fenix — Range-Based Paper Trading (5 strategies)
//!
//! Layer 3: Trading Strategy. Recibe datos de Layer 1, señales de Layer 2.
//! $20 virtual por estrategia. Entry en borde favorable, exit en borde opuesto.
//!
//! Estrategias: Fenix 35-65 (1 tick), 30-50 (3), 45-55 (5), 40-50 (7), 45-50 (10)

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tracing::info;

use crate::modules::analysis::signals;

// ─── Fenix Definitions ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FenixDef {
    name:          &'static str,
    code:          &'static str,
    min:           f64,
    max:           f64,
    confirm_ticks: u32,
}

static FENIX_DEFS: &[FenixDef] = &[
    FenixDef { name: "Fenix 35-65", code: "fenix35", min: 0.35, max: 0.65, confirm_ticks: 1 },
    FenixDef { name: "Fenix 30-50", code: "fenix30", min: 0.30, max: 0.50, confirm_ticks: 3 },
    FenixDef { name: "Fenix 45-55", code: "fenix45", min: 0.45, max: 0.55, confirm_ticks: 5 },
    FenixDef { name: "Fenix 40-50", code: "fenix40", min: 0.40, max: 0.50, confirm_ticks: 7 },
    FenixDef { name: "Fenix 45-50", code: "fenix4550", min: 0.45, max: 0.50, confirm_ticks: 10 },
];

const MOMENTUM_WINDOW: usize = 8;

// ─── Per-Strategy Session State ────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FenixSessionTrade {
    entered:       bool,
    direction_up:  bool,
    entry_price:   f64,
    target_price:  f64,
    ticks_near:    u32,
    settled:       bool,
    virtual_pnl:   f64,
    recent_prices: Vec<f64>,
}

impl Default for FenixSessionTrade {
    fn default() -> Self {
        Self {
            entered: false, direction_up: false, entry_price: 0.0, target_price: 0.0,
            ticks_near: 0, settled: false, virtual_pnl: 0.0,
            recent_prices: Vec::with_capacity(MOMENTUM_WINDOW),
        }
    }
}

struct FenixSessionState {
    trades:       Vec<FenixSessionTrade>,
    prev_bid_vol: f64,
    prev_ask_vol: f64,
    prev_imb:     f64,
}

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FenixStats {
    pub name:           String,
    pub code:           String,
    pub range:          String,
    pub confirm_ticks:  u32,
    pub capital:        f64,
    pub balance:        f64,
    pub session_pnl:    f64,
    pub session_balance: f64,
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
            confirm_ticks: def.confirm_ticks,
            capital: 20.0, balance: 20.0, session_pnl: 0.0, session_balance: 20.0,
            trades: 0, wins: 0,
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

    /// On each tick: evaluate all 5 strategies.
    /// Returns: Vec of (code, active, entry_price, pnl, skip, target, exited) + fenix_signal.
    pub fn on_tick(
        &self, session_id: i32,
        poly_mid: f64, poly_bid: f64, poly_ask: f64,
        predicted_bias: &str, poly_spread: f64,
        _binance_vol_100ms: f64, trades_per_second: f64,
        poly_imbalance: f64, price_velocity: f64,
        poly_bid_vol_all: f64, poly_ask_vol_all: f64,
    ) -> (Vec<(String, u8, f64, f64, u8, f64, u8)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| FenixSessionState {
            trades: FENIX_DEFS.iter().map(|_| FenixSessionTrade::default()).collect(),
            prev_bid_vol: poly_bid_vol_all,
            prev_ask_vol: poly_ask_vol_all,
            prev_imb: poly_imbalance,
        });

        let fenix_sig = signals::fenix_signal(
            poly_bid_vol_all, poly_ask_vol_all, poly_imbalance,
            state.prev_bid_vol, state.prev_ask_vol, state.prev_imb,
            price_velocity,
        );
        state.prev_bid_vol = poly_bid_vol_all;
        state.prev_ask_vol = poly_ask_vol_all;
        state.prev_imb = poly_imbalance;

        let bias_up = predicted_bias.contains("UP");
        let bias_down = predicted_bias.contains("DOWN");

        let mut results = Vec::with_capacity(FENIX_DEFS.len());
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let t = &mut state.trades[i];
            let range = def.max - def.min;

            // Exit check
            let mut just_exited = false;
            if t.entered && !t.settled {
                if t.direction_up && poly_mid >= t.target_price {
                    t.settled = true;
                    t.virtual_pnl = t.target_price - t.entry_price;
                    just_exited = true;
                    info!("[Fenix] #{} {} EXIT UP to {:.4} from {:.4} pnl={:.4}",
                        session_id, def.name, t.target_price, t.entry_price, t.virtual_pnl);
                } else if !t.direction_up && poly_mid <= t.target_price {
                    t.settled = true;
                    t.virtual_pnl = t.entry_price - t.target_price;
                    just_exited = true;
                    info!("[Fenix] #{} {} EXIT DOWN to {:.4} from {:.4} pnl={:.4}",
                        session_id, def.name, t.target_price, t.entry_price, t.virtual_pnl);
                }
            }

            if just_exited || t.settled {
                let active = if t.entered && !t.settled { 1u8 } else { 0u8 };
                let pnl = if t.settled { t.virtual_pnl } else { 0.0 };
                results.push((def.code.to_string(), active, t.entry_price, pnl, 0u8, t.target_price, if t.settled { 1u8 } else { 0u8 }));
                continue;
            }

            // Market guard
            let market_ok = signals::market_active(poly_bid, poly_ask, poly_bid_vol_all, poly_ask_vol_all);
            if !market_ok {
                t.ticks_near = 0;
                results.push((def.code.to_string(), 0u8, 0.0, 0.0, 0u8, 0.0, 0u8));
                t.recent_prices.push(poly_mid);
                if t.recent_prices.len() > MOMENTUM_WINDOW { t.recent_prices.remove(0); }
                continue;
            }

            // Direction
            let dir_up = if fenix_sig == 1 {
                true
            } else if fenix_sig == 2 {
                false
            } else if poly_imbalance > 1.2 && price_velocity > 2.0 {
                true
            } else if poly_imbalance < 0.8 && price_velocity < -2.0 {
                false
            } else {
                let rp = &t.recent_prices;
                if rp.len() >= 4 {
                    let slope: f64 = rp.windows(2).map(|w| w[1] - w[0]).sum();
                    slope > 0.0
                } else {
                    poly_mid > 0.5
                }
            };

            // Trend gate
            if bias_up || bias_down {
                if dir_up && bias_down { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8, 0.0, 0u8)); continue; }
                if !dir_up && bias_up { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8, 0.0, 0u8)); continue; }
            }

            // Spread gate
            let (spread_ok, _spread_ratio) = signals::spread_health(poly_mid, poly_spread, 2.0);
            if !spread_ok { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 2u8, 0.0, 0u8)); continue; }

            // Entry zone
            let margin = range * 0.25;
            let near_up = dir_up && poly_mid <= def.min + margin;
            let near_down = !dir_up && poly_mid >= def.max - margin;

            if near_up || near_down {
                t.ticks_near += 1;
                let eff_confirm = if trades_per_second >= 1.5 && poly_bid_vol_all > 50.0 {
                    def.confirm_ticks.saturating_sub(1).max(1)
                } else {
                    def.confirm_ticks
                };
                if !t.entered && t.ticks_near >= eff_confirm {
                    let entry_price = if dir_up { poly_ask } else { poly_bid };
                    t.entered = true;
                    t.direction_up = dir_up;
                    t.entry_price = entry_price;
                    t.target_price = if dir_up { def.max * 0.95 } else { def.min * 1.05 };
                    info!("[Fenix] #{} {} ENTER {}@{:.4}>{:.4} sgn={} vel={:.1} imb={:.2} bidV={:.0} askV={:.0}",
                        session_id, def.name, if dir_up {"UP"} else {"DOWN"},
                        entry_price, t.target_price, fenix_sig,
                        price_velocity, poly_imbalance, poly_bid_vol_all, poly_ask_vol_all);
                }
            } else {
                t.ticks_near = 0;
            }

            t.recent_prices.push(poly_mid);
            if t.recent_prices.len() > MOMENTUM_WINDOW { t.recent_prices.remove(0); }

            let active = if t.entered && !t.settled { 1u8 } else { 0u8 };
            let pnl = if active == 1 {
                if t.direction_up { poly_mid - t.entry_price } else { t.entry_price - poly_mid }
            } else if t.settled { t.virtual_pnl } else { 0.0 };

            results.push((def.code.to_string(), active, t.entry_price, pnl, 0u8, t.target_price, if t.settled { 1u8 } else { 0u8 }));
        }
        (results, fenix_sig)
    }

    /// Settle all open trades at session close.
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
        for s in stats.iter_mut() {
            s.session_balance = 20.0;
            s.session_pnl = 0.0;
        }

        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let trade = &state.trades[i];
            if !trade.entered { continue; }
            let pnl = if trade.settled {
                trade.virtual_pnl
            } else if is_tie {
                0.0
            } else if trade.direction_up {
                if actual_up { 1.0 - trade.entry_price } else { -trade.entry_price }
            } else {
                if actual_down { trade.entry_price } else { -(1.0 - trade.entry_price) }
            };
            let correct = pnl > 0.0;
            stats[i].session_pnl += pnl;
            stats[i].session_balance += pnl;
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
            info!("[Fenix] #{} {} SETTLED: {}@{}→{} pnl={:.4} bal=${:.2}|${:.2} {}",
                session_id, def.name,
                if trade.direction_up {"UP"} else {"DOWN"}, trade.entry_price, trade.target_price,
                pnl, stats[i].session_balance, stats[i].balance,
                if trade.settled { "[target hit]"} else { "[session end]" });
        }
        for s in stats.iter_mut() { s.sessions_tracked += 1; }
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&*self.stats.lock().unwrap()).unwrap_or_default()
    }
}
