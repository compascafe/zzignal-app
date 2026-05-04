//! Odiseo Strategies v3 — Limit-order logic with whale-jump protection
//!
//! Entry: best_ask >= entry_threshold → PLACE LIMIT BUY at tp_price.
//!        Only fills if best_ask <= tp_price (protection against whale jumps).
//! TP:    best_bid >= tp_price → LIMIT SELL at tp_price (take profit).
//! SL:    3-layer stop loss, last layer = MARKET SELL at any price.
//!
//!   Layer 1 (TP): best_bid >= tp_price → exit with profit
//!   Layer 2 (Micro): volume crash >30% + imbalance inversion → market exit
//!   Layer 3 (Trend): best_bid drops -0.03 from post-entry max + velocity < 0 → market exit
//!   Layer 4 (Hard): best_bid <= sl_hard → market exit
//!
//! Both UP and DOWN use the same symmetric logic (LONG on respective token).
//! 3 variants × 2 directions. $20 virtual each. One entry per session per direction.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tracing::info;

// ─── Odiseo Definitions ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct OdiseoDef {
    name:            &'static str,
    code:            &'static str,
    entry_threshold: f64,    // best_ask >= this → consider entry
    tp_price:        f64,    // limit buy price = limit sell price = take-profit
    sl_hard:         f64,    // best_bid <= this → hard stop (market sell)
    sl_trend_delta:  f64,    // best_bid drop from post-entry max for trend SL
    sl_micro_drop:   f64,    // volume % drop for microstructure SL
}

static ODISEO_DEFS: &[OdiseoDef] = &[
    // entry=0.90: limit buy @ 0.985, SL hard @ 0.84
    OdiseoDef { name: "Odiseo 90", code: "odiseo90", entry_threshold: 0.90, tp_price: 0.985, sl_hard: 0.84, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
    // entry=0.93: limit buy @ 0.985, SL hard @ 0.87
    OdiseoDef { name: "Odiseo 93", code: "odiseo93", entry_threshold: 0.93, tp_price: 0.985, sl_hard: 0.87, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
    // entry=0.95: limit buy @ 0.990, SL hard @ 0.90
    OdiseoDef { name: "Odiseo 95", code: "odiseo95", entry_threshold: 0.95, tp_price: 0.990, sl_hard: 0.90, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
];

// ─── Per-Strategy Session State ────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct OdiseoPosition {
    entered:         bool,
    entry_price:     f64,     // best_ask at entry (what we paid)
    size:            f64,     // contracts = $20 / entry_price
    settled:         bool,
    exit_price:      f64,
    exit_reason:     u8,      // 0=none, 1=TP, 2=SL-micro, 3=SL-trend, 4=SL-hard
    virtual_pnl:     f64,
    max_bid:         f64,     // post-entry high of best_bid (for trend SL)
    prev_vol:        f64,     // previous volume for micro SL
}

#[derive(Debug, Clone, Default)]
struct OdiseoSessionTrade {
    up:   OdiseoPosition,
    down: OdiseoPosition,
}

struct OdiseoSessionState {
    trades: Vec<OdiseoSessionTrade>,
}

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct OdiseoStats {
    pub name:            String,
    pub code:            String,
    pub entry_threshold: f64,
    pub tp_price:        f64,
    pub sl_hard:         f64,
    pub capital:         f64,
    pub balance:         f64,
    pub session_pnl:     f64,
    pub session_balance: f64,
    pub trades_up:       u64,
    pub wins_up:         u64,
    pub tp_exits_up:     u64,
    pub sl_exits_up:     u64,
    pub trades_down:     u64,
    pub wins_down:       u64,
    pub tp_exits_down:   u64,
    pub sl_exits_down:   u64,
    pub accuracy:        f64,
    pub total_pnl:       f64,
    pub avg_pnl:         f64,
    pub best_pnl:        f64,
    pub worst_pnl:       f64,
    pub sessions_tracked: u64,
    pub last_10:         Vec<bool>,
}

impl OdiseoStats {
    fn new(def: &OdiseoDef) -> Self {
        Self {
            name: def.name.into(), code: def.code.into(),
            entry_threshold: def.entry_threshold, tp_price: def.tp_price, sl_hard: def.sl_hard,
            capital: 20.0, balance: 20.0, session_pnl: 0.0, session_balance: 20.0,
            trades_up: 0, wins_up: 0, tp_exits_up: 0, sl_exits_up: 0,
            trades_down: 0, wins_down: 0, tp_exits_down: 0, sl_exits_down: 0,
            accuracy: 0.0, total_pnl: 0.0, avg_pnl: 0.0,
            best_pnl: 0.0, worst_pnl: 0.0, sessions_tracked: 0,
            last_10: Vec::with_capacity(10),
        }
    }
}

// ─── Odiseo Trading Manager ────────────────────────────────────────────────

pub struct OdiseoTradingManager {
    sessions: Mutex<HashMap<i32, OdiseoSessionState>>,
    stats:    Mutex<Vec<OdiseoStats>>,
}

impl OdiseoTradingManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            stats:    Mutex::new(ODISEO_DEFS.iter().map(OdiseoStats::new).collect()),
        }
    }

    /// Entry uses best_ask (what we PAY). Exit uses best_bid (what we GET).
    /// Limit buy at tp_price: only enters if best_ask is between entry_threshold and tp_price.
    /// Returns: (strategy_trades, odiseo_signal)
    /// signal: 1=UP entry, 2=DOWN entry, 3=both
    pub fn on_tick(&self, session_id: i32,
                   best_bid: f64, best_ask: f64,
                   bid_vol: f64, ask_vol: f64,
                   poly_imbalance: f64, price_velocity: f64,
    ) -> (Vec<(String, u8, f64, f64, f64, f64, u8, f64)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| OdiseoSessionState {
            trades: ODISEO_DEFS.iter().map(|_| OdiseoSessionTrade::default()).collect(),
        });

        let mut odiseo_signal = 0u8;
        let mut results = Vec::with_capacity(ODISEO_DEFS.len() * 2);

        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            let t = &mut state.trades[i];

            self.process_direction(true, t, def, session_id,
                best_bid, best_ask, bid_vol, ask_vol,
                poly_imbalance, price_velocity,
                &mut odiseo_signal, &mut results);

            self.process_direction(false, t, def, session_id,
                best_bid, best_ask, bid_vol, ask_vol,
                poly_imbalance, price_velocity,
                &mut odiseo_signal, &mut results);
        }
        (results, odiseo_signal)
    }

    #[allow(clippy::too_many_arguments)]
    fn process_direction(&self, is_up: bool, t: &mut OdiseoSessionTrade,
                         def: &OdiseoDef, session_id: i32,
                         best_bid: f64, best_ask: f64,
                         bid_vol: f64, ask_vol: f64,
                         poly_imbalance: f64, price_velocity: f64,
                         signal: &mut u8,
                         results: &mut Vec<(String, u8, f64, f64, f64, f64, u8, f64)>)
    {
        let dir_label = if is_up { "up" } else { "down" };
        let pos = if is_up { &mut t.up } else { &mut t.down };
        let code = format!("{}_{}", def.code, dir_label);

        // Market must have both sides to be tradeable
        if best_bid <= 0.0 && best_ask <= 0.0 {
            results.push((code, 0u8, 0.0, 0.0, 0.0, 0.0, 0u8, 20.0));
            return;
        }

        // ── Exit checks (only if entered and not settled) ───────────────
        if pos.entered && !pos.settled {
            let exit_reason = self.check_exit(pos, def, best_bid,
                if is_up { ask_vol } else { bid_vol },
                poly_imbalance, price_velocity);

            if exit_reason > 0 {
                pos.settled = true;
                pos.exit_reason = exit_reason;
                // Exit fill: if TP (reason=1), we get tp_price (limit sell filled).
                // If SL (reason≥2), we get best_bid (market sell at whatever bid is available).
                let exit_fill = if exit_reason == 1 { def.tp_price } else { best_bid };
                pos.exit_price = exit_fill;
                pos.virtual_pnl = (exit_fill - pos.entry_price) * pos.size;
                info!("[Odiseo] #{} {} EXIT reason={} @ {:.4} entry={:.4} pnl={:.4} sz={:.0}",
                    session_id, code, exit_reason, exit_fill, pos.entry_price, pos.virtual_pnl, pos.size);
            }
        }

        if pos.settled {
            let balance = 20.0 + pos.virtual_pnl;
            results.push((code, 0u8, pos.entry_price, pos.size, pos.virtual_pnl,
                pos.exit_price, pos.exit_reason, balance));
            return;
        }

        // ── Entry check: limit buy at tp_price ─────────────────────────
        // Only enter if best_ask is between entry_threshold AND tp_price.
        // If ask jumped above tp_price (whale), limit wouldn't fill → no entry.
        if !pos.entered && best_ask >= def.entry_threshold && best_ask <= def.tp_price {
            pos.entered = true;
            pos.entry_price = best_ask;
            pos.size = (20.0 / pos.entry_price).floor().max(1.0);
            pos.max_bid = best_bid;
            pos.prev_vol = if is_up { ask_vol } else { bid_vol };

            if is_up { *signal |= 1; } else { *signal |= 2; }

            info!("[Odiseo] #{} {} ENTER {} @ {:.4} sz={:.0} tp={:.4} bid={:.4} ask={:.4}",
                session_id, code, if is_up {"UP"} else {"DOWN"},
                pos.entry_price, pos.size, def.tp_price, best_bid, best_ask);
        }

        // ── Update tracking for active positions ───────────────────────
        if pos.entered && !pos.settled {
            if best_bid > pos.max_bid { pos.max_bid = best_bid; }
            pos.prev_vol = if is_up { ask_vol } else { bid_vol };
        }

        // ── Live PnL + balance ─────────────────────────────────────────
        let active = if pos.entered && !pos.settled { 1u8 } else { 0u8 };
        let live_pnl = if active == 1 {
            // Mark-to-market at best_bid (what we'd get if we sold now)
            (best_bid - pos.entry_price) * pos.size
        } else if pos.settled {
            pos.virtual_pnl
        } else {
            0.0
        };
        let balance = 20.0 + live_pnl;

        results.push((code, active, pos.entry_price, pos.size, live_pnl,
            if pos.settled { pos.exit_price } else { 0.0 },
            if pos.settled { pos.exit_reason } else { 0u8 },
            balance));
    }

    /// Check all exit layers against best_bid (what we'd GET when selling).
    fn check_exit(&self, pos: &OdiseoPosition, def: &OdiseoDef,
                  best_bid: f64, vol: f64,
                  poly_imbalance: f64, price_velocity: f64,
    ) -> u8 {
        // Layer 1: Limit sell at TP — best_bid >= tp_price means our limit WOULD fill
        if best_bid >= def.tp_price { return 1; }

        // Layer 2: Microstructure — volume crash + imbalance inversion
        if pos.prev_vol > 0.0 {
            let vol_drop = (pos.prev_vol - vol) / pos.prev_vol;
            if vol_drop > def.sl_micro_drop && poly_imbalance < -0.5 { return 2; }
        }

        // Layer 3: Trend reversal — best_bid dropped from max + BTC velocity against
        let bid_drop = pos.max_bid - best_bid;
        if bid_drop > def.sl_trend_delta && price_velocity < 0.0 { return 3; }

        // Layer 4: Hard stop — market sell at whatever bid
        if best_bid <= def.sl_hard { return 4; }

        0
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
        for s in stats.iter_mut() {
            s.session_balance = 20.0;
            s.session_pnl = 0.0;
        }

        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            let trade = &state.trades[i];
            let mut total_session_pnl = 0.0;

            total_session_pnl += self.settle_position(&trade.up, true, actual_up, is_tie,
                &mut stats[i], session_id, def.name, "UP");
            total_session_pnl += self.settle_position(&trade.down, false, actual_down, is_tie,
                &mut stats[i], session_id, def.name, "DOWN");

            stats[i].session_pnl += total_session_pnl;
            stats[i].session_balance += total_session_pnl;
            stats[i].balance += total_session_pnl;
            stats[i].accuracy = if stats[i].trades_up + stats[i].trades_down > 0 {
                (stats[i].wins_up + stats[i].wins_down) as f64 /
                (stats[i].trades_up + stats[i].trades_down) as f64
            } else { 0.0 };
        }
        for s in stats.iter_mut() { s.sessions_tracked += 1; }
    }

    fn settle_position(&self, pos: &OdiseoPosition, is_up: bool,
                       outcome_up: bool, is_tie: bool,
                       stats: &mut OdiseoStats,
                       session_id: i32, name: &str, dir: &str) -> f64
    {
        if !pos.entered { return 0.0; }

        let pnl = if pos.settled {
            pos.virtual_pnl
        } else if is_tie {
            0.0
        } else {
            let final_price = if (is_up && outcome_up) || (!is_up && !outcome_up) { 1.0 } else { 0.0 };
            (final_price - pos.entry_price) * pos.size
        };

        let correct = pnl > 0.0;

        if is_up {
            stats.trades_up += 1;
            if correct { stats.wins_up += 1; }
            match pos.exit_reason { 1 => stats.tp_exits_up += 1, 2|3|4 => stats.sl_exits_up += 1, _ => {} }
        } else {
            stats.trades_down += 1;
            if correct { stats.wins_down += 1; }
            match pos.exit_reason { 1 => stats.tp_exits_down += 1, 2|3|4 => stats.sl_exits_down += 1, _ => {} }
        }
        stats.total_pnl += pnl;
        stats.last_10.push(correct);
        if stats.last_10.len() > 10 { stats.last_10.remove(0); }

        info!("[Odiseo] #{} {}_{} SETTLED: entry={:.4} sz={:.0} exit={:.4} reason={} pnl={:.4} {}",
            session_id, name, dir, pos.entry_price, pos.size,
            if pos.settled { pos.exit_price } else { if outcome_up { 1.0 } else { 0.0 } },
            if pos.settled { pos.exit_reason } else { 5u8 },
            pnl, if pos.settled { "[exit hit]" } else { "[session end]" });

        pnl
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&*self.stats.lock().unwrap()).unwrap_or_default()
    }
}
