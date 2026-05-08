/// ZZIGNAL MONITOR v0.5 — Odiseo 83 + Houdini 65 TUI + Paper tracking
/// Teclas: p=PANIC  r=reinvertir  l=LIVE/PAPER  q=salir  c=copiar log
use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use chrono::Local;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

const WS_URL: &str = "ws://localhost:8080/ws";
const API_URL: &str = "http://localhost:8080";

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct WsMsg {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    balance: Option<f64>,
    status: Option<String>,
    btc: Option<f64>,
    price: Option<f64>,
    success: Option<bool>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OdiseoStatus {
    live_mode: bool,
    reinvest: Option<bool>,
    variants: Vec<OdiseoVariant>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct OdiseoVariant {
    enabled: Option<bool>,
    name: Option<String>,
    code: Option<String>,
    #[serde(default)]
    budget: f64,
    #[serde(default)]
    total_pnl: f64,
    #[serde(default)]
    balance: f64,
    #[serde(default)]
    trades_up: i64,
    #[serde(default)]
    trades_dn: i64,
    #[serde(default)]
    wins_up: i64,
    #[serde(default)]
    wins_dn: i64,
    #[serde(default)]
    tp_up: i64,
    #[serde(default)]
    tp_dn: i64,
    #[serde(default)]
    sl_up: i64,
    #[serde(default)]
    sl_dn: i64,
    #[serde(default)]
    sessions: i64,
    #[serde(default)]
    accuracy: f64,
    #[serde(default)]
    avg_pnl: f64,
    #[serde(default)]
    best: f64,
    #[serde(default)]
    worst: f64,
}

#[derive(Debug, Deserialize, Default)]
struct BtcInfo {
    price: f64,
    open: f64,
}

#[derive(Clone)]
struct LogEntry {
    ts: String,
    text: String,
    color: Color,
}

struct State {
    connected: bool,
    btc: f64,
    btc_open: f64,
    btc_entry: f64,
    bal: f64,
    live: bool,
    reinvest: bool,
    // ─── Odiseo (dynamic variant) ─────────────────
    odi_label: String, odi_code: String,
    odi_pnl: f64, odi_bal: f64, odi_budget: f64,
    odi_t_up: i64, odi_t_dn: i64, odi_w_up: i64, odi_w_dn: i64,
    odi_tp_up: i64, odi_tp_dn: i64, odi_sl_up: i64, odi_sl_dn: i64,
    odi_sessions: i64, odi_enabled: bool,
    odi_accuracy: f64, odi_avg_pnl: f64, odi_best: f64, odi_worst: f64,
    last_odi_t_up: i64, last_odi_t_dn: i64,
    // ─── Houdini 65 ────────────────────────────────
    h65_pnl: f64, h65_bal: f64, h65_budget: f64,
    h65_t_up: i64, h65_t_dn: i64, h65_w_up: i64, h65_w_dn: i64,
    h65_tp_up: i64, h65_tp_dn: i64, h65_sl_up: i64, h65_sl_dn: i64,
    h65_sessions: i64, h65_enabled: bool,
    h65_accuracy: f64, h65_avg_pnl: f64, h65_best: f64, h65_worst: f64,
    last_h65_t_up: i64, last_h65_t_dn: i64,
    // ─── general ───────────────────────────────────
    orders: i64,
    log: VecDeque<LogEntry>,
    last_poll: Instant, last_btc_poll: Instant, last_ws: Instant,
    warnings: VecDeque<String>,
}

impl State {
    fn new() -> Self {
        Self {
            connected: false, btc: 0.0, btc_open: 0.0, btc_entry: 0.0, bal: 0.0, live: false, reinvest: false,
            odi_label: "Odiseo".into(), odi_code: String::new(),
            odi_pnl: 0.0, odi_bal: 0.0, odi_budget: 20.0,
            odi_t_up: 0, odi_t_dn: 0, odi_w_up: 0, odi_w_dn: 0,
            odi_tp_up: 0, odi_tp_dn: 0, odi_sl_up: 0, odi_sl_dn: 0,
            odi_sessions: 0, odi_enabled: false, odi_accuracy: 0.0,
            odi_avg_pnl: 0.0, odi_best: 0.0, odi_worst: 0.0,
            last_odi_t_up: 0, last_odi_t_dn: 0,
            h65_pnl: 0.0, h65_bal: 0.0, h65_budget: 20.0,
            h65_t_up: 0, h65_t_dn: 0, h65_w_up: 0, h65_w_dn: 0,
            h65_tp_up: 0, h65_tp_dn: 0, h65_sl_up: 0, h65_sl_dn: 0,
            h65_sessions: 0, h65_enabled: true, h65_accuracy: 0.0,
            h65_avg_pnl: 0.0, h65_best: 0.0, h65_worst: 0.0,
            last_h65_t_up: 0, last_h65_t_dn: 0,
            orders: 0, log: VecDeque::with_capacity(100),
            last_poll: Instant::now(), last_btc_poll: Instant::now(), last_ws: Instant::now(),
            warnings: VecDeque::with_capacity(20),
        }
    }
    fn add_log(&mut self, text: String, color: Color) {
        let ts = Local::now().format("%H:%M:%S").to_string();
        self.log.push_front(LogEntry { ts, text, color });
        if self.log.len() > 100 { self.log.pop_back(); }
    }
    fn add_warning(&mut self, text: String) {
        let ts = Local::now().format("%H:%M:%S").to_string();
        self.warnings.push_front(format!("{} {}", ts, text));
        if self.warnings.len() > 20 { self.warnings.pop_back(); }
    }
    async fn http_post(&self, path: &str, body: &str) {
        let _ = reqwest::Client::new()
            .post(format!("{API_URL}{path}"))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send().await;
    }
}

fn find_variant<'a>(variants: &'a [OdiseoVariant], code: &str) -> Option<&'a OdiseoVariant> {
    variants.iter().find(|v| v.code.as_deref() == Some(code))
}

fn detect_trades(v: &OdiseoVariant, last_t_up: i64, last_t_dn: i64,
                 s: &mut State, label: &str) -> (i64, i64) {
    let delta_up = v.trades_up - last_t_up;
    let delta_dn = v.trades_dn - last_t_dn;
    if delta_up > 0 {
        let mode = if s.live { "LIVE" } else { "PAPER" };
        s.btc_entry = s.btc;
        s.add_log(format!("⬆ ENTER UP  [{} {}]  BTC ${:.0}", label, mode, s.btc), Color::Green);
    }
    if delta_dn > 0 {
        let mode = if s.live { "LIVE" } else { "PAPER" };
        s.btc_entry = s.btc;
        s.add_log(format!("⬇ ENTER DN  [{} {}]  BTC ${:.0}", label, mode, s.btc), Color::Red);
    }
    if delta_up < 0 {
        let mode = if s.live { "LIVE" } else { "PAPER" };
        s.add_log(format!("⬆ EXIT  UP  [{} {}]", label, mode), Color::Yellow);
    }
    if delta_dn < 0 {
        let mode = if s.live { "LIVE" } else { "PAPER" };
        s.add_log(format!("⬇ EXIT  DN  [{} {}]", label, mode), Color::Yellow);
    }
    (v.trades_up, v.trades_dn)
}

fn apply_variant(v: &OdiseoVariant, s: &mut State) {
    match v.code.as_deref() {
        Some(c) if c.starts_with("odiseo") && c != "odiseo65" => {
            let new_pnl = v.total_pnl;
            let delta = new_pnl - s.odi_pnl;
            if delta.abs() > 0.0001 && s.odi_pnl != 0.0 {
                s.add_log(format!("{} PnL {:+.4} ({:+.4})", v.name.as_deref().unwrap_or("Odi"), new_pnl, delta), if delta > 0.0 { Color::Green } else { Color::Red });
            }
            s.odi_code = c.to_string();
            s.odi_label = v.name.clone().unwrap_or_else(|| "Odiseo".into());
            s.odi_pnl = new_pnl;
            s.odi_budget = v.budget;
            s.odi_bal = v.balance;
            s.odi_t_up = v.trades_up;
            s.odi_t_dn = v.trades_dn;
            s.odi_w_up = v.wins_up;
            s.odi_w_dn = v.wins_dn;
            s.odi_tp_up = v.tp_up;
            s.odi_tp_dn = v.tp_dn;
            s.odi_sl_up = v.sl_up;
            s.odi_sl_dn = v.sl_dn;
            s.odi_sessions = v.sessions;
            s.odi_accuracy = v.accuracy;
            s.odi_avg_pnl = v.avg_pnl;
            s.odi_best = v.best;
            s.odi_worst = v.worst;
            s.odi_enabled = v.enabled.unwrap_or(true);
            let label = s.odi_label.clone();
            let (nu, nd) = detect_trades(v, s.last_odi_t_up, s.last_odi_t_dn, s, &label);
            s.last_odi_t_up = nu;
            s.last_odi_t_dn = nd;
            // Warnings
            let total_sl = v.sl_up + v.sl_dn;
            let total_trades = v.trades_up + v.trades_dn;
            if total_trades > 0 && total_sl >= 3 {
                s.add_warning(format!("{} ALERTA: {} SLs acumulados", s.odi_label, total_sl));
            }
            if v.total_pnl < -v.budget * 0.1 {
                s.add_warning(format!("{} PERDIDA >10% del budget (${:.2})", s.odi_label, v.budget));
            }
            if !v.enabled.unwrap_or(true) {
                s.add_warning(format!("{} DESACTIVADO", s.odi_label));
            }
        }
        Some("houdini65") => {
            let new_pnl = v.total_pnl;
            let delta = new_pnl - s.h65_pnl;
            if delta.abs() > 0.0001 && s.h65_pnl != 0.0 {
                s.add_log(format!("H65 PnL {:+.4} ({:+.4})", new_pnl, delta), if delta > 0.0 { Color::Green } else { Color::Red });
            }
            s.h65_pnl = new_pnl;
            s.h65_budget = v.budget;
            s.h65_bal = v.balance;
            s.h65_t_up = v.trades_up;
            s.h65_t_dn = v.trades_dn;
            s.h65_w_up = v.wins_up;
            s.h65_w_dn = v.wins_dn;
            s.h65_tp_up = v.tp_up;
            s.h65_tp_dn = v.tp_dn;
            s.h65_sl_up = v.sl_up;
            s.h65_sl_dn = v.sl_dn;
            s.h65_sessions = v.sessions;
            s.h65_accuracy = v.accuracy;
            s.h65_avg_pnl = v.avg_pnl;
            s.h65_best = v.best;
            s.h65_worst = v.worst;
            s.h65_enabled = v.enabled.unwrap_or(true);
            let (nu, nd) = detect_trades(v, s.last_h65_t_up, s.last_h65_t_dn, s, "H65");
            s.last_h65_t_up = nu;
            s.last_h65_t_dn = nd;
            // Warnings H65
            let total_sl = v.sl_up + v.sl_dn;
            let total_trades = v.trades_up + v.trades_dn;
            if total_trades > 0 && total_sl >= 3 {
                s.add_warning(format!("H65 ALERTA: {} SLs acumulados", total_sl));
            }
            if v.total_pnl < -v.budget * 0.1 {
                s.add_warning(format!("H65 PERDIDA >10% del budget (${:.2})", v.budget));
            }
            if !v.enabled.unwrap_or(true) {
                s.add_warning("HOUDINI 65 DESACTIVADO".into());
            }
        }
        _ => {}
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let (tx, mut rx) = mpsc::channel::<WsMsg>(256);

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

    let (itx, mut irx) = mpsc::channel::<KeyCode>(16);
    tokio::spawn(async move { loop { if let Ok(Event::Key(k)) = event::read() { let _ = itx.send(k.code).await; } } });

    let mut s = State::new();
    s.add_log(format!("ZZIGNAL MONITOR v0.6 — Odiseo + Houdini 65 — {} filtros", 12).into(), Color::Magenta);

    loop {
        while let Ok(msg) = rx.try_recv() {
            s.last_ws = Instant::now();
            match msg.msg_type.as_deref() {
                Some("connected") => { s.connected = true; s.add_log("WS OK".into(), Color::Green); }
                Some("snapshot") => { s.bal = msg.balance.unwrap_or(s.bal); s.btc = msg.btc.unwrap_or(s.btc); }
                Some("btc_price") => s.btc = msg.price.unwrap_or(s.btc),
                Some("balance") => {
                    let old = s.bal; s.bal = msg.balance.unwrap_or(s.bal);
                    if (s.bal - old).abs() > 0.01 {
                        s.add_log(format!("USD ${:.2} ({:+.2})", s.bal, s.bal - old), if s.bal > old { Color::Green } else { Color::Red });
                    }
                }
                Some("order_result") => {
                    let ok = msg.success.unwrap_or(false);
                    let txt = msg.message.unwrap_or_default();
                    s.add_log(format!("{} {}", if ok { "OK" } else { "FAIL" }, txt), if ok { Color::Green } else { Color::Red });
                }
                _ => {}
            }
        }

        if s.last_poll.elapsed() > Duration::from_secs(2) {
            s.last_poll = Instant::now();
            if let Ok(resp) = reqwest::get(format!("{API_URL}/api/odiseo/status")).await {
                if let Ok(data) = resp.json::<OdiseoStatus>().await {
                    s.live = data.live_mode;
                    s.reinvest = data.reinvest.unwrap_or(false);

                    // Find any active Odiseo variant (not Houdini/Wide 65)
                    let odi_v = data.variants.iter().find(|v| {
                        let code = v.code.as_deref().unwrap_or("");
                        code.starts_with("odiseo") && code != "odiseo65"
                    });
                    if let Some(v) = odi_v {
                        apply_variant(v, &mut s);
                    }
                    if let Some(v) = find_variant(&data.variants, "houdini65") {
                        apply_variant(v, &mut s);
                    }
                }
            }
            if let Ok(resp) = reqwest::get(format!("{API_URL}/api/orders")).await {
                if let Ok(orders) = resp.json::<Vec<serde_json::Value>>().await {
                    let new_count = orders.len() as i64;
                    if new_count != s.orders {
                        s.add_log(format!("Ordenes: {} -> {}", s.orders, new_count), Color::Cyan);
                    }
                    s.orders = new_count;
                }
            }
        }

        if s.last_btc_poll.elapsed() > Duration::from_secs(5) {
            s.last_btc_poll = Instant::now();
            if let Ok(resp) = reqwest::get(format!("{API_URL}/api/btc")).await {
                if let Ok(data) = resp.json::<BtcInfo>().await {
                    s.btc = data.price;
                    if s.btc_open == 0.0 { s.btc_open = data.open; }
                }
            }
        }

        while let Ok(k) = irx.try_recv() {
            match k {
                KeyCode::Char('q') | KeyCode::Esc => {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                KeyCode::Char('c') => {
                    let dump: String = s.log.iter().map(|e| format!("{} {}\n", e.ts, e.text)).collect();
                    let _ = std::fs::write("/tmp/zzignal_log.txt", dump);
                    s.add_log("Log copiado a /tmp/zzignal_log.txt".into(), Color::Cyan);
                }
                KeyCode::Char('p') => {
                    s.add_log("PANIC SELL!".into(), Color::Red);
                    s.http_post("/api/panic", "{}").await;
                }
                KeyCode::Char('r') => {
                    let nv = !s.reinvest; s.reinvest = nv;
                    s.add_log(format!("Reinvest: {}", if nv {"ON"}else{"OFF"}), Color::Yellow);
                    s.http_post("/api/odiseo/reinvest", &format!("{{\"enable\":{nv}}}")).await;
                }
                KeyCode::Char('l') => {
                    let nv = !s.live;
                    s.add_log(format!("LIVE: {}", if nv {"ON"}else{"OFF"}), if nv {Color::Red}else{Color::Cyan});
                    s.http_post("/api/odiseo/live", &format!("{{\"enable\":{nv}}}")).await;
                }
                _ => {}
            }
        }

        terminal.draw(|f| draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn draw(f: &mut Frame, s: &State) {
    let warn_h = if s.warnings.is_empty() { 0 } else { (s.warnings.len().min(3) as u16).max(1) };

    let constraints: Vec<Constraint> = vec![
        Constraint::Length(3),    // header
        Constraint::Length(1),    // banner (always visible)
        Constraint::Length(7),    // Odiseo 83 panel
        Constraint::Length(7),    // Houdini 65 panel
        Constraint::Length(warn_h), // warnings
        Constraint::Min(2),       // log
        Constraint::Length(2),    // footer
    ];

    let m = Layout::default().direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    let mut idx = 0;

    // ─── HEADER ─────────────────────────────────────────────────────────
    let h = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,7); 7]).split(m[idx]); idx += 1;

    // BTC
    let btc_ref = if s.btc_entry > 0.0 { s.btc_entry } else { s.btc_open };
    let btc_delta = if btc_ref > 0.0 { s.btc - btc_ref } else { 0.0 };
    let btc_delta_pct = if btc_ref > 0.0 { btc_delta / btc_ref * 100.0 } else { 0.0 };
    let btc_c = if btc_delta > 0.0 { Color::Green } else if btc_delta < 0.0 { Color::Red } else { Color::Yellow };
    let entry_label = if s.btc_entry > 0.0 { format!("ent {}", s.btc_entry as i64) } else { "open".into() };
    let btc_txt = format!("BTC ${:.0} ({}{:+.0} {:+.1}%)", s.btc, entry_label, btc_delta, btc_delta_pct);
    f.render_widget(Paragraph::new(btc_txt).style(Style::default().fg(btc_c).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[0]);

    // USD
    let bal_c = if s.bal > s.odi_budget.max(s.h65_budget) { Color::Green } else if s.bal > s.odi_budget.max(s.h65_budget) * 0.8 { Color::Yellow } else { Color::Red };
    f.render_widget(Paragraph::new(format!("USD ${:.2}", s.bal)).style(Style::default().fg(bal_c).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[1]);

    // MODE: always PAPER (cyan) or LIVE (red)
    let mode_style = if s.live {
        Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
    };
    f.render_widget(Paragraph::new(if s.live {" LIVE "}else{"PAPER"}).style(mode_style).block(Block::default().borders(Borders::ALL)), h[2]);

    // Odiseo dynamic status
    let odi_short = if s.odi_label.len() > 10 { &s.odi_label[..10] } else { &s.odi_label };
    let o83_st = if s.odi_enabled { if s.live { Color::Red } else { Color::Green } } else { Color::DarkGray };
    f.render_widget(Paragraph::new(if s.odi_enabled { format!("{} ON", odi_short) } else { format!("{} OFF", odi_short) }).style(Style::default().fg(o83_st).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[3]);

    // Houdini 65 status
    let h65_st = if s.h65_enabled { if s.live { Color::Red } else { Color::Green } } else { Color::DarkGray };
    f.render_widget(Paragraph::new(if s.h65_enabled {"H65 ON"}else{"H65 OFF"}).style(Style::default().fg(h65_st).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[4]);

    // WS
    let wsc = if s.connected { Color::Green } else { Color::Red };
    f.render_widget(Paragraph::new(if s.connected {"WS OK"}else{"WS OFF"}).style(Style::default().fg(wsc).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[5]);

    // Reinv
    let reinv_c = if s.reinvest { Color::Green } else { Color::DarkGray };
    f.render_widget(Paragraph::new(if s.reinvest {"Reinv ON"}else{"Reinv OFF"}).style(Style::default().fg(reinv_c).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[6]);

    // ─── BANNER: always visible ──────────────────────────────────────────
    if s.live {
        let live_banner = Paragraph::new(format!(" DINERO REAL ACTIVO — {} + HOUDINI 65 EN VIVO ", s.odi_label))
            .style(Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD));
        f.render_widget(live_banner, m[idx]);
    } else {
        let paper_banner = Paragraph::new(" PAPER MONEY — GRABANDO — Sin dinero real ")
            .style(Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD));
        f.render_widget(paper_banner, m[idx]);
    }
    idx += 1;

    // ─── ODISEO PANEL (dynamic variant) ───────────────────────────────────
    {
        let odi = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(m[idx]);
        let t = s.odi_t_up + s.odi_t_dn;
        let w = s.odi_w_up + s.odi_w_dn;
        let wr = if t > 0 { format!("{:.0}%", w as f64 / t as f64 * 100.0) } else { "-".into() };
        let pnl_pct = if s.odi_budget > 0.0 { s.odi_pnl / s.odi_budget * 100.0 } else { 0.0 };
        let pc = if s.odi_pnl > 0.001 { Color::Green } else if s.odi_pnl < -0.001 { Color::Red } else { Color::Gray };
        let bc = if s.live && s.odi_enabled { Color::Red } else { Color::DarkGray };
        let tp_total = s.odi_tp_up + s.odi_tp_dn;
        let sl_total = s.odi_sl_up + s.odi_sl_dn;

        let stats = format!(
            "Budget: ${:.0}   Balance: ${:.2}   PnL: {:+.4} ({:+.1}%)\n\
             Win Rate: {} ({}/{} trades)   Accuracy: {:.0}%\n\
             Sessions: {}   Avg PnL: {:+.4}   Best: {:+.4}   Worst: {:+.4}",
            s.odi_budget, s.odi_bal, s.odi_pnl, pnl_pct,
            wr, w, t, s.odi_accuracy * 100.0,
            s.odi_sessions, s.odi_avg_pnl, s.odi_best, s.odi_worst,
        );
        f.render_widget(Paragraph::new(stats).style(Style::default().fg(pc)).block(Block::default().borders(Borders::ALL).title(format!("{} — Estrategia Principal", s.odi_label)).border_style(Style::default().fg(bc))), odi[0]);

        let exit_info = format!(
            "ENTRADAS / SALIDAS\n\
             UP:   {}/{} trades   won {}/{}   TP {}   SL {}\n\
             DOWN: {}/{} trades   won {}/{}   TP {}   SL {}\n\
             TOTAL: {} trades   {} TP   {} SL",
            s.odi_w_up, s.odi_t_up, s.odi_w_up, s.odi_t_up, s.odi_tp_up, s.odi_sl_up,
            s.odi_w_dn, s.odi_t_dn, s.odi_w_dn, s.odi_t_dn, s.odi_tp_dn, s.odi_sl_dn,
            t, tp_total, sl_total,
        );
        f.render_widget(Paragraph::new(exit_info).style(Style::default().fg(Color::Gray)).block(Block::default().borders(Borders::ALL).title("Detalle UP/DOWN")), odi[1]);
    }
    idx += 1;

    // ─── HOUDINI 65 PANEL ────────────────────────────────────────────────
    {
        let odi = Layout::default().direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1,2), Constraint::Ratio(1,2)]).split(m[idx]);
        let t = s.h65_t_up + s.h65_t_dn;
        let w = s.h65_w_up + s.h65_w_dn;
        let wr = if t > 0 { format!("{:.0}%", w as f64 / t as f64 * 100.0) } else { "-".into() };
        let pnl_pct = if s.h65_budget > 0.0 { s.h65_pnl / s.h65_budget * 100.0 } else { 0.0 };
        let pc = if s.h65_pnl > 0.001 { Color::Green } else if s.h65_pnl < -0.001 { Color::Red } else { Color::Gray };
        let bc = if s.live && s.h65_enabled { Color::Red } else { Color::DarkGray };
        let tp_total = s.h65_tp_up + s.h65_tp_dn;
        let sl_total = s.h65_sl_up + s.h65_sl_dn;

        let stats = format!(
            "Budget: ${:.0}   Balance: ${:.2}   PnL: {:+.4} ({:+.1}%)\n\
             Win Rate: {} ({}/{} trades)   Accuracy: {:.0}%\n\
             Entry: >=0.65   TP:0.75(+15.4%)   SL:0.60(-7.7%)\n\
             Sessions: {}   Avg PnL: {:+.4}   Best: {:+.4}   Worst: {:+.4}",
            s.h65_budget, s.h65_bal, s.h65_pnl, pnl_pct,
            wr, w, t, s.h65_accuracy * 100.0,
            s.h65_sessions, s.h65_avg_pnl, s.h65_best, s.h65_worst,
        );
        f.render_widget(Paragraph::new(stats).style(Style::default().fg(pc)).block(Block::default().borders(Borders::ALL).title("Houdini 65 — Reversals").border_style(Style::default().fg(bc))), odi[0]);

        let exit_info = format!(
            "ENTRADAS / SALIDAS\n\
             UP:   {}/{} trades   won {}/{}   TP {}   SL {}\n\
             DOWN: {}/{} trades   won {}/{}   TP {}   SL {}\n\
             TOTAL: {} trades   {} TP   {} SL",
            s.h65_w_up, s.h65_t_up, s.h65_w_up, s.h65_t_up, s.h65_tp_up, s.h65_sl_up,
            s.h65_w_dn, s.h65_t_dn, s.h65_w_dn, s.h65_t_dn, s.h65_tp_dn, s.h65_sl_dn,
            t, tp_total, sl_total,
        );
        f.render_widget(Paragraph::new(exit_info).style(Style::default().fg(Color::Gray)).block(Block::default().borders(Borders::ALL).title("Detalle UP/DOWN H65")), odi[1]);
    }
    idx += 1;

    // ─── WARNINGS ───────────────────────────────────────────────────────
    if warn_h > 0 {
        let warn_lines: Vec<Line> = s.warnings.iter().take(3).map(|w|
            Line::from(Span::styled(w, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        ).collect();
        f.render_widget(Paragraph::new(warn_lines).block(Block::default().borders(Borders::ALL).title("Alertas").border_style(Style::default().fg(Color::Red))), m[idx]); idx += 1;
    }

    // ─── LOG ────────────────────────────────────────────────────────────
    let lines: Vec<Line> = s.log.iter().map(|e| Line::from(vec![
        Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
        Span::styled(&e.text, Style::default().fg(e.color)),
    ])).collect();
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Eventos")), m[idx]); idx += 1;

    let lag_ms = s.last_ws.elapsed().as_millis();
    let footer = format!(
        "WS: {}ms  |  ZZIGNAL MONITOR v0.6  |  [l]LIVE [r]Reinv [p]PANIC [c]Log [q]Salir  |  zz-go-h para filtros",
        lag_ms
    );
    f.render_widget(Paragraph::new(footer).style(Style::default().fg(Color::DarkGray)), m[idx]);
}
