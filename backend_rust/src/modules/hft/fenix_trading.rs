//! Fenix Trading — Range-based paper-trading with velocity edge
//!
//! 5 strategies, $20 virtual each. Entry at favorable edge of range, exit at opposite edge:
//!   Fenix 35-65: UP@0.35→0.55, DOWN@0.65→0.45 — confirm 1 tick
//!   Fenix 30-50: UP@0.30→0.45, DOWN@0.50→0.35 — confirm 3 ticks
//!   Fenix 45-55: UP@0.45→0.52, DOWN@0.55→0.48 — confirm 5 ticks
//!   Fenix 40-50: UP@0.40→0.48, DOWN@0.50→0.42 — confirm 7 ticks
//!   Fenix 45-50: UP@0.45→0.49, DOWN@0.50→0.46 — confirm 10 ticks
//!
//! Edge: BTC move seen on Binance → Polymarket price still old → enter before repricing (50ms)
//! CSV: fenixXX_trade, fenixXX_entry, fenixXX_pnl, fenixXX_skip, fenixXX_target, fenixXX_exit

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
    max:           f64,           // exit target for UP, entry zone for DOWN
    confirm_ticks: u32,
}

static FENIX_DEFS: &[FenixDef] = &[
    FenixDef { name: "Fenix 35-65", code: "fenix35", min: 0.35, max: 0.65, confirm_ticks: 1 },
    FenixDef { name: "Fenix 30-50", code: "fenix30", min: 0.30, max: 0.50, confirm_ticks: 3 },
    FenixDef { name: "Fenix 45-55", code: "fenix45", min: 0.45, max: 0.55, confirm_ticks: 5 },
    FenixDef { name: "Fenix 40-50", code: "fenix40", min: 0.40, max: 0.50, confirm_ticks: 7 },
    FenixDef { name: "Fenix 45-50", code: "fenix4550", min: 0.45, max: 0.50, confirm_ticks: 10 },
];

// ─── Per-Strategy Session State ────────────────────────────────────────────

const MOMENTUM_WINDOW: usize = 8;

#[derive(Debug, Clone)]
struct FenixSessionTrade {
    entered:         bool,
    direction_up:    bool,
    entry_price:     f64,
    target_price:    f64,     // exit target
    ticks_near:      u32,     // ticks near entry edge
    settled:         bool,
    virtual_pnl:     f64,
    correct:         bool,
    recent_prices:   Vec<f64>,
}

impl Default for FenixSessionTrade {
    fn default() -> Self {
        Self {
            entered: false, direction_up: false, entry_price: 0.0, target_price: 0.0,
            ticks_near: 0, settled: false, virtual_pnl: 0.0, correct: false,
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

fn compute_fenix_signal(
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

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FenixStats {
    pub name:           String,
    pub code:           String,
    pub range:          String,
    pub confirm_ticks:  u32,
    pub capital:        f64,
    pub balance:        f64,           // acumulativo desde inicio
    pub session_pnl:    f64,           // PnL de la última sesión
    pub session_balance: f64,          // balance al cierre de última sesión ($20 + PnL)
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

    /// Returns: (strategy_trades, fenix_signal)
    /// Each trade: (code, active, entry_price, unrealized_pnl, skip_reason, target_price, exited)
    pub fn on_tick(&self, session_id: i32, poly_mid: f64, poly_bid: f64, poly_ask: f64,
                   predicted_bias: &str, poly_spread: f64,
                   _binance_vol_100ms: f64, trades_per_second: f64,
                   poly_imbalance: f64, price_velocity: f64,
                   poly_bid_vol_all: f64, poly_ask_vol_all: f64,
    ) -> (Vec<(String, u8, f64, f64, u8, f64, u8)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| FenixSessionState {
            trades:        FENIX_DEFS.iter().map(|_| FenixSessionTrade::default()).collect(),
            prev_bid_vol:  poly_bid_vol_all,
            prev_ask_vol:  poly_ask_vol_all,
            prev_imb:      poly_imbalance,
        });

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
            let t = &mut state.trades[i];
            let range = def.max - def.min;

            // ── Exit check: price hit target? ────────────────────────────
            let mut just_exited = false;
            if t.entered && !t.settled {
                if t.direction_up && poly_mid >= t.target_price {
                    t.settled = true;
                    t.virtual_pnl = t.target_price - t.entry_price;
                    just_exited = true;
                    info!("[FenixTrading] #{} {} EXIT UP to {:.4} from {:.4} pnl={:.4}",
                        session_id, def.name, t.target_price, t.entry_price, t.virtual_pnl);
                } else if !t.direction_up && poly_mid <= t.target_price {
                    t.settled = true;
                    t.virtual_pnl = t.entry_price - t.target_price;
                    just_exited = true;
                    info!("[FenixTrading] #{} {} EXIT DOWN to {:.4} from {:.4} pnl={:.4}",
                        session_id, def.name, t.target_price, t.entry_price, t.virtual_pnl);
                }
            }

            if just_exited || t.settled {
                let active = if t.entered && !t.settled { 1u8 } else { 0u8 };
                let pnl = if t.settled { t.virtual_pnl } else { 0.0 };
                results.push((def.code.to_string(), active, t.entry_price, pnl, 0u8, t.target_price, if t.settled { 1u8 } else { 0u8 }));
                continue;
            }

            // ── Entry check: price near favorable edge + direction confirmed ─

            // Is market active? (volume + two-sided)
            let market_active = poly_bid_vol_all > 10.0 && poly_ask_vol_all > 10.0
                             && poly_bid > 0.0 && poly_ask > 0.0;

            if !market_active {
                t.ticks_near = 0;
                results.push((def.code.to_string(), 0u8, 0.0, 0.0, 0u8, 0.0, 0u8));
                t.recent_prices.push(poly_mid);
                if t.recent_prices.len() > MOMENTUM_WINDOW { t.recent_prices.remove(0); }
                continue;
            }

            // ── Direction: signal → momentum → majority → imbalance ─────
            let dir_up = if fenix_signal == 1 {
                true
            } else if fenix_signal == 2 {
                false
            } else if poly_imbalance > 1.2 && price_velocity > 2.0 {
                true
            } else if poly_imbalance < 0.8 && price_velocity < -2.0 {
                false
            } else {
                // momentum fallback
                let rp = &t.recent_prices;
                if rp.len() >= 4 {
                    let slope = rp[..].windows(2).map(|w| w[1] - w[0]).sum::<f64>();
                    slope > 0.0
                } else {
                    poly_mid > 0.5
                }
            };

            // ── Filter 1: trend gate ─────────────────────────────────────
            if bias_up || bias_down {
                if dir_up && bias_down { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8, 0.0, 0u8)); continue; }
                if !dir_up && bias_up { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 1u8, 0.0, 0u8)); continue; }
            }

            // ── Filter 4: spread gate ────────────────────────────────────
            let spread_ratio = if poly_mid > 0.0 { poly_spread / poly_mid } else { 1.0 };
            if spread_ratio > 2.0 { t.ticks_near = 0; results.push((def.code.to_string(), 0u8, 0.0, 0.0, 2u8, 0.0, 0u8)); continue; }

            // ── Entry zone: price at favorable edge ──────────────────────
            // UP: enter near min (cheap, buy low). DOWN: enter near max (expensive, sell high)
            let entry_margin = range * 0.25; // 25% of range width from the edge
            let near_entry_up = dir_up && poly_mid <= def.min + entry_margin;
            let near_entry_down = !dir_up && poly_mid >= def.max - entry_margin;

            if near_entry_up || near_entry_down {
                t.ticks_near += 1;

                // ── Volume trigger: fast entry ───────────────────────────
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
                    // Target: opposite edge with 5% margin
                    t.target_price = if dir_up { def.max * 0.95 } else { def.min * 1.05 };

                    info!("[FenixTrading] #{} {} ENTER {}@{:.4}>{:.4} (sgn={} vel={:.1} imb={:.2} bidV={:.0} askV={:.0} edge)",
                        session_id, def.name, if dir_up {"UP"} else {"DOWN"},
                        entry_price, t.target_price, fenix_signal,
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

        // Reset session balance to $20 per strategy
        for s in stats.iter_mut() {
            s.session_balance = 20.0;
            s.session_pnl = 0.0;
        }

        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let trade = &state.trades[i];
            if !trade.entered { continue; }

            // Use real exit PnL if already settled (target hit), else force-settle at current outcome
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

            // Per-session
            stats[i].session_pnl += pnl;
            stats[i].session_balance += pnl;
            // Cumulative
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

            info!("[FenixTrading] #{} {} SETTLED: {}@{}→{} pnl={:.4} bal=${:.2}|${:.2} {}",
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
