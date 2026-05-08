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

const FILTER_KEYS: &[(char, &str)] = &[
    ('1', "frozen_market"), ('2', "spread_health"), ('3', "flash_dump"),
    ('4', "min_volume"), ('5', "btc_trend_confirm"), ('6', "reversal_risk"),
    ('7', "spoof_protection"), ('8', "ask_wall"), ('9', "mid_price_sanity"),
    ('0', "imbalance_sanity"),
    // Filters 11-14: session_age, reentry_cooldown, liquidity_depth, depth_balance
    // Use a=all-ON / z=all-OFF to manage these together with the first 10.
];

#[derive(Clone, Copy, PartialEq)]
pub enum InputMode { Normal, Budget, Command }

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

    // Sessions
    sessions: Vec<SessionInfo>,
    selected_session: usize,

    // Trading UI
    selected_variant: usize, // 0=Odiseo, 1=Houdini

    // HFT live data
    hft: HftState,

    // ─── Position tracking (real-time) ───
    pos_h65_up: bool, pos_h65_dn: bool,
    pos_h65_entry_up: f64, pos_h65_entry_dn: f64,
    pos_odi_up: bool, pos_odi_dn: bool,
    pos_odi_entry_up: f64, pos_odi_entry_dn: f64,
    prev_hd65_up: u8, prev_hd65_dn: u8,
    prev_od83_up: u8, prev_od83_dn: u8,

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
}

impl State {
    fn new(paper_mode: bool) -> Self {
        Self {
            connected: false, tab: 0,
            btc: 0.0, btc_open: 0.0, btc_entry: 0.0, bal: 0.0,
            live: !paper_mode, reinvest: false, _paper_mode: paper_mode,
            odi_label: "Odiseo 83".into(), odi_code: String::new(),
            odi_pnl: 0.0, odi_bal: 0.0, odi_budget: 20.0,
            odi_t_up: 0, odi_t_dn: 0, odi_w_up: 0, odi_w_dn: 0,
            odi_tp_up: 0, odi_tp_dn: 0, odi_sl_up: 0, odi_sl_dn: 0,
            odi_sessions: 0, odi_enabled: true,
            odi_accuracy: 0.0, odi_avg_pnl: 0.0, odi_best: 0.0, odi_worst: 0.0,
            odi_filters: 0,
            last_odi_t_up: 0, last_odi_t_dn: 0,
            h65_pnl: 0.0, h65_bal: 0.0, h65_budget: 20.0,
            h65_t_up: 0, h65_t_dn: 0, h65_w_up: 0, h65_w_dn: 0,
            h65_tp_up: 0, h65_tp_dn: 0, h65_sl_up: 0, h65_sl_dn: 0,
            h65_sessions: 0, h65_enabled: true,
            h65_accuracy: 0.0, h65_avg_pnl: 0.0, h65_best: 0.0, h65_worst: 0.0,
            last_h65_t_up: 0, last_h65_t_dn: 0,
            sessions: Vec::new(), selected_session: 0,
            selected_variant: 0,
            hft: HftState::default(),
            pos_h65_up: false, pos_h65_dn: false,
            pos_h65_entry_up: 0.0, pos_h65_entry_dn: 0.0,
            pos_odi_up: false, pos_odi_dn: false,
            pos_odi_entry_up: 0.0, pos_odi_entry_dn: 0.0,
            prev_hd65_up: 0, prev_hd65_dn: 0,
            prev_od83_up: 0, prev_od83_dn: 0,
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

/// Slash command parser: /h10 /o20 /p /r /b30
async fn exec_slash_command(cmd: &str, s: &mut State) {
    let cmd = cmd.trim();
    if cmd.is_empty() { return; }

    let first = cmd.chars().next().unwrap();
    let rest = &cmd[1..];

    match first {
        'p' => {
            s.add_log("PANIC SELL!", Color::Red);
            s.pos_h65_up = false; s.pos_h65_dn = false;
            s.pos_odi_up = false; s.pos_odi_dn = false;
            http_post("/api/panic", "{}").await;
        }
        'r' => {
            s.reinvest = !s.reinvest;
            s.add_log(format!("Reinvest: {}", if s.reinvest {"ON"}else{"OFF"}), Color::Yellow);
            http_post("/api/odiseo/reinvest", &format!("{{\"enable\":{}}}", s.reinvest)).await;
        }
        'h' => {
            let idx = 1;
            if rest.is_empty() {
                s.h65_enabled = !s.h65_enabled;
                s.add_log(format!("H65: {}", if s.h65_enabled {"ON"}else{"OFF"}), if s.h65_enabled{Color::Green}else{Color::DarkGray});
                http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":{}}}", idx, s.h65_enabled)).await;
            } else if let Ok(amt) = rest.parse::<f64>() {
                let amt = amt.clamp(1.0, 200.0);
                s.h65_budget = amt;
                s.h65_enabled = true;
                s.selected_variant = 1;
                s.add_log(format!("H65 ON ${:.0}", amt), Color::Green);
                http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":true}}", idx)).await;
                http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await;
            }
        }
        'o' => {
            let idx = 0;
            if rest.is_empty() {
                s.odi_enabled = !s.odi_enabled;
                s.add_log(format!("O83: {}", if s.odi_enabled {"ON"}else{"OFF"}), if s.odi_enabled{Color::Green}else{Color::DarkGray});
                http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":{}}}", idx, s.odi_enabled)).await;
            } else if let Ok(amt) = rest.parse::<f64>() {
                let amt = amt.clamp(1.0, 200.0);
                s.odi_budget = amt;
                s.odi_enabled = true;
                s.selected_variant = 0;
                s.add_log(format!("O83 ON ${:.0}", amt), Color::Green);
                http_post("/api/odiseo/variant", &format!("{{\"index\":{},\"enable\":true}}", idx)).await;
                http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await;
            }
        }
        'b' => {
            if let Ok(amt) = rest.parse::<f64>() {
                let amt = amt.clamp(1.0, 200.0);
                let idx = s.selected_variant;
                if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                let name = if idx == 0 { "O83" } else { "H65" };
                s.add_log(format!("{} budget ${:.0}", name, amt), Color::Cyan);
                http_post("/api/odiseo/budget", &format!("{{\"index\":{},\"amount\":{}}}", idx, amt)).await;
            }
        }
        _ => {
            s.add_log(format!("?: /{}", cmd), Color::Red);
        }
    }
}

// ─── Variant detection ──────────────────────────────────────────────

fn find_variant<'a>(variants: &'a [OdiseoVariant], code: &str) -> Option<&'a OdiseoVariant> {
    variants.iter().find(|v| v.code.as_deref() == Some(code))
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
            s.odi_enabled = v.enabled.unwrap_or(true);
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
            s.h65_enabled = v.enabled.unwrap_or(true);
            let (nu, nd) = detect_trades(v, s.last_h65_t_up, s.last_h65_t_dn, s, "H65");
            s.last_h65_t_up = nu; s.last_h65_t_dn = nd;
            let total_sl = v.sl_up + v.sl_dn;
            if v.trades_up + v.trades_dn > 0 && total_sl >= 3 { s.add_warning(format!("H65 ALERTA: {} SLs", total_sl)); }
            if v.total_pnl < -v.budget * 0.1 { s.add_warning(format!("H65 PERDIDA >10%")); }
            if !v.enabled.unwrap_or(true) { s.add_warning("HOUDINI 65 DESACTIVADO"); }
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
    s.add_log("ZZIGNAL MONITOR v1.0 — Control Total", Color::Magenta);

    loop {
        // ─── Drain WS ─────────────────────────────────────────────────
        while let Ok(msg) = rx.try_recv() {
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
                _ => {}
            }
        }

        // ─── Poll Odiseo Status (2s) ─────────────────────────────────
        if s.last_poll_odiseo.elapsed() > Duration::from_secs(2) {
            s.last_poll_odiseo = Instant::now();
            if let Some(data) = http_get::<OdiseoStatus>("/api/odiseo/status").await {
                s.last_api_ok = Instant::now();
                s.live = data.live_mode;
                s.reinvest = data.reinvest.unwrap_or(false);

                let odi_v = data.variants.iter().find(|v| {
                    let code = v.code.as_deref().unwrap_or("");
                    code.starts_with("odiseo") && code != "odiseo65"
                });
                if let Some(v) = odi_v { apply_variant(v, &mut s); }
                if let Some(v) = find_variant(&data.variants, "houdini65") { apply_variant(v, &mut s); }
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

        // ─── Poll HFT (500ms) ────────────────────────────────────────
        if s.last_poll_hft.elapsed() > Duration::from_millis(500) {
            s.last_poll_hft = Instant::now();
            if let Some(data) = http_get::<HftState>("/api/hft/latest").await {
                s.last_api_ok = Instant::now();

                // ─── Position detection: track entry/exit from HFT state ───
                let new_hft = &data;

                // Houdini 65 UP
                if new_hft.hd65_up == 2 && s.prev_hd65_up != 2 {
                    s.pos_h65_up = true;
                    s.pos_h65_entry_up = new_hft.clob_trade_up;
                    s.add_log(format!("▲ H65 ENTER UP @ {:.4}", s.pos_h65_entry_up), Color::Green);
                } else if new_hft.hd65_up != 2 && s.prev_hd65_up == 2 {
                    s.pos_h65_up = false;
                    s.add_log(format!("▲ H65 EXIT UP @ {:.4}", new_hft.clob_trade_up), Color::Yellow);
                }
                // Houdini 65 DOWN
                if new_hft.hd65_dn == 2 && s.prev_hd65_dn != 2 {
                    s.pos_h65_dn = true;
                    s.pos_h65_entry_dn = new_hft.clob_trade_dn;
                    s.add_log(format!("▼ H65 ENTER DN @ {:.4}", s.pos_h65_entry_dn), Color::Red);
                } else if new_hft.hd65_dn != 2 && s.prev_hd65_dn == 2 {
                    s.pos_h65_dn = false;
                    s.add_log(format!("▼ H65 EXIT DN @ {:.4}", new_hft.clob_trade_dn), Color::Yellow);
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

                s.prev_hd65_up = new_hft.hd65_up;
                s.prev_hd65_dn = new_hft.hd65_dn;
                s.prev_od83_up = new_hft.od83_up;
                s.prev_od83_dn = new_hft.od83_dn;
                s.hft = data;
                s.odi_filters = s.hft.od83_filters;
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
            // ── Command mode: /h10 /p /l /o /r /b20 ──
            if s.input_mode == InputMode::Command {
                match k {
                    KeyCode::Esc => { s.input_mode = InputMode::Normal; s.input_buf.clear(); }
                    KeyCode::Enter => {
                        let cmd = s.input_buf.trim().to_string();
                        exec_slash_command(&cmd, &mut s).await;
                        s.input_mode = InputMode::Normal; s.input_buf.clear();
                    }
                    KeyCode::Backspace => { s.input_buf.pop(); }
                    KeyCode::Char(c) => {
                        if s.input_buf.len() < 20 { s.input_buf.push(c); }
                    }
                    _ => {}
                }
                continue;
            }

            // ── Budget input mode ──────────────────────────────────
            if s.input_mode == InputMode::Budget {
                match k {
                    KeyCode::Esc => { s.input_mode = InputMode::Normal; s.input_buf.clear(); }
                    KeyCode::Enter => {
                        if let Ok(amt) = s.input_buf.parse::<f64>() {
                            let amt = amt.clamp(1.0, 200.0);
                            let idx = s.selected_variant;
                            if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                            s.add_log(format!("Budget ${:.0}", amt), Color::Yellow);
                            http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                        }
                        s.input_mode = InputMode::Normal; s.input_buf.clear();
                    }
                    KeyCode::Backspace => { s.input_buf.pop(); }
                    KeyCode::Char(c @ ('0'..='9')) => {
                        if s.input_buf.len() < 4 { s.input_buf.push(c); }
                    }
                    KeyCode::Char('.') => {
                        if !s.input_buf.contains('.') && s.input_buf.len() < 4 { s.input_buf.push('.'); }
                    }
                    _ => {}
                }
                continue;
            }

            // ── Normal key handling ─────────────────────────────
            match (s.tab, k) {
                // ── GLOBAL (all tabs) ───────────────────────────────
                (_, KeyCode::Esc) | (_, KeyCode::Char('q')) => {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                (_, KeyCode::Right) | (_, KeyCode::Tab) => { s.tab = (s.tab + 1) % 4; }
                (_, KeyCode::Left)  => { s.tab = if s.tab == 0 { 3 } else { s.tab - 1 }; }

                (_, KeyCode::Char('/')) => {
                    s.input_mode = InputMode::Command; s.input_buf.clear();
                }
                // ── TRADING HOTKEYS (global, all tabs) ───────────────
                (_, KeyCode::Char('r')) => {
                    let nv = !s.reinvest; s.reinvest = nv;
                    s.add_log(format!("Reinvest: {}", if nv{"ON"}else{"OFF"}), Color::Yellow);
                    http_post("/api/odiseo/reinvest", &format!("{{\"enable\":{nv}}}")).await;
                }
                (_, KeyCode::Char('p')) => {
                    s.add_log("PANIC SELL!", Color::Red);
                    s.pos_h65_up = false; s.pos_h65_dn = false;
                    s.pos_odi_up = false; s.pos_odi_dn = false;
                    http_post("/api/panic", "{}").await;
                }
                (_, KeyCode::Char('o')) => {
                    let nv = !s.odi_enabled; s.odi_enabled = nv;
                    s.add_log(format!("Odiseo 83: {}", if nv{"ON"}else{"OFF"}), if nv{Color::Green}else{Color::DarkGray});
                    http_post("/api/odiseo/variant", &format!("{{\"index\":0,\"enable\":{nv}}}")).await;
                }
                (_, KeyCode::Char('h')) => {
                    let nv = !s.h65_enabled; s.h65_enabled = nv;
                    s.add_log(format!("Houdini 65: {}", if nv{"ON"}else{"OFF"}), if nv{Color::Green}else{Color::DarkGray});
                    http_post("/api/odiseo/variant", &format!("{{\"index\":1,\"enable\":{nv}}}")).await;
                }
                (_, KeyCode::Char('-')) | (_, KeyCode::Char('_')) => {
                    let idx = s.selected_variant;
                    let amt = if idx == 0 { s.odi_budget - 5.0 } else { s.h65_budget - 5.0 };
                    let amt = amt.max(1.0);
                    if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                    s.add_log(format!("Budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                }
                (_, KeyCode::Char('=')) | (_, KeyCode::Char('+')) => {
                    let idx = s.selected_variant;
                    let amt = if idx == 0 { s.odi_budget + 5.0 } else { s.h65_budget + 5.0 };
                    let amt = amt.min(200.0);
                    if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                    s.add_log(format!("Budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                }
                (_, KeyCode::Char('b')) => {
                    s.input_mode = InputMode::Budget; s.input_buf.clear();
                    let idx = s.selected_variant;
                    let cur = if idx == 0 { s.odi_budget } else { s.h65_budget };
                    s.input_buf = format!("{:.0}", cur);
                }
                (_, KeyCode::Char('j')) => { s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 }; }
                (_, KeyCode::Char('k')) => { s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 }; }
                (_, KeyCode::Up) => { s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 }; }
                (_, KeyCode::Down) => { s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 }; }

                // ── BUDGET PRESETS: digits in tabs 0,2,3 (not Trading) ──
                (0|2|3, KeyCode::Char(c @ ('1'..='9' | '0'))) => {
                    let presets: [(char, f64); 10] = [
                        ('1', 5.0), ('2', 10.0), ('3', 15.0), ('4', 20.0),
                        ('5', 30.0), ('6', 50.0), ('7', 75.0), ('8', 100.0),
                        ('9', 150.0), ('0', 200.0),
                    ];
                    if let Some(&(_, amt)) = presets.iter().find(|&&(k, _)| k == c) {
                        let idx = s.selected_variant;
                        if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                        let name = if idx == 0 { "O83" } else { "H65" };
                        s.add_log(format!("{} budget ${:.0}", name, amt), Color::Cyan);
                        http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                    }
                }

                // ── TRADING TAB (1) ─────────────────────────────────
                (1, KeyCode::Char('a')) => {
                    s.add_log("All variants ON", Color::Green);
                    for i in 0..13 { http_post("/api/odiseo/variant", &format!("{{\"index\":{i},\"enable\":true}}")).await; }
                    s.odi_enabled = true; s.h65_enabled = true;
                    http_post("/api/odiseo/filters", "{\"all\":true}").await;
                }
                (1, KeyCode::Char('z')) => {
                    s.add_log("All filters OFF", Color::Red);
                    http_post("/api/odiseo/filters", "{\"all\":false}").await;
                }
                (1, KeyCode::Char('t')) => {
                    let idx = s.selected_variant;
                    let name = if idx == 0 { "Odiseo 83" } else { "Houdini 65" };
                    s.add_log(format!("Only {}", name), Color::Yellow);
                    for i in 0..13 {
                        let on = i == idx;
                        http_post("/api/odiseo/variant", &format!("{{\"index\":{i},\"enable\":{on}}}")).await;
                    }
                    s.odi_enabled = idx == 0; s.h65_enabled = idx == 1;
                }
                (1, KeyCode::Char('[')) => {
                    let idx = s.selected_variant;
                    let amt = if idx == 0 { s.odi_budget - 5.0 } else { s.h65_budget - 5.0 };
                    let amt = amt.max(5.0);
                    if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                    s.add_log(format!("Budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                }
                (1, KeyCode::Char(']')) => {
                    let idx = s.selected_variant;
                    let amt = if idx == 0 { s.odi_budget + 5.0 } else { s.h65_budget + 5.0 };
                    let amt = amt.min(100.0);
                    if idx == 0 { s.odi_budget = amt; } else { s.h65_budget = amt; }
                    s.add_log(format!("Budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":{idx},\"amount\":{amt}}}")).await;
                }
                // Filter toggles (only in Trading tab)
                (1, KeyCode::Char(c @ ('1'..='9' | '0'))) => {
                    if let Some(&(_, name)) = FILTER_KEYS.iter().find(|&&(k,_)| k == c) {
                        let bit = 1u16 << FILTER_KEYS.iter().position(|&(k,_)| k == c).unwrap();
                        let currently_on = s.odi_filters & bit != 0;
                        let enable = !currently_on;
                        s.add_log(format!("Filter {}: {}", name, if enable {"ON"}else{"OFF"}), if enable {Color::Yellow}else{Color::DarkGray});
                        http_post("/api/odiseo/filters", &format!("{{\"name\":\"{}\",\"enable\":{}}}", name, enable)).await;
                    }
                }

                // ── SESSIONS TAB ────────────────────────────────────
                (2, KeyCode::Char('s')) => {
                    s.add_log("Starting 15-min indefinite session...", Color::Green);
                    let now = chrono::Utc::now();
                    let name = now.format("BTC15-Manual-%Y%m%dT%H%M").to_string();
                    let body = format!(r#"{{"name":"{}","duration_min":15,"depth_levels":50,"indefinite":true}}"#, name);
                    http_post("/api/sessions/start", &body).await;
                }
                (2, KeyCode::Char('S')) => {
                    let stop_id = s.sessions.get(s.selected_session)
                        .filter(|sess| sess.status == "recording")
                        .map(|sess| sess.id);
                    if let Some(id) = stop_id {
                        s.add_log(format!("Stopping session #{}", id), Color::Yellow);
                        let path = format!("/api/sessions/{}/stop", id);
                        http_post(&path, "{}").await;
                    }
                }
                (2, KeyCode::Char('e')) => {
                    if let Some(sess) = s.sessions.get(s.selected_session) {
                        let url = format!("{}/api/sessions/{}/export", API_URL, sess.id);
                        s.add_log(format!("Export: {}", url), Color::Cyan);
                    }
                }

                // ── SIGNALS TAB ────────────────────────────────────
                (3, KeyCode::Char(' ')) => {
                    s.add_log("Signal view refresh toggle", Color::Gray);
                }

                _ => {}
            }
        }

        terminal.draw(|f| ui::draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
