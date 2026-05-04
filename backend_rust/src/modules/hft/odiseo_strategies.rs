//! Odiseo Strategies v4 — Last-trade-price with anti-whale limit-buy protection
//!
//! Entry: last_trade_up >= entry_threshold AND <= tp_price → BUY at last_trade_up
//!        last_trade_down >= entry_threshold AND <= tp_price → BUY at last_trade_down
//!        If last_trade jumps above tp_price (whale): limit wouldn't fill → NO entry.
//! TP:    last_trade >= tp_price → exit
//! SL:    3-layer stop loss
//!
//! Both UP and DOWN symmetric. 3 variants × 2 directions. $20 virtual each.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tracing::info;

#[derive(Debug, Clone)]
struct OdiseoDef {
    name:            &'static str,
    code:            &'static str,
    entry_threshold: f64,
    tp_price:        f64,
    sl_hard:         f64,
    sl_trend_delta:  f64,
    sl_micro_drop:   f64,
}

static ODISEO_DEFS: &[OdiseoDef] = &[
    OdiseoDef { name: "Odiseo 90", code: "odiseo90", entry_threshold: 0.90, tp_price: 0.985, sl_hard: 0.84, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
    OdiseoDef { name: "Odiseo 93", code: "odiseo93", entry_threshold: 0.93, tp_price: 0.985, sl_hard: 0.87, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
    OdiseoDef { name: "Odiseo 95", code: "odiseo95", entry_threshold: 0.95, tp_price: 0.990, sl_hard: 0.90, sl_trend_delta: 0.03, sl_micro_drop: 0.30 },
];

#[derive(Debug, Clone, Default)]
struct OdiseoPosition {
    entered:         bool,
    entry_price:     f64,
    size:            f64,
    settled:         bool,
    exit_price:      f64,
    exit_reason:     u8,
    virtual_pnl:     f64,
    max_price:       f64,
    prev_vol:        f64,
}

#[derive(Debug, Clone, Default)]
struct OdiseoSessionTrade { up: OdiseoPosition, down: OdiseoPosition }

struct OdiseoSessionState { trades: Vec<OdiseoSessionTrade> }

#[derive(Debug, Clone, Serialize)]
pub struct OdiseoStats {
    pub name: String, pub code: String, pub entry_threshold: f64, pub tp_price: f64, pub sl_hard: f64,
    pub capital: f64, pub balance: f64, pub session_pnl: f64, pub session_balance: f64,
    pub trades_up: u64, pub wins_up: u64, pub tp_exits_up: u64, pub sl_exits_up: u64,
    pub trades_down: u64, pub wins_down: u64, pub tp_exits_down: u64, pub sl_exits_down: u64,
    pub accuracy: f64, pub total_pnl: f64, pub avg_pnl: f64, pub best_pnl: f64, pub worst_pnl: f64,
    pub sessions_tracked: u64, pub last_10: Vec<bool>,
}

impl OdiseoStats {
    fn new(def: &OdiseoDef) -> Self { Self {
        name: def.name.into(), code: def.code.into(),
        entry_threshold: def.entry_threshold, tp_price: def.tp_price, sl_hard: def.sl_hard,
        capital: 20.0, balance: 20.0, session_pnl: 0.0, session_balance: 20.0,
        trades_up:0,wins_up:0,tp_exits_up:0,sl_exits_up:0,trades_down:0,wins_down:0,tp_exits_down:0,sl_exits_down:0,
        accuracy:0.0,total_pnl:0.0,avg_pnl:0.0,best_pnl:0.0,worst_pnl:0.0,sessions_tracked:0,
        last_10: Vec::with_capacity(10),
    }}
}

pub struct OdiseoTradingManager {
    sessions: Mutex<HashMap<i32, OdiseoSessionState>>,
    stats:    Mutex<Vec<OdiseoStats>>,
}

impl OdiseoTradingManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()), stats: Mutex::new(ODISEO_DEFS.iter().map(OdiseoStats::new).collect()) }
    }

    pub fn on_tick(&self, session_id: i32,
                   poly_bid_vol_all: f64, poly_ask_vol_all: f64,
                   poly_imbalance: f64, price_velocity: f64,
                   last_trade_up: Option<f64>, last_trade_down: Option<f64>,
    ) -> (Vec<(String, u8, f64, f64, f64, f64, u8, f64)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| OdiseoSessionState {
            trades: ODISEO_DEFS.iter().map(|_| OdiseoSessionTrade::default()).collect(),
        });
        let mut sig = 0u8;
        let mut results = Vec::with_capacity(6);
        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            self.process(true, &mut state.trades[i], def, session_id, poly_bid_vol_all, poly_ask_vol_all,
                poly_imbalance, price_velocity, last_trade_up, &mut sig, &mut results);
            self.process(false, &mut state.trades[i], def, session_id, poly_bid_vol_all, poly_ask_vol_all,
                poly_imbalance, price_velocity, last_trade_down, &mut sig, &mut results);
        }
        (results, sig)
    }

    fn process(&self, is_up: bool, t: &mut OdiseoSessionTrade, def: &OdiseoDef,
               session_id: i32, bid_vol: f64, ask_vol: f64, imb: f64, vel: f64,
               lt: Option<f64>, sig: &mut u8,
               results: &mut Vec<(String, u8, f64, f64, f64, f64, u8, f64)>)
    {
        let code = format!("{}_{}", def.code, if is_up { "up" } else { "down" });
        let pos = if is_up { &mut t.up } else { &mut t.down };
        let trade_price = match lt { Some(p) if p > 0.0 => p, _ => { results.push((code,0,0.0,0.0,0.0,0.0,0,20.0)); return; } };

        // ── Exit ──
        if pos.entered && !pos.settled {
            let r = self.check_exit(pos, def, trade_price, if is_up { ask_vol } else { bid_vol }, imb, vel);
            if r > 0 {
                pos.settled = true; pos.exit_reason = r;
                let fill = if r == 1 { def.tp_price } else { trade_price };
                pos.exit_price = fill; pos.virtual_pnl = (fill - pos.entry_price) * pos.size;
                info!("[Odiseo] #{} {} EXIT r={} @{:.4} pnl={:.4}", session_id, code, r, fill, pos.virtual_pnl);
            }
        }
        if pos.settled { results.push((code,0,pos.entry_price,pos.size,pos.virtual_pnl,pos.exit_price,pos.exit_reason,20.0+pos.virtual_pnl)); return; }

        // ── Entry: anti-whale limit-buy ──
        if !pos.entered && trade_price >= def.entry_threshold && trade_price <= def.tp_price {
            pos.entered = true; pos.entry_price = trade_price;
            pos.size = (20.0 / trade_price).floor().max(1.0);
            pos.max_price = trade_price; pos.prev_vol = if is_up { ask_vol } else { bid_vol };
            *sig |= if is_up { 1 } else { 2 };
            info!("[Odiseo] #{} {} ENTER @{:.4} sz={:.0}", session_id, code, trade_price, pos.size);
        }

        // ── Track ──
        if pos.entered && !pos.settled {
            if trade_price > pos.max_price { pos.max_price = trade_price; }
            pos.prev_vol = if is_up { ask_vol } else { bid_vol };
        }

        let active = if pos.entered && !pos.settled { 1u8 } else { 0u8 };
        let pnl = if active == 1 { (trade_price - pos.entry_price) * pos.size } else if pos.settled { pos.virtual_pnl } else { 0.0 };
        results.push((code, active, pos.entry_price, pos.size, pnl,
            if pos.settled { pos.exit_price } else { 0.0 }, if pos.settled { pos.exit_reason } else { 0u8 }, 20.0 + pnl));
    }

    fn check_exit(&self, pos: &OdiseoPosition, def: &OdiseoDef, price: f64, vol: f64, imb: f64, vel: f64) -> u8 {
        if price >= def.tp_price { return 1; }
        if pos.prev_vol > 0.0 && (pos.prev_vol - vol) / pos.prev_vol > def.sl_micro_drop && imb < -0.5 { return 2; }
        if pos.max_price - price > def.sl_trend_delta && vel < 0.0 { return 3; }
        if price <= def.sl_hard { return 4; }
        0
    }

    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str) {
        let au = actual_outcome.eq_ignore_ascii_case("up");
        let ad = actual_outcome.eq_ignore_ascii_case("down");
        let tie = !au && !ad;
        let mut sessions = self.sessions.lock().unwrap();
        let state = match sessions.remove(&session_id) { Some(s) => s, None => return };
        let mut stats = self.stats.lock().unwrap();
        for s in stats.iter_mut() { s.session_balance = 20.0; s.session_pnl = 0.0; }
        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            let mut pnl = 0.0;
            pnl += self.settle(&state.trades[i].up, true, au, tie, &mut stats[i], session_id, def.name, "UP");
            pnl += self.settle(&state.trades[i].down, false, ad, tie, &mut stats[i], session_id, def.name, "DOWN");
            stats[i].session_pnl += pnl; stats[i].session_balance += pnl; stats[i].balance += pnl;
            stats[i].accuracy = if stats[i].trades_up+stats[i].trades_down > 0 {
                (stats[i].wins_up+stats[i].wins_down) as f64/(stats[i].trades_up+stats[i].trades_down) as f64
            } else { 0.0 };
        }
        for s in stats.iter_mut() { s.sessions_tracked += 1; }
    }

    fn settle(&self, pos: &OdiseoPosition, up: bool, outcome_up: bool, tie: bool, s: &mut OdiseoStats, sid: i32, name: &str, dir: &str) -> f64 {
        if !pos.entered { return 0.0; }
        let pnl = if pos.settled { pos.virtual_pnl }
        else if tie { 0.0 }
        else { let fp = if (up && outcome_up) || (!up && !outcome_up) { 1.0 } else { 0.0 }; (fp - pos.entry_price) * pos.size };
        let ok = pnl > 0.0;
        if up { s.trades_up += 1; if ok { s.wins_up += 1; } match pos.exit_reason { 1 => s.tp_exits_up += 1, 2|3|4 => s.sl_exits_up += 1, _ => {} } }
        else { s.trades_down += 1; if ok { s.wins_down += 1; } match pos.exit_reason { 1 => s.tp_exits_down += 1, 2|3|4 => s.sl_exits_down += 1, _ => {} } }
        s.total_pnl += pnl; s.last_10.push(ok); if s.last_10.len() > 10 { s.last_10.remove(0); }
        info!("[Odiseo] #{} {}_{} SETTLED: e={:.4} sz={:.0} x={:.4} r={} pnl={:.4}", sid, name, dir, pos.entry_price, pos.size,
            if pos.settled { pos.exit_price } else { if outcome_up { 1.0 } else { 0.0 } },
            if pos.settled { pos.exit_reason } else { 5 }, pnl);
        pnl
    }

    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&*self.stats.lock().unwrap()).unwrap_or_default()
    }
}
