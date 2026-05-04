//! Odiseo Strategies v5 — 5 variants, TP=0.97, last-10-min specialists, cumulative track
//!
//! Variants:
//!   90: entry>=0.90 tp=0.97 sl=0.84 (full 15min)
//!   93: entry>=0.93 tp=0.97 sl=0.87 (full 15min)
//!   95: entry>=0.95 tp=0.97 sl=0.90 (full 15min)
//!   94: entry>=0.94 tp=0.97 sl=0.88 (only last 10min)
//!   96: entry>=0.96 tp=0.97 sl=0.90 (only last 10min)
//!
//! Anti-whale: entry only if price between entry_threshold and tp_price.
//! 3-layer SL. $20 per variant per direction. Cumulative + per-session reset.

use std::collections::HashMap;
use std::sync::Mutex;
use serde::Serialize;
use tracing::info;

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
    OdiseoDef { name:"Odiseo 90", code:"odiseo90", entry_threshold:0.90, tp_price:0.97, sl_hard:0.87, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 93", code:"odiseo93", entry_threshold:0.93, tp_price:0.97, sl_hard:0.89, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 95", code:"odiseo95", entry_threshold:0.95, tp_price:0.985, sl_hard:0.89, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 94", code:"odiseo94", entry_threshold:0.94, tp_price:0.985, sl_hard:0.89, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:false },
    OdiseoDef { name:"Odiseo 96-97", code:"odiseo96", entry_threshold:0.96, tp_price:0.97, sl_hard:0.89, sl_trend_delta:0.03, sl_micro_drop:0.30, only_last_10min:true },
];

#[derive(Debug, Clone, Default)]
struct OdiseoPosition { entered:bool, entry_price:f64, size:f64, settled:bool, exit_price:f64, exit_reason:u8, virtual_pnl:f64, max_price:f64, prev_vol:f64 }
#[derive(Debug, Clone, Default)]
struct OdiseoSessionTrade { up:OdiseoPosition, down:OdiseoPosition }
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

pub struct OdiseoTradingManager { sessions:Mutex<HashMap<i32,OdiseoSessionState>>, stats:Mutex<Vec<OdiseoStats>> }
impl OdiseoTradingManager {
    pub fn new()->Self { Self { sessions:Mutex::new(HashMap::new()), stats:Mutex::new(ODISEO_DEFS.iter().map(OdiseoStats::new).collect()) } }

    pub fn on_tick(&self, session_id:i32, seconds_left:i32,
                   bid_vol:f64, ask_vol:f64, imb:f64, vel:f64,
                   lt_up:Option<f64>, lt_dn:Option<f64>,
    ) -> (Vec<(String,u8,f64,f64,f64,f64,u8,f64)>, u8)
    {
        let mut sessions = self.sessions.lock().unwrap();
        let state = sessions.entry(session_id).or_insert_with(|| OdiseoSessionState {
            trades: ODISEO_DEFS.iter().map(|_| OdiseoSessionTrade::default()).collect(),
        });
        let mut sig = 0u8;
        let mut results = Vec::with_capacity(10);
        let in_last_10 = seconds_left >= 0 && seconds_left <= 600;
        for (i, def) in ODISEO_DEFS.iter().enumerate() {
            if def.only_last_10min && !in_last_10 {
                results.push((format!("{}_up",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                results.push((format!("{}_down",def.code),0,0.0,0.0,0.0,0.0,0,20.0));
                continue;
            }
            self.process(true, &mut state.trades[i], def, session_id, bid_vol, ask_vol, imb, vel, lt_up, &mut sig, &mut results);
            self.process(false, &mut state.trades[i], def, session_id, bid_vol, ask_vol, imb, vel, lt_dn, &mut sig, &mut results);
        }
        (results, sig)
    }

    fn process(&self, is_up:bool, t:&mut OdiseoSessionTrade, def:&OdiseoDef, sid:i32,
               bv:f64, av:f64, imb:f64, vel:f64, lt:Option<f64>, sig:&mut u8,
               r:&mut Vec<(String,u8,f64,f64,f64,f64,u8,f64)>)
    {
        let code = format!("{}_{}", def.code, if is_up{"up"}else{"down"});
        let pos = if is_up {&mut t.up}else{&mut t.down};
        let px = match lt { Some(p) if p>0.0 => p, _ => { r.push((code,0,0.0,0.0,0.0,0.0,0,20.0)); return; }};

        if pos.entered && !pos.settled {
            let reason = self.check_exit(pos, def, px, if is_up{av}else{bv}, imb, vel);
            if reason > 0 {
                pos.settled = true; pos.exit_reason = reason;
                let fill = if reason==1 { def.tp_price } else { px };
                pos.exit_price = fill; pos.virtual_pnl = (fill - pos.entry_price) * pos.size;
                info!("[Odiseo] #{} {} EXIT r={} @{:.4} pnl={:.4}", sid, code, reason, fill, pos.virtual_pnl);
            }
        }
        if pos.settled { r.push((code,0,pos.entry_price,pos.size,pos.virtual_pnl,pos.exit_price,pos.exit_reason,20.0+pos.virtual_pnl)); return; }

        if !pos.entered && px >= def.entry_threshold && px <= def.tp_price {
            pos.entered = true; pos.entry_price = px;
            pos.size = (20.0/px).floor().max(1.0);
            pos.max_price = px; pos.prev_vol = if is_up{av}else{bv};
            *sig |= if is_up{1}else{2};
            info!("[Odiseo] #{} {} ENTER @{:.4} sz={:.0}", sid, code, px, pos.size);
        }

        if pos.entered && !pos.settled {
            if px > pos.max_price { pos.max_price = px; }
            pos.prev_vol = if is_up{av}else{bv};
        }

        let active = if pos.entered && !pos.settled {1u8}else{0u8};
        let pnl = if active==1 {(px-pos.entry_price)*pos.size}else if pos.settled{pos.virtual_pnl}else{0.0};
        r.push((code, active, pos.entry_price, pos.size, pnl, if pos.settled{pos.exit_price}else{0.0}, if pos.settled{pos.exit_reason}else{0u8}, 20.0+pnl));
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
            stats[i].accuracy=if stats[i].trades_up+stats[i].trades_dn>0{(stats[i].wins_up+stats[i].wins_dn)as f64/(stats[i].trades_up+stats[i].trades_dn)as f64}else{0.0};
        }
        for s in stats.iter_mut(){s.sessions+=1;}
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
}
