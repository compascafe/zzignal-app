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

const ODI_BUDGETS: [f64; 4] = [5.0, 10.0, 20.0, 40.0];
const H65_BUDGETS: [f64; 4] = [5.0, 10.0, 20.0, 40.0];

struct State {
    connected: bool,
    tab: usize,
    btc: f64, btc_open: f64, btc_entry: f64,
    bal: f64,
    live: bool, reinvest: bool,

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

    orders: i64,
    log: VecDeque<api::LogEntry>,
    warnings: VecDeque<String>,
    last_poll_odiseo: Instant,
    last_poll_btc: Instant,
    last_poll_hft: Instant,
    last_poll_sessions: Instant,
    last_ws: Instant,
}

impl State {
    fn new() -> Self {
        Self {
            connected: false, tab: 0,
            btc: 0.0, btc_open: 0.0, btc_entry: 0.0, bal: 0.0,
            live: false, reinvest: false,
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
            orders: 0,
            log: VecDeque::with_capacity(100),
            warnings: VecDeque::with_capacity(20),
            last_poll_odiseo: Instant::now(),
            last_poll_btc: Instant::now(),
            last_poll_hft: Instant::now(),
            last_poll_sessions: Instant::now(),
            last_ws: Instant::now(),
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

    let mut s = State::new();
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
                s.hft = data;
                // Track filter bitmask from hft data
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
            match (s.tab, k) {
                // ── GLOBAL ──────────────────────────────────────
                (_, KeyCode::Esc) | (_, KeyCode::Char('q')) => {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                (_, KeyCode::Right) | (_, KeyCode::Tab) => { s.tab = (s.tab + 1) % 4; }
                (_, KeyCode::Left)  => { s.tab = if s.tab == 0 { 3 } else { s.tab - 1 }; }

                // ── DASHBOARD + GLOBAL ─────────────────────────
                (0|1, KeyCode::Char('l')) => {
                    let nv = !s.live;
                    s.add_log(format!("LIVE: {}", if nv{"ON"}else{"OFF"}), if nv{Color::Red}else{Color::Cyan});
                    http_post("/api/odiseo/live", &format!("{{\"enable\":{nv}}}")).await;
                }
                (0|1, KeyCode::Char('r')) => {
                    let nv = !s.reinvest; s.reinvest = nv;
                    s.add_log(format!("Reinvest: {}", if nv{"ON"}else{"OFF"}), Color::Yellow);
                    http_post("/api/odiseo/reinvest", &format!("{{\"enable\":{nv}}}")).await;
                }
                (0|1, KeyCode::Char('p')) => {
                    s.add_log("PANIC SELL!", Color::Red);
                    http_post("/api/panic", "{}").await;
                }
                (0|1, KeyCode::Char('o')) => {
                    let nv = !s.odi_enabled; s.odi_enabled = nv;
                    s.add_log(format!("Odiseo 83: {}", if nv{"ON"}else{"OFF"}), if nv{Color::Green}else{Color::DarkGray});
                    http_post("/api/odiseo/variant", &format!("{{\"index\":0,\"enable\":{nv}}}")).await;
                }
                (0|1, KeyCode::Char('h')) => {
                    let nv = !s.h65_enabled; s.h65_enabled = nv;
                    s.add_log(format!("Houdini 65: {}", if nv{"ON"}else{"OFF"}), if nv{Color::Green}else{Color::DarkGray});
                    http_post("/api/odiseo/variant", &format!("{{\"index\":1,\"enable\":{nv}}}")).await;
                }

                // ── TRADING TAB ────────────────────────────────
                (1, KeyCode::Char('a')) => {
                    s.add_log("All variants ON", Color::Green);
                    for i in 0..13 { http_post("/api/odiseo/variant", &format!("{{\"index\":{i},\"enable\":true}}")).await; }
                    s.odi_enabled = true; s.h65_enabled = true;
                }
                (1, KeyCode::Char('z')) => {
                    s.add_log("All variants OFF", Color::Red);
                    for i in 0..13 { http_post("/api/odiseo/variant", &format!("{{\"index\":{i},\"enable\":false}}")).await; }
                    s.odi_enabled = false; s.h65_enabled = false;
                }
                (1, KeyCode::Char('t')) => {
                    // Only THIS variant
                    let idx = s.selected_variant;
                    let name = if idx == 0 { "Odiseo 83" } else { "Houdini 65" };
                    s.add_log(format!("Only {}", name), Color::Yellow);
                    for i in 0..13 {
                        let on = i == idx;
                        http_post("/api/odiseo/variant", &format!("{{\"index\":{i},\"enable\":{on}}}")).await;
                    }
                    s.odi_enabled = idx == 0; s.h65_enabled = idx == 1;
                }
                (1, KeyCode::Up) | (1, KeyCode::Char('k')) => {
                    s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 };
                }
                (1, KeyCode::Down) | (1, KeyCode::Char('j')) => {
                    s.selected_variant = if s.selected_variant == 0 { 1 } else { 0 };
                }
                // Budget: 1-4 = Odiseo, 5-8 = Houdini
                (1, KeyCode::Char(c @ '1'..='4')) => {
                    let idx = (c as u8 - b'1') as usize;
                    let amt = ODI_BUDGETS[idx];
                    s.odi_budget = amt;
                    s.add_log(format!("Odiseo budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":0,\"amount\":{amt}}}")).await;
                }
                (1, KeyCode::Char(c @ '5'..='8')) => {
                    let idx = (c as u8 - b'5') as usize;
                    let amt = H65_BUDGETS[idx];
                    s.h65_budget = amt;
                    s.add_log(format!("H65 budget: ${:.0}", amt), Color::Yellow);
                    http_post("/api/odiseo/budget", &format!("{{\"index\":1,\"amount\":{amt}}}")).await;
                }
                (1, KeyCode::Char('f')) => {
                    // Toggle all filters on/off
                    let nv = s.odi_filters == 0;
                    s.add_log(format!("Filters: {}", if nv {"ALL ON"}else{"ALL OFF"}), Color::Yellow);
                    if nv {
                        http_post("/api/odiseo/filters", "{\"enable\":true}").await;
                    } else {
                        http_post("/api/odiseo/filters", "{\"enable\":false}").await;
                    }
                }

                // ── SESSIONS TAB ───────────────────────────────
                (2, KeyCode::Up) | (2, KeyCode::Char('k')) => {
                    if s.selected_session > 0 { s.selected_session -= 1; }
                }
                (2, KeyCode::Down) | (2, KeyCode::Char('j')) => {
                    if s.selected_session + 1 < s.sessions.len() { s.selected_session += 1; }
                }
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

                // ── SIGNALS TAB ────────────────────────────────
                (3, KeyCode::Char(' ')) => {
                    // Pause/resume signal polling
                    s.add_log("Signal view refresh toggle", Color::Gray);
                }

                _ => {}
            }
        }

        terminal.draw(|f| ui::draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
