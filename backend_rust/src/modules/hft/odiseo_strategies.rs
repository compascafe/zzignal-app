//! Odiseo Strategies v6 — 13 variants, TP=0.97, session boundary protection
//!
//! Variants:
//!   83: entry>=0.83 tp=0.97 sl=0.81 (PRINCIPAL, full 15min)
//!   65: entry>=0.65 tp=0.95 sl=0.63 (Wide 65 reversals, full 15min)
//!   86-93: entry>=0.86-0.93 tp=0.97 (full 15min)
//!   94:  entry>=0.94 tp=0.97 sl=0.92 (only last 10min)
//!   95:  entry>=0.95 tp=0.97 sl=0.93 (full 15min)
//!   96:  entry>=0.96 tp=0.985 sl=0.95 (only last 10min)
//!
//! Anti-whale: entry only if price between entry_threshold and tp_price.
//! 3-layer SL. $20 per variant per direction. Cumulative + per-session reset.
//!
//! Session boundary protection (v6):
//!   - First 20s (seconds_left > 880): NO entries, liquidate open positions.
//!   - Last 20s  (seconds_left <= 20): NO entries, liquidate open positions.
//!     (45s necesario: en sesion #895 el mercado murio a 59:28, 0 ticks 59:40-59:50).
//!   - Flash-protection exit = exit_reason 6 (market sell, NOT counted as SL).

use std::collections::HashMap;
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
}

static ODISEO_DEFS: &[OdiseoDef] = &[
    OdiseoDef { name:"Odiseo 83", code:"odiseo83", entry_threshold:0.83, tp_price:0.97, sl_hard:0.81, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Houdini 65", code:"houdini65", entry_threshold:0.65, tp_price:0.75, sl_hard:0.60, sl_trend_delta:0.02, sl_micro_drop:0.20, only_last_10min:false },
    OdiseoDef { name:"Wide 65", code:"odiseo65", entry_threshold:0.65, tp_price:0.95, sl_hard:0.63, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 86", code:"odiseo86", entry_threshold:0.86, tp_price:0.97, sl_hard:0.84, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 87", code:"odiseo87", entry_threshold:0.87, tp_price:0.97, sl_hard:0.85, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 88", code:"odiseo88", entry_threshold:0.88, tp_price:0.97, sl_hard:0.86, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 89", code:"odiseo89", entry_threshold:0.89, tp_price:0.97, sl_hard:0.87, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 90", code:"odiseo90", entry_threshold:0.90, tp_price:0.97, sl_hard:0.88, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 91", code:"odiseo91", entry_threshold:0.91, tp_price:0.97, sl_hard:0.89, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 92", code:"odiseo92", entry_threshold:0.92, tp_price:0.97, sl_hard:0.90, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 93", code:"odiseo93", entry_threshold:0.93, tp_price:0.97, sl_hard:0.91, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 94", code:"odiseo94", entry_threshold:0.94, tp_price:0.97, sl_hard:0.92, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 95", code:"odiseo95", entry_threshold:0.95, tp_price:0.97, sl_hard:0.93, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 96", code:"odiseo96", entry_threshold:0.96, tp_price:0.985, sl_hard:0.95, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:true },
];

#[derive(Debug, Clone, Default)]
struct OdiseoPosition { entered:bool, entry_price:f64, size:f64, settled:bool, exit_price:f64, exit_reason:u8, virtual_pnl:f64, max_price:f64, prev_vol:f64 }
#[derive(Debug, Clone, Default)]
struct OdiseoSessionTrade { up:OdiseoPosition, down:OdiseoPosition, sl_count:u32, session_profit:f64 }
struct OdiseoSessionState { trades:Vec<OdiseoSessionTrade> }

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
    sessions:   Mutex<HashMap<i32, OdiseoSessionState>>,
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
    prev_snapshot: Mutex<HashMap<String, OdiseoStats>>, // previous stats for delta calc
}

impl OdiseoTradingManager {
    pub fn new(cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<CmdMsg>>) -> Self {
        let n = ODISEO_DEFS.len();
        let mut enabled = Vec::with_capacity(n);
        for _ in 0..n { enabled.push(AtomicBool::new(true)); }
        let budgets = vec![20.0; n];
        Self { sessions: Mutex::new(HashMap::new()), stats: Mutex::new(ODISEO_DEFS.iter().map(OdiseoStats::new).collect()), live_mode: AtomicBool::new(false), enabled, budgets: Mutex::new(budgets), reinvest: AtomicBool::new(true), max_sessions: Mutex::new(vec![0u32; n]), sessions_done: Mutex::new(vec![0u32; n]), cmd_tx, filter_chain: FilterChain::default_chain(), session_history: Mutex::new(Vec::new()), prev_snapshot: Mutex::new(HashMap::new()) }
    }
    pub fn set_live_mode(&self, on:bool) { self.live_mode.store(on, Ordering::Relaxed); }
    pub fn set_variant(&self, idx:usize, on:bool) { if idx < self.enabled.len() { self.enabled[idx].store(on, Ordering::Relaxed); } }
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

        // ── Boundary safety: no trade in first 20s or last 20s ──
        let in_first = seconds_left > 880;
        let in_last = seconds_left <= 20;
        let boundary_block = in_first || in_last;
        let boundary_label = if in_first {"first20s"} else {"last20s"};

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

        let px = match lt { Some(p) if p>0.0 => p, _ => {
            // Si ya hay posición abierta, mantenemos último estado (no resetear)
            if pos.entered && !pos.settled {
                let pnl = (pos.entry_price - pos.entry_price) * pos.size; // 0
                r.push((code.clone(), 2u8, pos.entry_price, pos.size, 0.0, 0.0, 0u8, budget));
                return;
            }
            r.push((code,1,0.0,0.0,0.0,0.0,0,budget)); return;
        }};

        if pos.entered && !pos.settled {
            let reason = self.check_exit(pos, def, px, if is_up{av}else{bv}, imb, vel);
            if reason > 0 {
                pos.settled = true; pos.exit_reason = reason;
                let fill = if reason==1 { def.tp_price } else { px };
                pos.exit_price = fill; pos.virtual_pnl = (fill - pos.entry_price) * pos.size;
                t.session_profit += pos.virtual_pnl;
                if reason >= 2 { t.sl_count += 1; }
                info!("[Odiseo] #{} {} EXIT r={} @{:.4} pnl={:.4} sl_count={}", sid, code, reason, fill, pos.virtual_pnl, t.sl_count);
                // ── Live: place real exit order ──
                if self.live_mode.load(Ordering::Relaxed) {
                    if let Some(ref tx) = self.cmd_tx {
                        let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                        if reason == 1 {
                            let _ = tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Sell, outcome, price: def.tp_price, size: pos.size });
                        } else {
                            // CLOB V2: SELL market orders use shares (contracts), not USD
                            let _ = tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Sell, outcome, amount_usdc: pos.size.max(1.0) });
                        }
                    }
                }
            }
        }
        if pos.settled {
            let bal = budget + pos.virtual_pnl;
            r.push((code.clone(),0,pos.entry_price,pos.size,pos.virtual_pnl,pos.exit_price,pos.exit_reason,bal));
            *pos = OdiseoPosition::default(); // reset para re-entry en misma sesión
        }

        if !pos.entered && px >= def.entry_threshold && px <= def.tp_price {
            // ── PRE-ENTRY FILTER LAYER ──────────────────────────────
            {
                let mut fctx = ctx.clone();
                fctx.px = px;
                fctx.is_up = is_up;
                fctx.budget = budget;
                if let FilterResult::Block { reason } = self.filter_chain.check(&fctx, &code, sid) {
                    info!("[Odiseo] #{} {} FILTERED OUT: {}", sid, code, reason);
                    let status = 1u8;
                    r.push((code, status, 0.0, 0.0, 0.0, 0.0, 0u8, budget));
                    return;
                }
            }
            // ─────────────────────────────────────────────────────────
            pos.entered = true; pos.entry_price = px;
            pos.size = (budget/px).floor().max(1.0);
            pos.max_price = px; pos.prev_vol = if is_up{av}else{bv};
            *sig |= if is_up{1}else{2};
            info!("[Odiseo] #{} {} ENTER @{:.4} sz={:.0}", sid, code, px, pos.size);
            // ── Live: place real entry order ──
            if self.live_mode.load(Ordering::Relaxed) {
                if let Some(ref tx) = self.cmd_tx {
                    let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
                    let _ = tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Buy, outcome, price: px, size: pos.size });
                }
            }
        }

        if pos.entered && !pos.settled {
            if px > pos.max_price { pos.max_price = px; }
            pos.prev_vol = if is_up{av}else{bv};
        }

        let status = if pos.entered && !pos.settled {2u8}else{1u8}; // 2=ACTIVE, 1=WATCHING
        let pnl = if status==2 {(px-pos.entry_price)*pos.size}else if pos.settled{pos.virtual_pnl}else{0.0};
        r.push((code, status, pos.entry_price, pos.size, pnl, if pos.settled{pos.exit_price}else{0.0}, if pos.settled{pos.exit_reason}else{0u8}, budget+pnl));
    }

    fn check_exit(&self, pos:&OdiseoPosition, def:&OdiseoDef, px:f64, vol:f64, imb:f64, vel:f64) -> u8 {
        if px >= def.tp_price { return 1; }
        if pos.prev_vol>0.0 && (pos.prev_vol-vol)/pos.prev_vol > def.sl_micro_drop && imb < -0.5 { return 2; }
        if pos.max_price - px > def.sl_trend_delta && vel < 0.0 { return 3; }
        if px <= def.sl_hard { return 4; }
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
