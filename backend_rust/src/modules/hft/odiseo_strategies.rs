//! Odiseo Strategies v11 — Scalper Momentum (variante 2)
//!
//!   O83: entry>=0.83 tp=0.97 sl=0.81 trail=0.05
//!   H65: entry>=0.65 tp=0.75 sl=0.60 trail=0.04
//!   SCM: momentum_delta>=0.015 (2-tick) tp=+0.03 sl=-0.02 trail=0.01
//!
//! Exit reasons: 1=TP 2=SL_micro 3=SL_trend 4=SL_hard 5=trail 6=flash

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::Serialize;
use tracing::info;

use crate::modules::core::worker::{CmdMsg, OrderSide, Outcome as WorkerOutcome};
use crate::modules::hft::odiseo_filters::{FilterChain, FilterContext, FilterResult};

#[derive(Debug, Clone)]
struct OdiseoDef {
    name:             &'static str,
    code:             &'static str,
    entry_threshold:  f64,
    tp_price:         f64,
    sl_hard:          f64,
    sl_trend_delta:   f64,
    sl_micro_drop:    f64,
    only_last_10min:  bool,
    trail_distance:   f64,
    confirm_ticks:    u32,
    btc_trend_filter: bool,
    momentum_delta:   f64,   // 0=threshold entry, >0=scalp: min Δ in 2 ticks
}

static ODISEO_DEFS: &[OdiseoDef] = &[
    OdiseoDef { name:"Odiseo 83",   code:"odiseo83",  entry_threshold:0.83, tp_price:0.97, sl_hard:0.81, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false, trail_distance:0.05, confirm_ticks:1, btc_trend_filter:true, momentum_delta:0.0 },
    OdiseoDef { name:"Houdini 65", code:"houdini65", entry_threshold:0.65, tp_price:0.75, sl_hard:0.60, sl_trend_delta:0.02, sl_micro_drop:0.20, only_last_10min:false, trail_distance:0.04, confirm_ticks:1, btc_trend_filter:true, momentum_delta:0.0 },
    OdiseoDef { name:"Scalper M",  code:"scalper",   entry_threshold:0.30, tp_price:0.99, sl_hard:0.20, sl_trend_delta:0.02, sl_micro_drop:0.30, only_last_10min:false, trail_distance:0.01, confirm_ticks:1, btc_trend_filter:true, momentum_delta:0.015 },
];

#[derive(Debug, Clone)]
pub struct OdiseoPosition {
    pub entered: bool, pub entry_price: f64, pub size: f64, pub settled: bool,
    pub exit_price: f64, pub exit_reason: u8, pub virtual_pnl: f64,
    pub max_price: f64, pub min_price: f64, pub prev_vol: f64,
    pub confirm_count: u32,
    pub last_px: f64,
    pub prices: VecDeque<f64>,
    pub entry_seconds: i32,  // for Senna timeout
    pub signal_px: f64,      // price that triggered entry (before slippage)
}
impl Default for OdiseoPosition {
    fn default() -> Self {
        Self {
            entered: false, entry_price: 0.0, size: 0.0, settled: false,
            exit_price: 0.0, exit_reason: 0, virtual_pnl: 0.0,
            max_price: 0.0, min_price: 1.0, prev_vol: 0.0,
            confirm_count: 0,
            last_px: 0.0,
            prices: VecDeque::with_capacity(4),
            entry_seconds: 0,
            signal_px: 0.0,
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct OdiseoSessionTrade { pub up:OdiseoPosition, pub down:OdiseoPosition, pub sl_count:u32, pub session_profit:f64 }
pub struct OdiseoSessionState { pub trades:Vec<OdiseoSessionTrade> }

#[derive(Debug, Clone, Serialize)]
pub struct OdiseoStats {
    pub name:String, pub code:String, pub entry:f64, pub tp:f64, pub sl:f64, pub last10:bool,
    pub capital:f64, pub balance:f64, pub session_pnl:f64, pub session_balance:f64,
    pub trades_up:u64, pub wins_up:u64, pub tp_up:u64, pub sl_up:u64,
    pub trades_dn:u64, pub wins_dn:u64, pub tp_dn:u64, pub sl_dn:u64,
    pub accuracy:f64, pub total_pnl:f64, pub avg_pnl:f64, pub best:f64, pub worst:f64,
    pub sessions:u64, pub last_10:Vec<bool>,
}
impl OdiseoStats { fn new(d:&OdiseoDef)->Self { Self {
    name:d.name.into(), code:d.code.into(), entry:d.entry_threshold, tp:d.tp_price, sl:d.sl_hard, last10:d.only_last_10min,
    capital:20.0, balance:20.0, session_pnl:0.0, session_balance:20.0,
    trades_up:0,wins_up:0,tp_up:0,sl_up:0,trades_dn:0,wins_dn:0,tp_dn:0,sl_dn:0,
    accuracy:0.0,total_pnl:0.0,avg_pnl:0.0,best:0.0,worst:0.0,sessions:0,last_10:Vec::with_capacity(10),
}}}

/// Per-session Odiseo 83 snapshot for frontend performance tracking
#[derive(Debug, Clone, Serialize)]
pub struct OdiseoSessionSummary {
    pub session_id: i32,
    pub variant: String,
    pub pnl: f64,
    pub balance: f64,
    pub entries: u64,
    pub exits: u64,
    pub wins: u64,
    pub pnl_pct: f64,  // pnl as % of budget (default $20)
}

pub struct OdiseoTradingManager {
    pub sessions:   Mutex<HashMap<i32, OdiseoSessionState>>,
    stats:      Mutex<Vec<OdiseoStats>>,
    pub live_mode: AtomicBool,
    pub enabled: Vec<AtomicBool>,
    pub budgets: Mutex<Vec<f64>>,
    pub reinvest: AtomicBool,  // reinvest profits into next session
    pub max_sessions: Mutex<Vec<u32>>,  // 0 = unlimited, N = auto-off after N sessions
    sessions_done: Mutex<Vec<u32>>,     // completed session count per variant
    cmd_tx:     Option<tokio::sync::mpsc::UnboundedSender<CmdMsg>>,
    pub filter_chain: FilterChain,     // pre-entry filter layer
    session_history: Mutex<Vec<OdiseoSessionSummary>>, // per-session snapshots
    prev_snapshot: Mutex<HashMap<String, OdiseoStats>>,
    pub last_trigger: Mutex<u8>,  // 0=none, 1=CLOB momentum, 2=BTC big move
    pub last_clob_delta: Mutex<f64>,  // last computed CLOB price delta
    pub last_btc_vel: Mutex<f64>,     // BTC velocity at last momentum eval // previous stats for delta calc
}

impl OdiseoTradingManager {
    pub fn new(cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<CmdMsg>>) -> Self {
        let n = ODISEO_DEFS.len();
        let mut enabled = Vec::with_capacity(n);
        for _ in 0..n { enabled.push(AtomicBool::new(false)); }
        let budgets = vec![20.0; n];
        Self { sessions: Mutex::new(HashMap::new()), stats: Mutex::new(ODISEO_DEFS.iter().map(OdiseoStats::new).collect()), live_mode: AtomicBool::new(false), enabled, budgets: Mutex::new(budgets), reinvest: AtomicBool::new(true), max_sessions: Mutex::new(vec![0u32; n]), sessions_done: Mutex::new(vec![0u32; n]), cmd_tx, filter_chain: FilterChain::default_chain(), session_history: Mutex::new(Vec::new()), prev_snapshot: Mutex::new(HashMap::new()), last_trigger: Mutex::new(0), last_clob_delta: Mutex::new(0.0), last_btc_vel: Mutex::new(0.0) }
    }
    pub fn set_live_mode(&self, on:bool) { self.live_mode.store(on, Ordering::Relaxed); }
    pub fn set_variant(&self, idx:usize, on:bool) { if idx < self.enabled.len() { self.enabled[idx].store(on, Ordering::Relaxed); } }
    pub fn disable_all(&self) {
        self.live_mode.store(false, Ordering::Relaxed);
        for en in &self.enabled { en.store(false, Ordering::Relaxed); }
        // Clear all session state — prevents stale positions from re-triggering
        self.sessions.lock().unwrap().clear();
        info!("[Odiseo] ALL strategies DISABLED — live_mode OFF, all variants OFF, sessions cleared");
    }
    pub fn is_enabled(&self, idx:usize) -> bool { idx < self.enabled.len() && self.enabled[idx].load(Ordering::Relaxed) }
    pub fn set_budget(&self, idx:usize, amount:f64) { if let Some(b) = self.budgets.lock().unwrap().get_mut(idx) { *b = amount.max(5.0).min(100.0); } }
    pub fn get_budget(&self, idx:usize) -> f64 { self.budgets.lock().unwrap().get(idx).copied().unwrap_or(20.0) }
    pub fn set_reinvest(&self, on:bool) { self.reinvest.store(on, Ordering::Relaxed); }
    pub fn set_max_sessions(&self, idx:usize, max:u32) { if let Some(m) = self.max_sessions.lock().unwrap().get_mut(idx) { *m = max; } }
    pub fn get_max_sessions(&self, idx:usize) -> u32 { self.max_sessions.lock().unwrap().get(idx).copied().unwrap_or(0) }
    pub fn get_sessions_done(&self, idx:usize) -> u32 { self.sessions_done.lock().unwrap().get(idx).copied().unwrap_or(0) }

    pub fn on_tick(&self, session_id:i32, seconds_left:i32,
                   bid_vol:f64, ask_vol:f64, imb:f64, vel:f64,
                   lt_up:Option<f64>, lt_dn:Option<f64>,
                   ctx: &FilterContext,
    ) -> (Vec<(String,u8,f64,f64,f64,f64,u8,f64)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| OdiseoSessionState {
            trades: ODISEO_DEFS.iter().map(|_| OdiseoSessionTrade::default()).collect(),
        });
        let budgets = self.budgets.lock().unwrap().clone();
        let mut sig = 0u8;
        let mut results = Vec::with_capacity(24);
        let in_last_10 = seconds_left >= 0 && seconds_left <= 600;
        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            if !self.is_enabled(i) {
                results.push((format!("{}_up",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                results.push((format!("{}_down",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                continue;
            }
            if def.only_last_10min && !in_last_10 {
                results.push((format!("{}_up",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                results.push((format!("{}_down",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                continue;
            }
            self.process(true, &mut state.trades[i], def, session_id, bid_vol, ask_vol, imb, vel, lt_up, &mut sig, &mut results, budgets[i], seconds_left, ctx);
            self.process(false, &mut state.trades[i], def, session_id, bid_vol, ask_vol, imb, vel, lt_dn, &mut sig, &mut results, budgets[i], seconds_left, ctx);
        }
        (results, sig)
    }

    fn process(&self, is_up:bool, t:&mut OdiseoSessionTrade, def:&OdiseoDef, sid:i32,
               bv:f64, av:f64, imb:f64, vel:f64, lt:Option<f64>, sig:&mut u8,
               r:&mut Vec<(String,u8,f64,f64,f64,f64,u8,f64)>, budget:f64, seconds_left:i32,
               ctx: &FilterContext)
    {
        let code = format!("{}_{}", def.code, if is_up{"up"}else{"down"});
        let pos = if is_up {&mut t.up}else{&mut t.down};

        // ── Boundary safety: no trade in first 20s or last 60s ──
        let in_first = seconds_left > 880;
        let in_last = seconds_left <= 60;
        let boundary_block = in_first || in_last;
        let boundary_label = if in_first {"first20s"} else {"last60s"};

        // If position open and we're in boundary → force liquidate at market
        if pos.entered && !pos.settled && boundary_block {
            if let Some(px) = lt.filter(|&p| p > 0.0) {
                pos.settled = true; pos.exit_reason = 6; // 6 = flash_protection
                pos.exit_price = px;
                pos.virtual_pnl = (px - pos.entry_price) * pos.size;
                t.session_profit += pos.virtual_pnl;
                // NOT counted as SL — boundary protection is not a strategy failure
                info!("[Odiseo] #{} {} FLASH-PROTECT({}) EXIT @{:.4} pnl={:.4}", sid, code, boundary_label, px, pos.virtual_pnl);
                if self.live_mode.load(Ordering::Relaxed) {
                    if let Some(ref tx) = self.cmd_tx {
                        let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                        let _ = tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Sell, outcome, amount_usdc: pos.size.max(1.0) });
                    }
                }
                let bal = budget + pos.virtual_pnl;
                r.push((code.clone(), 0u8, pos.entry_price, pos.size, pos.virtual_pnl, px, 6u8, bal));
                *pos = OdiseoPosition::default();
                return;
            }
            // No price yet → keep position active but don't enter new logic
            r.push((code.clone(), 2u8, pos.entry_price, pos.size, 0.0, 0.0, 0u8, budget));
            return;
        }

        // If no position and we're in boundary → block new entries
        if !pos.entered && boundary_block {
            r.push((code, 1u8, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
            return;
        }

        // ── Límites de sesión ──
        let profit_limit = budget * 0.15; // 15% profit stop
        let sl_limit = 4u32;            // max 4 stop-losses per session
        if t.session_profit >= profit_limit {
            r.push((code,0,0.0,0.0,0.0,0.0,0,budget)); return;
        }
        if t.sl_count >= sl_limit {
            r.push((code,0,0.0,0.0,0.0,0.0,0,budget)); return;
        }

        let (px, px_fresh) = match lt { Some(p) if p>0.0 => (p, true), _ => {
            let raw = if is_up { ctx.raw_trade_up } else { ctx.raw_trade_dn };
            if raw > 0.0 {
                (raw, true)
            } else {
                let mid_alive = ctx.mid > 0.0 && (ctx.mid - 0.5).abs() > 0.01;
                if mid_alive {
                    (ctx.mid, true)
                } else if ctx.best_bid > 0.0 && ctx.best_bid < 1.0 {
                    (ctx.best_bid, true)
                } else {
                    // Per-side stored best bid (always fresh from AppState)
                    let side_bid = if is_up { ctx.best_bid_up } else { ctx.best_bid_dn };
                    if side_bid > 0.0 && side_bid < 1.0 {
                        (side_bid, true)
                    } else if pos.entered && !pos.settled && pos.last_px > 0.0 {
                        (pos.last_px, false)
                    } else if pos.entered && !pos.settled {
                        r.push((code.clone(), 2u8, pos.entry_price, pos.size, 0.0, 0.0, 0u8, budget));
                        return;
                    } else {
                        r.push((code,1,0.0,0.0,0.0,0.0,0,budget)); return;
                    }
                }
            }
        }};
        // Persist last known price (only from real data, not stale fallback)
        if px > 0.0 { pos.last_px = px; }

        if pos.entered && !pos.settled {
            // ── SENNA TIMEOUT: force market sell after 60s without exit ──
            if def.momentum_delta > 0.0 && pos.entry_seconds > 0 {
                let age = pos.entry_seconds - seconds_left;
                if age > 60 {
                    pos.settled = true; pos.exit_reason = 6;
                    pos.exit_price = px; pos.virtual_pnl = (px - pos.entry_price) * pos.size;
                    t.session_profit += pos.virtual_pnl;
                    info!("[Odiseo] #{} {} SENNA TIMEOUT @{:.4} pnl={:.4}", sid, code, px, pos.virtual_pnl);
                    if self.live_mode.load(Ordering::Relaxed) {
                        if let Some(ref tx) = self.cmd_tx {
                            let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                            let _ = tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Sell, outcome, amount_usdc: pos.size.max(1.0) });
                        }
                    }
                    let bal = budget + pos.virtual_pnl;
                    r.push((code.clone(), 0u8, pos.entry_price, pos.size, pos.virtual_pnl, px, 6u8, bal));
                    *pos = OdiseoPosition::default();
                    return;
                }
            }
            let reason = self.check_exit(pos, def, px, if is_up{av}else{bv}, imb, vel);
            if reason > 0 {
                pos.settled = true; pos.exit_reason = reason;
                let tp_price = if def.momentum_delta > 0.0 { pos.entry_price + 0.03 } else { def.tp_price };
                let fill = if reason==1 { tp_price } else { px };
                pos.exit_price = fill; pos.virtual_pnl = (fill - pos.entry_price) * pos.size;
                t.session_profit += pos.virtual_pnl;
                if reason >= 2 && reason <= 4 { t.sl_count += 1; } // only hard SL (2-4), trail(5)/flash(6) excluded
                info!("[Odiseo] #{} {} EXIT r={} @{:.4} pnl={:.4} sl_count={}", sid, code, reason, fill, pos.virtual_pnl, t.sl_count);
                // ── Live: place real exit order ──
                if self.live_mode.load(Ordering::Relaxed) {
                    if let Some(ref tx) = self.cmd_tx {
                        let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                        if reason == 1 {
                            let tp_price = if def.momentum_delta > 0.0 { pos.entry_price + 0.03 } else { def.tp_price };
                            let _ = tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Sell, outcome, price: tp_price, size: pos.size });
                        } else {
                            // Trail / SL / flash: market sell (immediate fill)
                            let _ = tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Sell, outcome, amount_usdc: pos.size.max(1.0) });
                        }
                    }
                }
            }
        }
        if pos.settled {
            let bal = budget + pos.virtual_pnl;
            r.push((code.clone(),0,pos.entry_price,pos.size,pos.virtual_pnl,pos.exit_price,pos.exit_reason,bal));
            *pos = OdiseoPosition::default();
            return; // PREVENT same-tick re-entry (Bug #1 fix — rapid sell/buy cycle)
        }

        if !pos.entered && px >= def.entry_threshold && px <= def.tp_price {
            // ── SCALP MODE: momentum entry (def.momentum_delta > 0) ──
            let scalp_trigger = if def.momentum_delta > 0.0 {
                pos.prices.push_back(px);
                if pos.prices.len() > 4 { pos.prices.pop_front(); }
                let gap = if pos.prices.len() >= 2 { px - pos.prices[0] } else { 0.0 };
                let clob_ok = gap >= def.momentum_delta;
                let btc_ok = if is_up { vel > 10.0 } else { vel < -10.0 };
                // Enter on CLOB momentum OR BTC big move + CLOB confirming
                *self.last_clob_delta.lock().unwrap() = gap;
                *self.last_btc_vel.lock().unwrap() = vel;
                if clob_ok || (btc_ok && px > pos.prices.get(0).copied().unwrap_or(0.0)) {
                    // Track which trigger fired
                    *self.last_trigger.lock().unwrap() = if clob_ok { 1 } else { 2 };
                    true
                } else {
                    r.push((code, 1u8, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                    return;
                }
            } else { false };
            // Clean up price window for non-scalp variants
            if !scalp_trigger { pos.prices.clear(); }

            // ── MOMENTUM FILTER: only on fresh price (skip stale, skip scalp) ──
            if !scalp_trigger && px_fresh && pos.last_px > 0.0 && px <= pos.last_px {
                pos.confirm_count = 0;
                r.push((code, 1u8, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                return;
            }
            // ── BTC MOMENTUM: block only if BTC moves hard against trade ──
            if def.btc_trend_filter {
                let btc_ok = if is_up { vel > -5.0 } else { vel < 5.0 };
                if !btc_ok {
                    pos.confirm_count = 0;
                    r.push((code, 1u8, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                    return;
                }
            }
            // ── ENTRY CONFIRMATION (skip for scalp with 1 tick) ──
            if def.confirm_ticks > 1 {
                pos.confirm_count = pos.confirm_count.saturating_add(1);
                if pos.confirm_count < def.confirm_ticks {
                    r.push((code, 1u8, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                    return;
                }
            }
            // ── PRE-ENTRY FILTER LAYER ──────────────────────────────
            {
                let mut fctx = ctx.clone();
                fctx.px = px;
                fctx.is_up = is_up;
                fctx.budget = budget;
                if let FilterResult::Block { reason } = self.filter_chain.check(&fctx, &code, sid) {
                    info!("[Odiseo] #{} {} FILTERED OUT: {}", sid, code, reason);
                    pos.confirm_count = 0;
                    let status = 1u8;
                    r.push((code, status, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                    return;
                }
            }
            // ─────────────────────────────────────────────────────────
            // Use px (signal price) as entry — best_ask can have wide spreads
            pos.entered = true; pos.entry_price = px;
            pos.signal_px = px;  // signal price (before slippage)
            pos.size = (budget/px).floor().max(1.0);
            pos.max_price = px; pos.min_price = px; pos.prev_vol = if is_up{av}else{bv};
            pos.confirm_count = 0;
            pos.entry_seconds = seconds_left;  // for Senna timeout
            *sig |= if is_up{1}else{2};
            info!("[Odiseo] #{} {} ENTER @{:.4} sz={:.0} trail={:.2}", sid, code, px, pos.size, def.trail_distance);
            // ── Live: place entry order ──
            if self.live_mode.load(Ordering::Relaxed) {
                if let Some(ref tx) = self.cmd_tx {
                    let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                    if def.momentum_delta > 0.0 {
                        // Scalp: limit buy at signal+0.02 (buys at right price, not worst ask)
                        let limit_price = (px + 0.02).min(0.99);
                        let _ = tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Buy, outcome, price: limit_price, size: pos.size });
                    } else {
                        // Threshold mode: limit buy at signal+0.02 to prevent slippage
                        let limit_price = (px + 0.02).min(def.tp_price - 0.01);
                        let _ = tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Buy, outcome, price: limit_price, size: pos.size });
                    }
                }
            }
        } else if !pos.entered && def.confirm_ticks > 1 {
            // Price dropped below threshold → reset confirmation
            pos.confirm_count = 0;
        }

        if pos.entered && !pos.settled {
            if px > pos.max_price { pos.max_price = px; }
            if px < pos.min_price { pos.min_price = px; }
            pos.prev_vol = if is_up{av}else{bv};
        }

        let status = if pos.entered && !pos.settled {2u8}else{1u8}; // 2=ACTIVE, 1=WATCHING
        let pnl = if status==2 {(px-pos.entry_price)*pos.size}else if pos.settled{pos.virtual_pnl}else{0.0};
        r.push((code, status, pos.entry_price, pos.size, pnl, if pos.settled{pos.exit_price}else{0.0}, if pos.settled{pos.exit_reason}else{0u8}, budget+pnl));
    }

    fn check_exit(&self, pos:&OdiseoPosition, def:&OdiseoDef, px:f64, vol:f64, imb:f64, vel:f64) -> u8 {
        let is_scalp = def.momentum_delta > 0.0;
        // ── TP (dynamic for scalp: f(x)=15-0.1x) ──
        let tp = if is_scalp {
            let cents = pos.entry_price * 100.0;
            let pct = ((15.0 - 0.1 * cents).max(3.0).min(12.0)) / 100.0;
            pos.entry_price * (1.0 + pct)
        } else { def.tp_price };
        if px >= tp { return 1; }
        // ── SL hard (60% of TP for scalp) ──
        let sl = if is_scalp {
            let cents = pos.entry_price * 100.0;
            let pct = ((15.0 - 0.1 * cents).max(3.0).min(12.0)) / 100.0;
            pos.entry_price * (1.0 - pct * 0.6)
        } else { def.sl_hard };
        if px <= sl { return 4; }
        // ── Trailing stop ──
        let trail = if is_scalp { 0.01 } else { def.trail_distance };
        if pos.max_price > 0.0 && trail > 0.0 && px <= pos.max_price - trail {
            return 5;
        }
        // ── SL micro (skip for scalp — too sensitive) ──
        if !is_scalp && pos.prev_vol>0.0 && (pos.prev_vol-vol)/pos.prev_vol > def.sl_micro_drop && imb < -0.5 { return 2; }
        // ── SL trend ──
        if pos.max_price - px > def.sl_trend_delta && vel < 0.0 { return 3; }
        0
    }

    pub fn on_session_close(&self, sid:i32, outcome:&str) {
        let au=outcome.eq_ignore_ascii_case("up"); let ad=outcome.eq_ignore_ascii_case("down"); let tie=!au&&!ad;
        let mut sessions=self.sessions.lock().unwrap();
        let state=match sessions.remove(&sid){Some(s)=>s,None=>return};
        let mut stats=self.stats.lock().unwrap();
        for s in stats.iter_mut(){s.session_balance=20.0;s.session_pnl=0.0;}
        for(i,def) in ODISEO_DEFS.iter().enumerate() {
            let mut pnl=0.0;
            pnl+=self.settle(&state.trades[i].up,true,au,tie,&mut stats[i],sid,def.name,"UP");
            pnl+=self.settle(&state.trades[i].down,false,ad,tie,&mut stats[i],sid,def.name,"DOWN");
            stats[i].session_pnl+=pnl;stats[i].session_balance+=pnl;stats[i].balance+=pnl;
            if self.reinvest.load(Ordering::Relaxed) { let mut bd = self.budgets.lock().unwrap(); if let Some(b) = bd.get_mut(i) { if pnl > 0.0 { *b = (*b + pnl).min(100.0); } } }
            stats[i].accuracy=if stats[i].trades_up+stats[i].trades_dn>0{(stats[i].wins_up+stats[i].wins_dn)as f64/(stats[i].trades_up+stats[i].trades_dn)as f64}else{0.0};

            // Auto-disable after N sessions
            let mut sd = self.sessions_done.lock().unwrap();
            if let Some(done) = sd.get_mut(i) {
                *done += 1;
                let max = self.max_sessions.lock().unwrap().get(i).copied().unwrap_or(0);
                if max > 0 && *done >= max {
                    self.set_variant(i, false);
                    info!("[Odiseo] {} auto-disabled after {} sessions", ODISEO_DEFS[i].name, max);
                }
            }
        }
        for s in stats.iter_mut(){s.sessions+=1;}

        // Snapshot Odiseo 83 performance for this session
        self.snapshot_odiseo83(sid, &stats);
    }

    fn snapshot_odiseo83(&self, session_id: i32, stats: &[OdiseoStats]) {
        let mut history = self.session_history.lock().unwrap();
        let mut prev = self.prev_snapshot.lock().unwrap();

        // Only track Odiseo 83 variant (index 0)
        if let Some(ods) = stats.first() {
            let budget = self.get_budget(0);
            let prev_stats = prev.get("odiseo83");

            let (entries_delta, exits_delta, wins_delta, pnl_delta, balance) = if let Some(ps) = prev_stats {
                let ed = ods.trades_up + ods.trades_dn - ps.trades_up - ps.trades_dn;
                let xd = (ods.tp_up + ods.tp_dn + ods.sl_up + ods.sl_dn) - (ps.tp_up + ps.tp_dn + ps.sl_up + ps.sl_dn);
                let wd = ods.wins_up + ods.wins_dn - ps.wins_up - ps.wins_dn;
                let pd = ods.total_pnl - ps.total_pnl;
                (ed, xd, wd, pd, ods.balance)
            } else {
                (ods.trades_up + ods.trades_dn,
                 ods.tp_up + ods.tp_dn + ods.sl_up + ods.sl_dn,
                 ods.wins_up + ods.wins_dn,
                 ods.total_pnl,
                 ods.balance)
            };

            let summary = OdiseoSessionSummary {
                session_id,
                variant: "odiseo83".into(),
                pnl: pnl_delta,
                balance,
                entries: entries_delta,
                exits: exits_delta,
                wins: wins_delta,
                pnl_pct: if budget > 0.0 { (pnl_delta / budget) * 100.0 } else { 0.0 },
            };

            prev.insert("odiseo83".into(), ods.clone());
            history.push(summary);
            if history.len() > 50 { history.remove(0); } // keep last 50
        }
    }

    fn settle(&self, pos:&OdiseoPosition, up:bool, outcome_up:bool, tie:bool, s:&mut OdiseoStats, sid:i32, name:&str, dir:&str) -> f64 {
        if !pos.entered { return 0.0; }
        let pnl = if pos.settled { pos.virtual_pnl }
        else if tie { 0.0 }
        else { let fp=if(up&&outcome_up)||(!up&&!outcome_up){1.0}else{0.0}; (fp-pos.entry_price)*pos.size };
        let ok=pnl>0.0;
        if up { s.trades_up+=1;if ok{s.wins_up+=1;} match pos.exit_reason{1=>s.tp_up+=1,2|3|4=>s.sl_up+=1,_=>{}} }
        else { s.trades_dn+=1;if ok{s.wins_dn+=1;} match pos.exit_reason{1=>s.tp_dn+=1,2|3|4=>s.sl_dn+=1,_=>{}} }
        s.total_pnl+=pnl;s.last_10.push(ok);if s.last_10.len()>10{s.last_10.remove(0);}
        info!("[Odiseo] #{} {}_{} SETTLED e={:.4} sz={:.0} x={:.4} r={} pnl={:.4}", sid, name, dir, pos.entry_price, pos.size, if pos.settled{pos.exit_price}else{if outcome_up{1.0}else{0.0}}, if pos.settled{pos.exit_reason}else{5}, pnl);
        pnl
    }

    pub fn export_json(&self) -> String { serde_json::to_string_pretty(&*self.stats.lock().unwrap()).unwrap_or_default() }

    pub fn get_total_pnl(&self, code: &str) -> f64 {
        self.stats.lock().unwrap().iter()
            .find(|s| s.code == code)
            .map(|s| s.total_pnl)
            .unwrap_or(0.0)
    }

    pub fn session_summaries(&self) -> Vec<OdiseoSessionSummary> {
        self.session_history.lock().unwrap().clone()
    }
}
