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

    // ─── SL config ───
    sl_pct: f64,       // default 12 (%)
    sl_market: bool,   // true=market sell, false=limit sell

    // ─── Budget input mode ───
    input_mode: InputMode,
    input_buf: String,

    orders: i64,
    log: VecDeque<api::LogEntry>,
    trades: VecDeque<api::TradeEntry>,
    warnings: VecDeque<String>,
    last_poll_odiseo: Instant,
    last_poll_btc: Instant,
    last_poll_hft: Instant,
    last_poll_sessions: Instant,
    last_ws: Instant,
    last_api_ok: Instant,
    ws_pings: VecDeque<u64>,   // last 20 WS latencies (ms)
    api_pings: VecDeque<u64>,  // last 20 API latencies (ms)
    book_up: api::BookDepth,
    book_dn: api::BookDepth,
    last_poll_depth: Instant,
    prev_secs_left: i32,
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
            sl_pct: 12.0, sl_market: true,
            input_mode: InputMode::Normal,
            input_buf: String::new(),
            orders: 0,
            log: VecDeque::with_capacity(100),
            trades: VecDeque::with_capacity(100),
            warnings: VecDeque::with_capacity(20),
            last_poll_odiseo: Instant::now(),
            last_poll_btc: Instant::now(),
            last_poll_hft: Instant::now(),
            last_poll_sessions: Instant::now(),
            last_ws: Instant::now(),
            last_api_ok: Instant::now(),
            ws_pings: VecDeque::with_capacity(20),
            api_pings: VecDeque::with_capacity(20),
            book_up: api::BookDepth::default(),
            book_dn: api::BookDepth::default(),
            last_poll_depth: Instant::now(),
            prev_secs_left: -1,
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

/// Slash command parser: /10up65 /15d40e35 /p — manual trading
async fn exec_slash_command(cmd: &str, s: &mut State) {
    let cmd = cmd.trim();
    if cmd.is_empty() { return; }

    let first = cmd.chars().next().unwrap();

    match first {
        'p' => {
            s.add_log("PANIC — liquidando TODO", Color::Red);
            if let Err(e) = http_post("/api/panic", "{}").await {
                s.add_log(format!("PANIC FAIL: {}", e), Color::Red);
            } else {
                s.add_log("TODO LIQUIDADO", Color::Green);
            }
        }
        // ─── SL config: /sl = toggle MKT/LMT, /sl10 = 10%, /nsl = OFF ───
        's' if cmd.len() >= 2 && cmd.as_bytes()[1] == b'l' => {
            let rest = &cmd[2..];
            if rest.is_empty() {
                s.sl_market = !s.sl_market;
                s.add_log(format!("SL: {} {}", if s.sl_market {"MARKET"}else{"LIMIT"}, if s.sl_pct > 0.0 {format!("{}%", s.sl_pct)}else{"OFF".into()}), Color::Yellow);
            } else if let Ok(pct) = rest.parse::<f64>() {
                s.sl_pct = pct.max(1.0).min(50.0);
                s.add_log(format!("SL: {}% {}", s.sl_pct, if s.sl_market {"MARKET"}else{"LIMIT"}), Color::Yellow);
            } else {
                s.add_log("USO: /sl  |  /sl10  |  /nsl", Color::Red);
            }
        }
        // ─── No SL: /nsl ───
        'n' if cmd == "nsl" => {
            s.sl_pct = 0.0;
            s.add_log("SL: OFF — sin stop loss", Color::DarkGray);
        }
        _ => {
            // Parse compact command: /10up65 or /15d40e35
            let rest = cmd;
            let (amount, remaining) = match parse_amount(rest) {
                Some(v) => v,
                None => { s.add_log(format!("?: /{}   formato: /10up65 /15d40e35 /p", rest), Color::Red); return; }
            };
            let (side, remaining) = match parse_side_name(remaining) {
                Some(v) => v,
                None => { s.add_log(format!("?: /{} — usa 'up' o 'd'", rest), Color::Red); return; }
            };
            let (price, remaining) = match parse_cents(remaining) {
                Some(v) => v,
                None => { s.add_log(format!("?: /{} — falta precio", rest), Color::Red); return; }
            };
            let exit_price = if remaining.starts_with('e') {
                parse_cents(&remaining[1..]).map(|(p, _)| p)
            } else { None };

            let outcome = if side == "up" { "up" } else { "down" };
            let size = (amount / price).floor().max(1.0);

            // Place limit BUY
            let sl_price = price * (1.0 - s.sl_pct / 100.0);
            s.add_log(format!("▶ BUY {} ${:.0} @{:.2} sz={:.0}",
                outcome.to_uppercase(), amount, price, size),
                if side == "up" { Color::Green } else { Color::Red });
            if s.sl_pct > 0.0 {
                s.add_log(format!("  SL: {}% {}  — colocar tras ver FILL real", s.sl_pct, if s.sl_market {"MARKET"}else{"LIMIT"}), Color::Yellow);
            } else {
                s.add_log("  SL: OFF", Color::DarkGray);
            }
            if let Err(e) = http_post("/api/orders/limit",
                &format!(r#"{{"side":"buy","outcome":"{}","price":{},"size":{}}}"#, outcome, price, size)).await {
                s.add_log(format!("BUY FAIL: {}", e), Color::Red);
            }

            // Place SL order (limit sell at SL price)
            if !s.sl_market {
                s.add_log(format!("▶ SL LIMIT SELL {} @{:.4}", outcome.to_uppercase(), sl_price), Color::Yellow);
                if let Err(e) = http_post("/api/orders/limit",
                    &format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, sl_price, size)).await {
                    s.add_log(format!("SL FAIL: {}", e), Color::Red);
                }
            } else {
                s.add_log(format!("  SL: {}% MARKET — ejecutar manual si precio baja a {:.4}", s.sl_pct, sl_price), Color::DarkGray);
            }

            // Place exit limit SELL if specified
            if let Some(exit) = exit_price {
                s.add_log(format!("▶ LIMIT SELL {} @{:.2} (exit)", outcome.to_uppercase(), exit), Color::Yellow);
                if let Err(e) = http_post("/api/orders/limit",
                    &format!(r#"{{"side":"sell","outcome":"{}","price":{},"size":{}}}"#, outcome, exit, size)).await {
                    s.add_log(format!("EXIT FAIL: {}", e), Color::Red);
                }
            }
        }
    }
}

fn parse_amount(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 { return None; }
    let n: f64 = s[..end].parse().ok()?;
    if n < 1.0 || n > 200.0 { return None; }
    Some((n, &s[end..]))
}

fn parse_side_name(s: &str) -> Option<(&str, &str)> {
    if s.starts_with("up") { Some(("up", &s[2..])) }
    else if s.starts_with('d') { Some(("down", &s[1..])) }
    else { None }
}

fn parse_cents(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 { return None; }
    let n: f64 = s[..end].parse().ok()?;
    let price = n / 100.0;
    if price <= 0.0 || price >= 1.0 { return None; }
    Some((price, &s[end..]))
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

    // ─── Track trades for TAP display ────────────────────────────
    if new_hft.clob_trade_up > 0.0 && (s.trades.is_empty() || (new_hft.clob_trade_up - s.trades.front().map(|t| t.price).unwrap_or(0.0)).abs() > 0.0001) {
        s.trades.push_front(api::TradeEntry {
            ts: new_hft.time.clone(),
            side: "UP".into(),
            price: new_hft.clob_trade_up,
            size: new_hft.clob_trade_up_vol,
        });
        if s.trades.len() > 100 { s.trades.pop_back(); }
    }
    if new_hft.clob_trade_dn > 0.0 && (s.trades.is_empty() || s.trades.len() < 2 || (new_hft.clob_trade_dn - s.trades.iter().filter(|t| t.side == "DOWN").next().map(|t| t.price).unwrap_or(0.0)).abs() > 0.0001) {
        s.trades.push_front(api::TradeEntry {
            ts: new_hft.time.clone(),
            side: "DOWN".into(),
            price: new_hft.clob_trade_dn,
            size: new_hft.clob_trade_dn_vol,
        });
        if s.trades.len() > 100 { s.trades.pop_back(); }
    }

    s.hft = new_hft.clone();
    s.odi_filters = s.hft.od83_filters;

    // ─── Session change detection: clear stale book data ───────────
    let secs = new_hft.secs_left;
    if s.prev_secs_left >= 0 && secs > s.prev_secs_left + 60 {
        s.book_up = api::BookDepth::default();
        s.book_dn = api::BookDepth::default();
        s.add_log(format!("SESSION RESET — new orderbook"), Color::Yellow);
    }
    s.prev_secs_left = secs;
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
                Some("book") => {
                    if let Some(ref book) = msg.book {
                        match msg.side.as_deref() {
                            Some("up") => s.book_up = book.clone(),
                            Some("down") => s.book_dn = book.clone(),
                            _ => {}
                        }
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
                KeyCode::Tab => { s.tab = (s.tab + 1) % 3; }
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
