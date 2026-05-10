mod api;
mod ui;

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::style::Color;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

use api::*;

#[derive(Clone, Copy, PartialEq)]
pub enum InputMode { Normal, Command }

struct State {
    connected: bool,
    tab: usize,
    btc: f64, btc_open: f64, btc_entry: f64,
    bal: f64,
    live: bool, reinvest: bool, _paper_mode: bool,

    // Odiseo
    odi_label: String, odi_code: String,
    odi_pnl: f64, odi_bal: f64, odi_budget: f64,
    odi_t_up: i64, odi_t_dn: i64, odi_w_up: i64, odi_w_dn: i64,
    odi_tp_up: i64, odi_tp_dn: i64, odi_sl_up: i64, odi_sl_dn: i64,
    odi_sessions: i64, odi_enabled: bool,
    odi_accuracy: f64, odi_avg_pnl: f64, odi_best: f64, odi_worst: f64,
    odi_filters: u16,
    last_odi_t_up: i64, last_odi_t_dn: i64,

    // Houdini 65
    h65_pnl: f64, h65_bal: f64, h65_budget: f64,
    h65_t_up: i64, h65_t_dn: i64, h65_w_up: i64, h65_w_dn: i64,
    h65_tp_up: i64, h65_tp_dn: i64, h65_sl_up: i64, h65_sl_dn: i64,
    h65_sessions: i64, h65_enabled: bool,
    h65_accuracy: f64, h65_avg_pnl: f64, h65_best: f64, h65_worst: f64,
    last_h65_t_up: i64, last_h65_t_dn: i64,

    // Senna (Scalper Momentum)
    sen_pnl: f64, sen_bal: f64, sen_budget: f64,
    sen_t_up: i64, sen_t_dn: i64, sen_w_up: i64, sen_w_dn: i64,
    sen_sessions: i64, sen_enabled: bool,
    last_sen_t_up: i64, last_sen_t_dn: i64,

    // Sessions
    sessions: Vec<SessionInfo>,
    selected_session: usize,

    // Trading UI
    selected_variant: usize, // 0=Odiseo, 1=Houdini, 2=Senna

    // HFT live data
    hft: HftState,

    // ─── Position tracking (real-time) ───
    pos_h65_up: bool, pos_h65_dn: bool,
    pos_h65_entry_up: f64, pos_h65_entry_dn: f64,
    pos_odi_up: bool, pos_odi_dn: bool,
    pos_odi_entry_up: f64, pos_odi_entry_dn: f64,
    prev_hd65_up: u8, prev_hd65_dn: u8,
    prev_od83_up: u8, prev_od83_dn: u8,
    pos_sen_up: bool, pos_sen_dn: bool,
    pos_sen_entry_up: f64, pos_sen_entry_dn: f64,
    prev_sen_up: u8, prev_sen_dn: u8,

    // ─── Command locks (prevent poll overwrite after slash command) ───
    h65_lock: bool,
    odi_lock: bool,
    sen_lock: bool,

    // ─── Budget input mode ───
    input_mode: InputMode,
    input_buf: String,

    orders: i64,
    log: VecDeque<api::LogEntry>,
    warnings: VecDeque<String>,
    last_poll_odiseo: Instant,
    last_poll_btc: Instant,
    last_poll_hft: Instant,
    last_poll_sessions: Instant,
    last_ws: Instant,
    last_api_ok: Instant,
    ws_pings: VecDeque<u64>,   // last 20 WS latencies (ms)
    api_pings: VecDeque<u64>,  // last 20 API latencies (ms)
}

impl State {
    fn new(paper_mode: bool) -> Self {
        Self {
            connected: false, tab: 0,
            btc: 0.0, btc_open: 0.0, btc_entry: 0.0, bal: 0.0,
            live: !paper_mode, reinvest: false, _paper_mode: paper_mode,
            odi_label: "Odiseo 83".into(), odi_code: String::new(),
            odi_pnl: 0.0, odi_bal: 0.0, odi_budget: 0.0,
            odi_t_up: 0, odi_t_dn: 0, odi_w_up: 0, odi_w_dn: 0,
            odi_tp_up: 0, odi_tp_dn: 0, odi_sl_up: 0, odi_sl_dn: 0,
            odi_sessions: 0, odi_enabled: false,
            odi_accuracy: 0.0, odi_avg_pnl: 0.0, odi_best: 0.0, odi_worst: 0.0,
            odi_filters: 0,
            last_odi_t_up: 0, last_odi_t_dn: 0,
            h65_pnl: 0.0, h65_bal: 0.0, h65_budget: 0.0,
            h65_t_up: 0, h65_t_dn: 0, h65_w_up: 0, h65_w_dn: 0,
            h65_tp_up: 0, h65_tp_dn: 0, h65_sl_up: 0, h65_sl_dn: 0,
            h65_sessions: 0, h65_enabled: false,
            h65_accuracy: 0.0, h65_avg_pnl: 0.0, h65_best: 0.0, h65_worst: 0.0,
            last_h65_t_up: 0, last_h65_t_dn: 0,
            sen_pnl: 0.0, sen_bal: 0.0, sen_budget: 0.0,
            sen_t_up: 0, sen_t_dn: 0, sen_w_up: 0, sen_w_dn: 0,
            sen_sessions: 0, sen_enabled: false,
            last_sen_t_up: 0, last_sen_t_dn: 0,
            sessions: Vec::new(), selected_session: 0,
            selected_variant: 0,
            hft: HftState::default(),
            pos_h65_up: false, pos_h65_dn: false,
            pos_h65_entry_up: 0.0, pos_h65_entry_dn: 0.0,
            pos_odi_up: false, pos_odi_dn: false,
            pos_odi_entry_up: 0.0, pos_odi_entry_dn: 0.0,
            prev_hd65_up: 0, prev_hd65_dn: 0,
            prev_od83_up: 0, prev_od83_dn: 0,
            pos_sen_up: false, pos_sen_dn: false,
            pos_sen_entry_up: 0.0, pos_sen_entry_dn: 0.0,
            prev_sen_up: 0, prev_sen_dn: 0,
            h65_lock: false, odi_lock: false, sen_lock: false,
            input_mode: InputMode::Normal,
            input_buf: String::new(),
            orders: 0,
            log: VecDeque::with_capacity(100),
            warnings: VecDeque::with_capacity(20),
            last_poll_odiseo: Instant::now(),
            last_poll_btc: Instant::now(),
            last_poll_hft: Instant::now(),
            last_poll_sessions: Instant::now(),
            last_ws: Instant::now(),
            last_api_ok: Instant::now(),
            ws_pings: VecDeque::with_capacity(20),
            api_pings: VecDeque::with_capacity(20),
        }
    }

    fn add_log(&mut self, text: impl Into<String>, color: Color) {
        self.log.push_front(api::LogEntry::new(text.into(), color));
        if self.log.len() > 100 { self.log.pop_back(); }
    }

    fn add_warning(&mut self, text: impl Into<String>) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        self.warnings.push_front(format!("{} {}", ts, text.into()));
        if self.warnings.len() > 20 { self.warnings.pop_back(); }
    }
}

/// Slash command parser: /h10 /o20 /p  (budgets: 5..100, multiples of 5)
async fn exec_slash_command(cmd: &str, s: &mut State) {
    let cmd = cmd.trim();
    if cmd.is_empty() { return; }

    let first = cmd.chars().next().unwrap();
    let rest = &cmd[1..];

    match first {
        // ─── PANIC: liquidate + market sell + disable ALL strategies ───
        'p' => {
            s.add_log("PANIC — liquidando + apagando TODO", Color::Red);
            s.pos_h65_up = false; s.pos_h65_dn = false;
            s.pos_odi_up = false; s.pos_odi_dn = false;
            s.h65_enabled = false; s.odi_enabled = false;
            // NO lock — let poll confirm backend state

            // 1. PANIC endpoint (cancel + market sell + internal disable_all)
            if let Err(e) = http_post("/api/panic", "{}").await {
                s.add_log(format!("PANIC FAIL: {}", e), Color::Red);
            }

            // 2. Explicitly disable LIVE mode
            if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":false}").await {
                s.add_log(format!("LIVE OFF FAIL: {}", e), Color::Red);
            }

            // 3. Explicitly disable ALL variants (same as zz-emergency)
            for idx in 0..3 {
                let body = format!("{{\"index\":{},\"enable\":false}}", idx);
                if let Err(e) = http_post("/api/odiseo/variant", &body).await {
                    s.add_log(format!("VAR {} OFF FAIL: {}", idx, e), Color::Red);
                }
            }
            s.add_log("TODAS LAS ESTRATEGIAS APAGADAS", Color::Green);
        }
        // ─── Houdini 65: /h = toggle, /h5..h100 = ON + budget + LIVE ───
        'h' => {
            let idx = 1;
            if rest.is_empty() {
                s.h65_enabled = !s.h65_enabled;
                s.h65_lock = true;
                if s.h65_enabled {
                    s.selected_variant = 1;
                    s.add_log(format!("H65 ON  ${:.0}", s.h65_budget), Color::Green);
                    // also enable LIVE mode so orders are real
                    let _ = http_post("/api/odiseo/live", "{\"enable\":true}").await;
                } else {
                    s.add_log("H65 OFF", Color::DarkGray);
                }
                if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":{}}}", idx, s.h65_enabled)).await {
                    s.add_log(format!("H65 API FAIL: {}", e), Color::Red);
                    s.h65_lock = false;
                }
            } else {
                match rest.parse::<f64>() {
                    Ok(amt) if amt >= 5.0 && amt <= 100.0 && amt.trunc() % 5.0 == 0.0 => {
                        s.h65_budget = amt;
                        s.h65_enabled = true;
                        s.selected_variant = 1;
                        s.h65_lock = true;
                        s.add_log(format!("H65 LIVE ${:.0}", amt), Color::Green);
                        let mut ok = true;
                        // 1. Enable LIVE mode (orders reales)
                        if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":true}").await {
                            s.add_log(format!("LIVE ON FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        // 2. Disable all other variants except Houdini 65
                        for i in 0..3 {
                            if i == idx { continue; }
                            let body = format!("{{\"index\":{},\"enable\":false}}", i);
                            let _ = http_post("/api/odiseo/variant", &body).await;
                        }
                        // 3. Enable Houdini 65
                        if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":true}}", idx)).await {
                            s.add_log(format!("H65 API FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        // 4. Set budget
                        if let Err(e) = http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await {
                            s.add_log(format!("H65 BUDGET API FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        if !ok { s.h65_lock = false; }
                    }
                    _ => {
                        s.add_log("USO: /h5..h100 (múltiplos de 5)", Color::Red);
                    }
                }
            }
        }
        // ─── Odiseo 83: /o = toggle, /o5..o100 = ON + budget + LIVE ───
        'o' => {
            let idx = 0;
            if rest.is_empty() {
                s.odi_enabled = !s.odi_enabled;
                s.odi_lock = true;
                if s.odi_enabled {
                    s.selected_variant = 0;
                    s.add_log(format!("O83 ON  ${:.0}", s.odi_budget), Color::Green);
                    let _ = http_post("/api/odiseo/live", "{\"enable\":true}").await;
                } else {
                    s.add_log("O83 OFF", Color::DarkGray);
                }
                if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":{}}}", idx, s.odi_enabled)).await {
                    s.add_log(format!("O83 API FAIL: {}", e), Color::Red);
                    s.odi_lock = false;
                }
            } else {
                match rest.parse::<f64>() {
                    Ok(amt) if amt >= 5.0 && amt <= 100.0 && amt.trunc() % 5.0 == 0.0 => {
                        s.odi_budget = amt;
                        s.odi_enabled = true;
                        s.selected_variant = 0;
                        s.odi_lock = true;
                        s.add_log(format!("O83 LIVE ${:.0}", amt), Color::Green);
                        let mut ok = true;
                        // 1. Enable LIVE mode
                        if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":true}").await {
                            s.add_log(format!("LIVE ON FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        // 2. Disable all other variants except Odiseo 83
                        for i in 0..3 {
                            if i == idx { continue; }
                            let body = format!("{{\"index\":{},\"enable\":false}}", i);
                            let _ = http_post("/api/odiseo/variant", &body).await;
                        }
                        // 3. Enable Odiseo 83
                        if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":true}}", idx)).await {
                            s.add_log(format!("O83 API FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        // 4. Set budget
                        if let Err(e) = http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await {
                            s.add_log(format!("O83 BUDGET API FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        if !ok { s.odi_lock = false; }
                    }
                    _ => {
                        s.add_log("USO: /o5..o100 (múltiplos de 5)", Color::Red);
                    }
                }
            }
        }
        // ─── Senna (Scalper Momentum): /s = toggle, /s5..s100 = ON + budget ───
        's' => {
            let idx = 2;
            if rest.is_empty() {
                s.sen_enabled = !s.sen_enabled;
                s.sen_lock = true;
                if s.sen_enabled {
                    s.selected_variant = 2;
                    s.add_log(format!("SENNA ON ${:.0}", s.sen_budget), Color::Green);
                    let _ = http_post("/api/odiseo/live", "{\"enable\":true}").await;
                } else {
                    s.add_log("SENNA OFF", Color::DarkGray);
                }
                if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":{}}}", idx, s.sen_enabled)).await {
                    s.add_log(format!("SENNA API FAIL: {}", e), Color::Red);
                    s.sen_lock = false;
                }
            } else {
                match rest.parse::<f64>() {
                    Ok(amt) if amt >= 5.0 && amt <= 100.0 && amt.trunc() % 5.0 == 0.0 => {
                        s.sen_budget = amt;
                        s.sen_enabled = true;
                        s.selected_variant = 2;
                        s.sen_lock = true;
                        s.add_log(format!("SENNA LIVE ${:.0}", amt), Color::Green);
                        let mut ok = true;
                        if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":true}").await {
                            s.add_log(format!("LIVE ON FAIL: {}", e), Color::Red);
                            ok = false;
                        }
                        for i in 0..3 { if i == idx { continue; }
                            let _ = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":false}}", i)).await;
                        }
                        if let Err(e) = http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":true}}", idx)).await {
                            s.add_log(format!("SENNA API FAIL: {}", e), Color::Red); ok = false;
                        }
                        if let Err(e) = http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await {
                            s.add_log(format!("SENNA BUDGET FAIL: {}", e), Color::Red); ok = false;
                        }
                        if !ok { s.sen_lock = false; }
                    }
                    _ => { s.add_log("USO: /s5..s100 (múltiplos de 5)", Color::Red); }
                }
            }
        }
        _ => {
            s.add_log(format!("?: /{}   |  /h5..h100 /o5..o100 /s5..s100 /p", cmd), Color::Red);
        }
    }
}

// ─── Variant detection ──────────────────────────────────────────────

fn find_variant<'a>(variants: &'a [OdiseoVariant], code: &str) -> Option<&'a OdiseoVariant> {
    variants.iter().find(|v| v.code.as_deref() == Some(code))
}

fn apply_hft_state(new_hft: &HftState, s: &mut State) {
    // ─── Position detection: track entry/exit from HFT state ───
    // Houdini 65 UP
    if new_hft.hd65_up == 2 && s.prev_hd65_up != 2 {
        s.pos_h65_up = true;
        s.pos_h65_entry_up = new_hft.clob_trade_up;
        s.add_log(format!("▲ H65 ENTER UP @ {:.4}", s.pos_h65_entry_up), Color::Green);
    } else if new_hft.hd65_up != 2 && s.prev_hd65_up == 2 {
        s.pos_h65_up = false;
        let pnl = if s.pos_h65_entry_up > 0.0 && new_hft.clob_trade_up > 0.0 {
            s.h65_budget * (new_hft.clob_trade_up / s.pos_h65_entry_up - 1.0)
        } else { 0.0 };
        s.add_log(format!("▲ H65 EXIT UP @ {:.4} PnL:{:+.2}", new_hft.clob_trade_up, pnl),
            if pnl >= 0.0 { Color::Green } else { Color::Red });
    }
    // Houdini 65 DOWN
    if new_hft.hd65_dn == 2 && s.prev_hd65_dn != 2 {
        s.pos_h65_dn = true;
        s.pos_h65_entry_dn = new_hft.clob_trade_dn;
        s.add_log(format!("▼ H65 ENTER DN @ {:.4}", s.pos_h65_entry_dn), Color::Red);
    } else if new_hft.hd65_dn != 2 && s.prev_hd65_dn == 2 {
        s.pos_h65_dn = false;
        let pnl = if s.pos_h65_entry_dn > 0.0 && new_hft.clob_trade_dn > 0.0 {
            s.h65_budget * (new_hft.clob_trade_dn / s.pos_h65_entry_dn - 1.0)
        } else { 0.0 };
        s.add_log(format!("▼ H65 EXIT DN @ {:.4} PnL:{:+.2}", new_hft.clob_trade_dn, pnl),
            if pnl >= 0.0 { Color::Green } else { Color::Red });
    }
    // Odiseo 83 UP
    if new_hft.od83_up == 2 && s.prev_od83_up != 2 {
        s.pos_odi_up = true;
        s.pos_odi_entry_up = new_hft.clob_trade_up;
        s.add_log(format!("▲ O83 ENTER UP @ {:.4}", s.pos_odi_entry_up), Color::Green);
    } else if new_hft.od83_up != 2 && s.prev_od83_up == 2 {
        s.pos_odi_up = false;
        s.add_log(format!("▲ O83 EXIT UP @ {:.4}", new_hft.clob_trade_up), Color::Yellow);
    }
    // Odiseo 83 DOWN
    if new_hft.od83_dn == 2 && s.prev_od83_dn != 2 {
        s.pos_odi_dn = true;
        s.pos_odi_entry_dn = new_hft.clob_trade_dn;
        s.add_log(format!("▼ O83 ENTER DN @ {:.4}", s.pos_odi_entry_dn), Color::Red);
    } else if new_hft.od83_dn != 2 && s.prev_od83_dn == 2 {
        s.pos_odi_dn = false;
        s.add_log(format!("▼ O83 EXIT DN @ {:.4}", new_hft.clob_trade_dn), Color::Yellow);
    }
    // Senna UP
    if new_hft.sen_up == 2 && s.prev_sen_up != 2 {
        s.pos_sen_up = true;
        s.pos_sen_entry_up = new_hft.clob_trade_up;
        s.add_log(format!("⚡ SENNA ENTER UP @ {:.4}", s.pos_sen_entry_up), Color::Cyan);
    } else if new_hft.sen_up != 2 && s.prev_sen_up == 2 {
        s.pos_sen_up = false;
        let pnl = if s.pos_sen_entry_up > 0.0 && new_hft.clob_trade_up > 0.0 {
            s.sen_budget * (new_hft.clob_trade_up / s.pos_sen_entry_up - 1.0)
        } else { 0.0 };
        s.add_log(format!("⚡ SENNA EXIT UP @ {:.4} PnL:{:+.2}", new_hft.clob_trade_up, pnl),
            if pnl >= 0.0 { Color::Green } else { Color::Red });
    }
    // Senna DOWN
    if new_hft.sen_dn == 2 && s.prev_sen_dn != 2 {
        s.pos_sen_dn = true;
        s.pos_sen_entry_dn = new_hft.clob_trade_dn;
        s.add_log(format!("⚡ SENNA ENTER DN @ {:.4}", s.pos_sen_entry_dn), Color::Cyan);
    } else if new_hft.sen_dn != 2 && s.prev_sen_dn == 2 {
        s.pos_sen_dn = false;
        let pnl = if s.pos_sen_entry_dn > 0.0 && new_hft.clob_trade_dn > 0.0 {
            s.sen_budget * (new_hft.clob_trade_dn / s.pos_sen_entry_dn - 1.0)
        } else { 0.0 };
        s.add_log(format!("⚡ SENNA EXIT DN @ {:.4} PnL:{:+.2}", new_hft.clob_trade_dn, pnl),
            if pnl >= 0.0 { Color::Green } else { Color::Red });
    }

    s.prev_hd65_up = new_hft.hd65_up;
    s.prev_hd65_dn = new_hft.hd65_dn;
    s.prev_od83_up = new_hft.od83_up;
    s.prev_od83_dn = new_hft.od83_dn;
    s.prev_sen_up = new_hft.sen_up;
    s.prev_sen_dn = new_hft.sen_dn;
    s.hft = new_hft.clone();
    s.odi_filters = s.hft.od83_filters;
}

fn detect_trades(v: &OdiseoVariant, last_t_up: i64, last_t_dn: i64,
                 s: &mut State, label: &str) -> (i64, i64) {
    let delta_up = v.trades_up - last_t_up;
    let delta_dn = v.trades_dn - last_t_dn;
    if delta_up > 0 { s.add_log(format!("⬆ ENTER UP  [{} {}]  BTC ${:.0}", label, if s.live{"LIVE"}else{"PAPER"}, s.btc), Color::Green); s.btc_entry = s.btc; }
    if delta_dn > 0 { s.add_log(format!("⬇ ENTER DN  [{} {}]  BTC ${:.0}", label, if s.live{"LIVE"}else{"PAPER"}, s.btc), Color::Red); s.btc_entry = s.btc; }
    if delta_up < 0 { s.add_log(format!("⬆ EXIT  UP  [{} {}]", label, if s.live{"LIVE"}else{"PAPER"}), Color::Yellow); }
    if delta_dn < 0 { s.add_log(format!("⬇ EXIT  DN  [{} {}]", label, if s.live{"LIVE"}else{"PAPER"}), Color::Yellow); }
    (v.trades_up, v.trades_dn)
}

fn apply_variant(v: &OdiseoVariant, s: &mut State) {
    match v.code.as_deref() {
        Some(c) if c.starts_with("odiseo") && c != "odiseo65" => {
            let new_pnl = v.total_pnl;
            let delta = new_pnl - s.odi_pnl;
            if delta.abs() > 0.0001 && s.odi_pnl != 0.0 {
                s.add_log(format!("{} PnL {:+.4} ({:+.4})", v.name.as_deref().unwrap_or("Odi"), new_pnl, delta), if delta>0.0{Color::Green}else{Color::Red});
            }
            s.odi_code = c.to_string();
            s.odi_label = v.name.clone().unwrap_or_else(|| "Odiseo".into());
            s.odi_pnl = new_pnl; s.odi_budget = v.budget; s.odi_bal = v.balance;
            s.odi_t_up = v.trades_up; s.odi_t_dn = v.trades_dn;
            s.odi_w_up = v.wins_up; s.odi_w_dn = v.wins_dn;
            s.odi_tp_up = v.tp_up; s.odi_tp_dn = v.tp_dn;
            s.odi_sl_up = v.sl_up; s.odi_sl_dn = v.sl_dn;
            s.odi_sessions = v.sessions;
            s.odi_accuracy = v.accuracy; s.odi_avg_pnl = v.avg_pnl;
            s.odi_best = v.best; s.odi_worst = v.worst;
            // Don't overwrite enabled state if user just sent a slash command
            if !s.odi_lock {
                s.odi_enabled = v.enabled.unwrap_or(true);
            } else if v.enabled.unwrap_or(true) == s.odi_enabled {
                s.odi_lock = false; // backend confirmed our state, unlock
            }
            let (nu, nd) = detect_trades(v, s.last_odi_t_up, s.last_odi_t_dn, s, &s.odi_label.clone());
            s.last_odi_t_up = nu; s.last_odi_t_dn = nd;
            let total_sl = v.sl_up + v.sl_dn;
            let total_trades = v.trades_up + v.trades_dn;
            if total_trades > 0 && total_sl >= 3 { s.add_warning(format!("{} ALERTA: {} SLs", s.odi_label, total_sl)); }
            if v.total_pnl < -v.budget * 0.1 { s.add_warning(format!("{} PERDIDA >10%", s.odi_label)); }
            if !v.enabled.unwrap_or(true) { s.add_warning(format!("{} DESACTIVADO", s.odi_label)); }
        }
        Some("houdini65") => {
            let new_pnl = v.total_pnl;
            let delta = new_pnl - s.h65_pnl;
            if delta.abs() > 0.0001 && s.h65_pnl != 0.0 {
                s.add_log(format!("H65 PnL {:+.4} ({:+.4})", new_pnl, delta), if delta>0.0{Color::Green}else{Color::Red});
            }
            s.h65_pnl = new_pnl; s.h65_budget = v.budget; s.h65_bal = v.balance;
            s.h65_t_up = v.trades_up; s.h65_t_dn = v.trades_dn;
            s.h65_w_up = v.wins_up; s.h65_w_dn = v.wins_dn;
            s.h65_tp_up = v.tp_up; s.h65_tp_dn = v.tp_dn;
            s.h65_sl_up = v.sl_up; s.h65_sl_dn = v.sl_dn;
            s.h65_sessions = v.sessions;
            s.h65_accuracy = v.accuracy; s.h65_avg_pnl = v.avg_pnl;
            s.h65_best = v.best; s.h65_worst = v.worst;
            // Don't overwrite enabled state if user just sent a slash command
            if !s.h65_lock {
                s.h65_enabled = v.enabled.unwrap_or(true);
            } else if v.enabled.unwrap_or(true) == s.h65_enabled {
                s.h65_lock = false; // backend confirmed our state, unlock
            }
            let (nu, nd) = detect_trades(v, s.last_h65_t_up, s.last_h65_t_dn, s, "H65");
            s.last_h65_t_up = nu; s.last_h65_t_dn = nd;
            let total_sl = v.sl_up + v.sl_dn;
            if v.trades_up + v.trades_dn > 0 && total_sl >= 3 { s.add_warning(format!("H65 ALERTA: {} SLs", total_sl)); }
            if v.total_pnl < -v.budget * 0.1 { s.add_warning("H65 PERDIDA >10%"); }
            if !v.enabled.unwrap_or(true) { s.add_warning("HOUDINI 65 DESACTIVADO"); }
        }
        Some("scalper") => {
            s.sen_pnl = v.total_pnl; s.sen_budget = v.budget; s.sen_bal = v.balance;
            s.sen_t_up = v.trades_up; s.sen_t_dn = v.trades_dn;
            s.sen_w_up = v.wins_up; s.sen_w_dn = v.wins_dn;
            s.sen_sessions = v.sessions;
            if !s.sen_lock {
                s.sen_enabled = v.enabled.unwrap_or(true);
            } else if v.enabled.unwrap_or(true) == s.sen_enabled {
                s.sen_lock = false;
            }
            let (nu, nd) = detect_trades(v, s.last_sen_t_up, s.last_sen_t_dn, s, "SENNA");
            s.last_sen_t_up = nu; s.last_sen_t_dn = nd;
        }
        _ => {}
    }
}

// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> io::Result<()> {
    let paper_mode = std::env::args().any(|a| a == "--paper");

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let (tx, mut rx) = mpsc::channel::<WsMsg>(256);

    // WebSocket task
    tokio::spawn(async move {
        loop {
            if let Ok((ws, _)) = connect_async(WS_URL).await {
                let (_, mut read) = ws.split();
                let _ = tx.send(WsMsg { msg_type: Some("connected".into()), ..Default::default() }).await;
                while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = read.next().await {
                    if let Ok(msg) = serde_json::from_str::<WsMsg>(&text) {
                        let _ = tx.send(msg).await;
                    } else if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text) {
                        if raw.get("type").and_then(|v| v.as_str()) == Some("order_result") {
                            let _ = tx.send(WsMsg {
                                msg_type: Some("order_result".into()),
                                success: raw.get("success").and_then(|v| v.as_bool()),
                                message: raw.get("message").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                ..Default::default()
                            }).await;
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });

    // Keyboard input task
    let (itx, mut irx) = mpsc::channel::<KeyCode>(16);
    tokio::spawn(async move { loop { if let Ok(Event::Key(k)) = event::read() { let _ = itx.send(k.code).await; } } });

    let mut s = State::new(paper_mode);
    let mode_label = if s.live { "DINERO REAL" } else { "PAPER MONEY" };
    s.add_log(format!("ZZIGNAL MONITOR — {}", mode_label), Color::Magenta);

    // Force backend live_mode to match our binary mode
    if s.live {
        if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":true}").await {
            s.add_log(format!("API LIVE FAIL: {}", e), Color::Red);
        }
        s.add_log("DINERO REAL — esperando comandos", Color::Red);
    } else {
        if let Err(e) = http_post("/api/odiseo/live", "{\"enable\":false}").await {
            s.add_log(format!("API LIVE FAIL: {}", e), Color::Red);
        }
        // Paper mode: auto-enable both strategies for data collection
        if let Err(e) = http_post("/api/odiseo/variant", "{\"index\":0,\"enable\":true}").await {
            s.add_log(format!("API O83 FAIL: {}", e), Color::Red);
        } else {
            s.odi_enabled = true;
        }
        if let Err(e) = http_post("/api/odiseo/variant", "{\"index\":1,\"enable\":true}").await {
            s.add_log(format!("API H65 FAIL: {}", e), Color::Red);
        } else {
            s.h65_enabled = true;
        }
        s.add_log("PAPER MONEY — ambas estrategias ON", Color::Cyan);
    }

    loop {
        // ─── Drain WS ─────────────────────────────────────────────────
        while let Ok(msg) = rx.try_recv() {
            let ws_lat = s.last_ws.elapsed().as_millis() as u64;
            s.ws_pings.push_front(ws_lat);
            if s.ws_pings.len() > 20 { s.ws_pings.pop_back(); }
            s.last_ws = Instant::now();
            match msg.msg_type.as_deref() {
                Some("connected") => { s.connected = true; s.add_log("WS OK", Color::Green); }
                Some("snapshot") => { s.bal = msg.balance.unwrap_or(s.bal); s.btc = msg.btc.unwrap_or(s.btc); }
                Some("btc_price") => s.btc = msg.price.unwrap_or(s.btc),
                Some("balance") => {
                    let old = s.bal; s.bal = msg.balance.unwrap_or(s.bal);
                    if (s.bal - old).abs() > 0.01 { s.add_log(format!("USD ${:.2} ({:+.2})", s.bal, s.bal-old), if s.bal>old{Color::Green}else{Color::Red}); }
                }
                Some("order_result") => {
                    let ok = msg.success.unwrap_or(false);
                    let txt = msg.message.unwrap_or_default();
                    s.add_log(format!("{} {}", if ok {"OK"}else{"FAIL"}, txt), if ok{Color::Green}else{Color::Red});
                }
                Some("hft_state") => {
                    if let Some(ref hft) = msg.data {
                        apply_hft_state(hft, &mut s);
                    }
                }
                _ => {}
            }
        }

        // ─── Poll Odiseo Status (2s) ─────────────────────────────────
        if s.last_poll_odiseo.elapsed() > Duration::from_secs(2) {
            s.last_poll_odiseo = Instant::now();
            if let Some(data) = http_get::<OdiseoStatus>("/api/odiseo/status").await {
                let api_lat = s.last_api_ok.elapsed().as_millis() as u64;
                s.api_pings.push_front(api_lat);
                if s.api_pings.len() > 20 { s.api_pings.pop_back(); }
                s.last_api_ok = Instant::now();
                // Don't overwrite live from API — mode is fixed per binary
                s.reinvest = data.reinvest.unwrap_or(false);

                let odi_v = data.variants.iter().find(|v| {
                    let code = v.code.as_deref().unwrap_or("");
                    code.starts_with("odiseo") && code != "odiseo65"
                });
                if let Some(v) = odi_v { apply_variant(v, &mut s); }
                if let Some(v) = find_variant(&data.variants, "houdini65") { apply_variant(v, &mut s); }
                if let Some(v) = find_variant(&data.variants, "scalper") { apply_variant(v, &mut s); }
            }

            if let Some(orders) = http_get::<Vec<serde_json::Value>>("/api/orders").await {
                let nc = orders.len() as i64;
                if nc != s.orders { s.add_log(format!("Orders: {} -> {}", s.orders, nc), Color::Cyan); }
                s.orders = nc;
            }
        }

        // ─── Poll BTC (5s) ───────────────────────────────────────────
        if s.last_poll_btc.elapsed() > Duration::from_secs(5) {
            s.last_poll_btc = Instant::now();
            if let Some(data) = http_get::<BtcInfo>("/api/btc").await {
                s.btc = data.price;
                if s.btc_open == 0.0 { s.btc_open = data.open; }
            }
            if let Some(data) = http_get::<HealthInfo>("/api/health").await {
                s.bal = data.balance;
            }
        }

        // ─── Poll HFT (500ms, fallback si WS hft_state no llega) ─────
        if s.last_poll_hft.elapsed() > Duration::from_millis(500) {
            s.last_poll_hft = Instant::now();
            if let Some(data) = http_get::<HftState>("/api/hft/latest").await {
                let api_lat = s.last_api_ok.elapsed().as_millis() as u64;
                s.api_pings.push_front(api_lat);
                if s.api_pings.len() > 20 { s.api_pings.pop_back(); }
                s.last_api_ok = Instant::now();
                apply_hft_state(&data, &mut s);
            }
        }

        // ─── Poll Sessions (10s) ─────────────────────────────────────
        if s.last_poll_sessions.elapsed() > Duration::from_secs(10) {
            s.last_poll_sessions = Instant::now();
            if let Some(data) = http_get::<Vec<SessionInfo>>("/api/sessions").await {
                s.sessions = data;
            }
        }

        // ─── Keyboard ────────────────────────────────────────────────
        while let Ok(k) = irx.try_recv() {
            // ── Command mode ──────────────────────────────────────
            if s.input_mode == InputMode::Command {
                match k {
                    KeyCode::Esc => { s.input_mode = InputMode::Normal; s.input_buf.clear(); }
                    KeyCode::Enter => {
                        let c = s.input_buf.trim().to_string();
                        exec_slash_command(&c, &mut s).await;
                        s.input_mode = InputMode::Normal; s.input_buf.clear();
                    }
                    KeyCode::Backspace => { s.input_buf.pop(); }
                    KeyCode::Char(c) => { if s.input_buf.len() < 20 { s.input_buf.push(c); } }
                    _ => {}
                }
                continue;
            }

            match k {
                KeyCode::Esc | KeyCode::Char('q') => {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                KeyCode::Tab => { s.tab = if s.tab == 0 { 1 } else { 0 }; }
                KeyCode::Char('/') => {
                    s.input_mode = InputMode::Command; s.input_buf.clear();
                }
                // ── Sessions ──
                KeyCode::Char('s') => {
                    s.add_log("Starting 15-min session...", Color::Green);
                    let now = chrono::Utc::now();
                    let name = now.format("BTC15-Manual-%Y%m%dT%H%M").to_string();
                    if let Err(e) = http_post("/api/sessions/start", &format!(r#"{{"name":"{}","duration_min":15,"depth_levels":50,"indefinite":true}}"#, name)).await {
                        s.add_log(format!("Session start FAIL: {}", e), Color::Red);
                    }
                }
                _ => {}
            }
        }

        terminal.draw(|f| ui::draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
