/// ZZIGNAL Monitor — TUI real-time para Odiseo 85
/// Teclas: p=PANIC  r=reinvertir  l=LIVE/PAPER  q=salir
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
use ratatui::{Frame, Terminal};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;

const WS_URL: &str = "ws://localhost:8080/ws";
const API_URL: &str = "http://localhost:8080";

#[derive(Debug, Deserialize, Default)]
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

#[derive(Debug, Deserialize)]
struct OdiseoVariant {
    enabled: Option<bool>,
    name: Option<String>,
    budget: Option<f64>,
    total_pnl: Option<f64>,
    balance: Option<f64>,
    trades_up: Option<i64>,
    trades_dn: Option<i64>,
    wins_up: Option<i64>,
    wins_dn: Option<i64>,
    sessions: Option<i64>,
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
    bal: f64,
    live: bool,
    reinvest: bool,
    odi_pnl: f64,
    odi_bal: f64,
    odi_budget: f64,
    odi_t_up: i64,
    odi_t_dn: i64,
    odi_w_up: i64,
    odi_w_dn: i64,
    odi_sessions: i64,
    odi_enabled: bool,
    odi_active: bool,  // whether currently in a trade
    orders: i64,
    log: VecDeque<LogEntry>,
    last_poll: Instant,
    last_session: i64,
}

impl State {
    fn new() -> Self {
        Self {
            connected: false, btc: 0.0, bal: 0.0, live: false, reinvest: false,
            odi_pnl: 0.0, odi_bal: 0.0, odi_budget: 7.0,
            odi_t_up: 0, odi_t_dn: 0, odi_w_up: 0, odi_w_dn: 0,
            odi_sessions: 0, odi_enabled: true, odi_active: false, orders: 0,
            log: VecDeque::with_capacity(100), last_poll: Instant::now(), last_session: 0,
        }
    }
    fn add_log(&mut self, text: String, color: Color) {
        let ts = Local::now().format("%H:%M:%S").to_string();
        self.log.push_front(LogEntry { ts, text, color });
        if self.log.len() > 100 { self.log.pop_back(); }
    }
    async fn http_post(&self, path: &str, body: &str) {
        let _ = reqwest::Client::new()
            .post(format!("{API_URL}{path}"))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send().await;
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let (tx, mut rx) = mpsc::channel::<WsMsg>(256);

    // WS task
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

    // Input task
    let (itx, mut irx) = mpsc::channel::<KeyCode>(16);
    tokio::spawn(async move { loop { if let Ok(Event::Key(k)) = event::read() { let _ = itx.send(k.code).await; } } });

    let mut s = State::new();
    s.add_log("ZZIGNAL MONITOR v0.1".into(), Color::Magenta);

    loop {
        // WS messages
        while let Ok(msg) = rx.try_recv() {
            match msg.msg_type.as_deref() {
                Some("connected") => { s.connected = true; s.add_log("WS OK".into(), Color::Green); }
                Some("snapshot") => { s.bal = msg.balance.unwrap_or(s.bal); s.btc = msg.btc.unwrap_or(s.btc); }
                Some("btc_price") => s.btc = msg.price.unwrap_or(s.btc),
                Some("balance") => {
                    let old = s.bal; s.bal = msg.balance.unwrap_or(s.bal);
                    if (s.bal - old).abs() > 0.01 {
                        s.add_log(format!("💰 ${:.2} ({:+.2})", s.bal, s.bal - old), if s.bal > old { Color::Green } else { Color::Red });
                    }
                }
                Some("order_result") => {
                    let ok = msg.success.unwrap_or(false);
                    let txt = msg.message.unwrap_or_default();
                    s.add_log(format!("{} {}", if ok { "✅" } else { "❌" }, txt), if ok { Color::Green } else { Color::Red });
                }
                _ => {}
            }
        }

        // HTTP poll every 2s
        if s.last_poll.elapsed() > Duration::from_secs(2) {
            s.last_poll = Instant::now();
            // Odiseo status
            if let Ok(resp) = reqwest::get(format!("{API_URL}/api/odiseo/status")).await {
                if let Ok(data) = resp.json::<OdiseoStatus>().await {
                    s.live = data.live_mode;
                    s.reinvest = data.reinvest.unwrap_or(false);
                    if let Some(v) = data.variants.first() {
                        let new_pnl = v.total_pnl.unwrap_or(0.0);
                        let delta = new_pnl - s.odi_pnl;
                        if delta.abs() > 0.0001 {
                            s.add_log(format!("PnL {:+.4} ({:+.4})", new_pnl, delta), if delta > 0.0 { Color::Green } else { Color::Red });
                        }
                        s.odi_pnl = new_pnl;
                        s.odi_budget = v.budget.unwrap_or(7.0);
                        s.odi_bal = v.balance.unwrap_or(7.0);
                        s.odi_t_up = v.trades_up.unwrap_or(0);
                        s.odi_t_dn = v.trades_dn.unwrap_or(0);
                        s.odi_w_up = v.wins_up.unwrap_or(0);
                        s.odi_w_dn = v.wins_dn.unwrap_or(0);
                        s.odi_sessions = v.sessions.unwrap_or(0);
                        s.odi_enabled = v.enabled.unwrap_or(true);
                    }
                }
            }
            // Orders count
            if let Ok(resp) = reqwest::get(format!("{API_URL}/api/orders")).await {
                if let Ok(orders) = resp.json::<Vec<serde_json::Value>>().await {
                    s.orders = orders.len() as i64;
                }
            }
        }

        // Keyboard
        while let Ok(k) = irx.try_recv() {
            match k {
                KeyCode::Char('q') | KeyCode::Esc => {
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                KeyCode::Char('p') => {
                    s.add_log("🚨 PANIC SELL!".into(), Color::Red);
                    s.http_post("/api/panic", "{}").await;
                }
                KeyCode::Char('r') => {
                    let nv = !s.reinvest; s.reinvest = nv;
                    s.add_log(format!("🔄 Reinvest: {}", if nv {"ON"}else{"OFF"}), Color::Yellow);
                    let body = format!("{{\"enable\":{nv}}}");
                    s.http_post("/api/odiseo/reinvest", &body).await;
                }
                KeyCode::Char('l') => {
                    let nv = !s.live;
                    s.add_log(format!("⚡ LIVE: {}", if nv {"ON"}else{"OFF"}), if nv {Color::Red}else{Color::Cyan});
                    let body = format!("{{\"enable\":{nv}}}");
                    s.http_post("/api/odiseo/live", &body).await;
                }
                _ => {}
            }
        }

        // Draw
        terminal.draw(|f| draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn draw(f: &mut Frame, s: &State) {
    let m = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(8), Constraint::Min(1), Constraint::Length(2)])
        .split(f.area());

    // Header
    let h = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1,5); 5]).split(m[0]);

    let btc_c = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(format!("BTC ${:.0}", s.btc)).style(btc_c).block(Block::default().borders(Borders::ALL)), h[0]);

    let bal_c = if s.bal > 7.0 { Color::Green } else if s.bal > 5.0 { Color::Yellow } else { Color::Red };
    f.render_widget(Paragraph::new(format!("BAL ${:.2}", s.bal)).style(Style::default().fg(bal_c).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[1]);

    let lc = if s.live { Color::Red } else { Color::Gray };
    f.render_widget(Paragraph::new(if s.live {"⚡ LIVE"}else{"PAPER"}).style(Style::default().fg(lc).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL)), h[2]);

    let wsc = if s.connected { Color::Green } else { Color::Red };
    f.render_widget(Paragraph::new(if s.connected {"WS OK"}else{"WS OFF"}).style(Style::default().fg(wsc)).block(Block::default().borders(Borders::ALL)), h[3]);

    f.render_widget(Paragraph::new(Local::now().format("%H:%M:%S").to_string()).style(Style::default().fg(Color::Gray)).block(Block::default().borders(Borders::ALL)), h[4]);

    // Odiseo 85
    let odi = Layout::default().direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3,5), Constraint::Ratio(2,5)]).split(m[1]);

    let t = s.odi_t_up + s.odi_t_dn;
    let w = s.odi_w_up + s.odi_w_dn;
    let wr = if t > 0 { format!("{:.0}%", w as f64 / t as f64 * 100.0) } else { "—".into() };
    let pc = if s.odi_pnl > 0.001 { Color::Green } else if s.odi_pnl < -0.001 { Color::Red } else { Color::Gray };
    let bc = if s.live && s.odi_enabled { Color::Red } else { Color::Gray };

    let txt = format!(
        "Odiseo 85   Budget: ${:.0}   Balance: ${:.2}   Reinvest: {}\n\
         PnL: {:+.4}   Win Rate: {}\n\
         Trades: {}  (UP {}/{}  DN {}/{})   Sessions: {}   Órdenes: {}",
        s.odi_budget, s.odi_bal, if s.reinvest {"ON"}else{"OFF"},
        s.odi_pnl, wr,
        t, s.odi_w_up, s.odi_t_up, s.odi_w_dn, s.odi_t_dn, s.odi_sessions, s.orders,
    );
    f.render_widget(Paragraph::new(txt).style(Style::default().fg(pc).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL).title("⚡ Odiseo 85").border_style(Style::default().fg(bc))), odi[0]);

    let act = "[p] PANIC  [r] Reinvest  [l] LIVE/PAPER  [q] Salir";
    f.render_widget(Paragraph::new(act).style(Style::default().fg(Color::DarkGray)).block(Block::default().borders(Borders::ALL).title("Controles")), odi[1]);

    // Log
    let lines: Vec<Line> = s.log.iter().map(|e| Line::from(vec![
        Span::styled(format!("{} ", e.ts), Style::default().fg(Color::DarkGray)),
        Span::styled(&e.text, Style::default().fg(e.color)),
    ])).collect();
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("📋 Eventos")), m[2]);

    f.render_widget(Paragraph::new("[p] PANIC  [r] Reinvest  [l] LIVE  [q] Salir").style(Style::default().fg(Color::DarkGray)), m[3]);
}
