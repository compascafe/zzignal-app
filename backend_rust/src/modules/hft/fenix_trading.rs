//! Fenix Trading — Paper-trading simulation for mid-range strategies
//!
//! 5 strategies, $20 virtual capital each:
//!   Fenix 35-65: poly_mid [0.35, 0.65] → direction by majority (>0.5 = UP)
//!   Fenix 30-50: poly_mid [0.30, 0.50] → predicts DOWN
//!   Fenix 45-55: poly_mid [0.45, 0.55] → direction by majority
//!   Fenix 40-50: poly_mid [0.40, 0.50] → predicts DOWN
//!   Fenix 45-50: poly_mid [0.45, 0.50] → predicts DOWN
//!
//! Trading rules:
//!   - Enter when price stays in range for ≥ 3 consecutive ticks (confirmation)
//!   - UP positions: buy at ask, settle at 1.0 (win) or 0.0 (lose)
//!   - DOWN positions: sell at bid, settle at 0.0 (win) or 1.0 (lose)
//!   - Max 1 position per session per strategy
//!   - PnL tracked cumulatively

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use tracing::info;

// ─── Fenix Definitions ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct FenixDef {
    name:      &'static str,
    code:      &'static str,
    min:       f64,
    max:       f64,
    direction: &'static str, // "UP", "DOWN", "MAJORITY"
}

static FENIX_DEFS: &[FenixDef] = &[
    FenixDef { name: "Fenix 35-65", code: "fenix35", min: 0.35, max: 0.65, direction: "MAJORITY" },
    FenixDef { name: "Fenix 30-50", code: "fenix30", min: 0.30, max: 0.50, direction: "DOWN" },
    FenixDef { name: "Fenix 45-55", code: "fenix45", min: 0.45, max: 0.55, direction: "MAJORITY" },
    FenixDef { name: "Fenix 40-50", code: "fenix40", min: 0.40, max: 0.50, direction: "DOWN" },
    FenixDef { name: "Fenix 45-50", code: "fenix4550", min: 0.45, max: 0.50, direction: "DOWN" },
];

// ─── Per-Strategy Session State ────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct FenixSessionTrade {
    entered:         bool,
    direction_up:    bool,
    entry_price:     f64,
    ticks_in_range:  u32,
    settled:         bool,
    virtual_pnl:     f64,
    correct:         bool,
}

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FenixStats {
    pub name:           String,
    pub code:           String,
    pub range:          String,
    pub direction:      String,
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
            direction: def.direction.into(),
            capital: 20.0, balance: 20.0, trades: 0, wins: 0,
            accuracy: 0.0, total_pnl: 0.0, avg_pnl: 0.0,
            best_pnl: 0.0, worst_pnl: 0.0, sessions_tracked: 0,
            last_10: Vec::with_capacity(10),
        }
    }
}

// ─── Fenix Trading Manager ─────────────────────────────────────────────────

pub struct FenixTradingManager {
    /// Per-session state: session_id → [5 strategies]
    sessions: Mutex<HashMap<i32, Vec<FenixSessionTrade>>>,
    /// Cumulative stats per strategy
    stats:    Mutex<Vec<FenixStats>>,
}

impl FenixTradingManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            stats:    Mutex::new(FENIX_DEFS.iter().map(FenixStats::new).collect()),
        }
    }

    /// Called on every tick. Checks each Fenix strategy for entry conditions.
    pub fn on_tick(&self, session_id: i32, poly_mid: f64, poly_bid: f64, poly_ask: f64) -> Vec<(String, u8, f64)> {
        let mut sessions = self.sessions.lock().unwrap();
        let trades = sessions.entry(session_id).or_insert_with(|| {
            FENIX_DEFS.iter().map(|_| FenixSessionTrade::default()).collect()
        });

        let mut results = Vec::with_capacity(FENIX_DEFS.len());
        for (i, def) in FENIX_DEFS.iter().enumerate() {
            let in_range = poly_mid >= def.min && poly_mid <= def.max;
            if in_range {
                trades[i].ticks_in_range += 1;
            }

            // Entry condition: 3+ ticks in range, not yet entered
            if !trades[i].entered && trades[i].ticks_in_range >= 3 {
                let dir_up = match def.direction {
                    "UP"       => true,
                    "DOWN"     => false,
                    _          => poly_mid > 0.5,
                };
                trades[i].entered = true;
                trades[i].direction_up = dir_up;
                trades[i].entry_price = if dir_up { poly_ask } else { poly_bid };
                info!("[FenixTrading] Session #{} {} ENTER: {}@{:.4}",
                    session_id, def.name, if dir_up {"UP"} else {"DOWN"}, trades[i].entry_price);
            }

            let active = if trades[i].entered && !trades[i].settled { 1u8 } else { 0u8 };
            let entry = if active == 1 { trades[i].entry_price } else { 0.0 };
            results.push((def.code.to_string(), active, entry));
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

            info!("[FenixTrading] Session #{} {} SETTLED: {}@{:.4} → {} pnl={:.4} bal={:.2}",
                session_id, def.name,
                if trade.direction_up {"UP"} else {"DOWN"}, trade.entry_price,
                if correct {"✓"} else {"✗"}, pnl, stats[i].balance);
        }
        // Count session even for non-entered strategies
        for s in stats.iter_mut() {
            s.sessions_tracked += 1;
        }
    }

    pub fn export_json(&self) -> String {
        let stats = self.stats.lock().unwrap().clone();
        serde_json::to_string_pretty(&stats).unwrap_or_default()
    }

    pub fn accuracy(&self, idx: usize) -> f64 {
        self.stats.lock().unwrap().get(idx).map(|s| s.accuracy).unwrap_or(0.0)
    }

    pub fn best_strategy(&self) -> Option<(String, f64)> {
        let stats = self.stats.lock().unwrap();
        stats.iter()
            .filter(|s| s.trades > 0)
            .max_by(|a, b| a.total_pnl.partial_cmp(&b.total_pnl).unwrap_or(std::cmp::Ordering::Equal))
            .map(|s| (s.name.clone(), s.total_pnl))
    }
}
