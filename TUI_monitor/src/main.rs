mod api;
mod commands;
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

// ─── Messages from spawned poller task → main loop ─────────────
enum PollUpdate {
    Btc(BtcInfo),
    Health(HealthInfo),
    Hft(HftState),
    Orders(Vec<OrderInfo>),
    Sessions(Vec<SessionInfo>),
}

struct State {
    connected: bool,
    tab: usize,
    btc: f64, btc_open: f64,
    bal: f64,

    sessions: Vec<SessionInfo>,

    hft: HftState,

    sl_pct: f64,
    sl_market: bool,

    input_mode: InputMode,
    input_buf: String,

    orders: i64,
    open_orders: Vec<api::OrderInfo>,
    log: VecDeque<api::LogEntry>,
    trades: VecDeque<api::TradeEntry>,
    warnings: VecDeque<String>,
    last_ws: Instant,
    last_api_ok: Instant,
    ws_pings: VecDeque<u64>,
    api_pings: VecDeque<u64>,
    book_up: api::BookDepth,
    book_dn: api::BookDepth,
    prev_secs_left: i32,
    session_open_up: f64,
    session_open_dn: f64,

    pub mt_outcome: String,
    pub mt_size: f64,
    pub mt_entry: f64,
    pub mt_budget: f64,
    pub mt_order_id: String,
    pub mt_exit_price: f64,
    pub mt_exit_order_id: String,
    pub mt_state: u8,
    pub mt_order_seen: bool,
    pub mt_exit_order_seen: bool,
    pub mt_order_placed_at: Instant,
    pub mt_exit_placed_at: Instant,
    pub mt_pnl_cum: f64,
    pub mt_trades: i64,
    pub mt_wins: i64,
    pub mt_last_fill_pct: f64,
    pub mt_tsl_pct: f64,
    pub mt_tsl_high: f64,
    pub mt_tsl_low: f64,
    pub mt_fill_avg: f64,
    pub mt_fill_count: i64,

    pub last_order_id: String,
    pub last_order_type: String,
    pub mt_sl_order_id: String,
    pub mt_sl_price: f64,

    pub trade_log: VecDeque<TradeLogEntry>,
    pub command_history: VecDeque<String>,
    pub history_cursor: Option<usize>,
    pub alerts: Vec<commands::Alert>,

    pub show_man: bool,
    pub pulse_tick: u64,
    pub btc_vol_1m: f64,
    pub btc_history: VecDeque<(f64, std::time::Instant)>,
    pub btc_velocity: f64,
    pub btc_acceleration: f64,
    pub session_vol_cum: f64,
    pub clob_up_history: VecDeque<(std::time::Instant, f64)>,
    pub clob_dn_history: VecDeque<(std::time::Instant, f64)>,
    pub clob_up_30s: f64,
    pub clob_dn_30s: f64,
    pub btc_price_30s: f64,
    pub prev_bid_up: f64, pub prev_ask_up: f64,
    pub prev_bid_dn: f64, pub prev_ask_dn: f64,
    pub abs_up: f64, pub abs_dn: f64,
    pub last_btc_vel: f64,
    pub imb_history: VecDeque<f64>,
    pub up_imb_history: VecDeque<f64>,
    pub dn_imb_history: VecDeque<f64>,
    pub prev_comb_imb: f64,

    pub sess_up_prices: VecDeque<f64>,
    pub sess_dn_prices: VecDeque<f64>,
    pub sess_mids: VecDeque<f64>,
    pub sess_spreads: VecDeque<f64>,
    pub sess_trades_up: u64,
    pub sess_trades_dn: u64,
    pub sess_vol_up: f64,
    pub sess_vol_dn: f64,
    pub sess_max_spread: f64,
    pub sess_min_spread: f64,
    pub ofi_up_history: VecDeque<f64>,
    pub ofi_dn_history: VecDeque<f64>,
    pub micro_up_history: VecDeque<f64>,
    pub micro_dn_history: VecDeque<f64>,

}

#[derive(Clone)]
pub struct TradeLogEntry {
    pub ts: String,
    pub text: String,
    pub color: Color,
}

impl TradeLogEntry {
    fn new(text: String, color: Color) -> Self {
        Self { ts: chrono::Local::now().format("%H:%M:%S").to_string(), text, color }
    }
}

impl State {
    fn new() -> Self {
        Self {
            connected: false, tab: 0,
            btc: 0.0, btc_open: 0.0,             bal: 0.0,
            sessions: Vec::new(),
            hft: HftState::default(),
            sl_pct: 0.0, sl_market: true,
            input_mode: InputMode::Normal,
            input_buf: String::new(),
            orders: 0,
            open_orders: Vec::new(),
            log: VecDeque::with_capacity(100),
            trades: VecDeque::with_capacity(100),
            warnings: VecDeque::with_capacity(20),
            last_ws: Instant::now(),
            last_api_ok: Instant::now(),
            ws_pings: VecDeque::with_capacity(20),
            api_pings: VecDeque::with_capacity(20),
            book_up: api::BookDepth::default(),
            book_dn: api::BookDepth::default(),
            prev_secs_left: -1,
            session_open_up: 0.0,
            session_open_dn: 0.0,
            mt_outcome: String::new(),
            mt_size: 0.0, mt_entry: 0.0, mt_budget: 0.0,
            mt_order_id: String::new(), mt_exit_price: 0.0,
            mt_exit_order_id: String::new(),
            mt_state: 0, mt_order_seen: false, mt_exit_order_seen: false,
            mt_order_placed_at: Instant::now(),
            mt_exit_placed_at: Instant::now(),
            mt_pnl_cum: 0.0, mt_trades: 0, mt_wins: 0,
            mt_last_fill_pct: 0.0,
            mt_sl_order_id: String::new(),
            mt_sl_price: 0.0,
            mt_tsl_pct: 0.0, mt_tsl_high: 0.0, mt_tsl_low: 1.0,
            mt_fill_avg: 0.0, mt_fill_count: 0,
            last_order_id: String::new(), last_order_type: String::new(),
            trade_log: VecDeque::with_capacity(60),
            command_history: VecDeque::with_capacity(50),
            history_cursor: None,
            alerts: Vec::new(),
            show_man: false,
            pulse_tick: 0,
            btc_vol_1m: 0.0,
            btc_history: VecDeque::with_capacity(10),
            btc_velocity: 0.0,
            btc_acceleration: 0.0,
            session_vol_cum: 0.0,
            clob_up_history: VecDeque::with_capacity(100),
            clob_dn_history: VecDeque::with_capacity(100),
            clob_up_30s: 0.0,
            clob_dn_30s: 0.0,
            btc_price_30s: 0.0,
            prev_bid_up: 0.0, prev_ask_up: 0.0,
            prev_bid_dn: 0.0, prev_ask_dn: 0.0,
            abs_up: 0.0, abs_dn: 0.0,
            last_btc_vel: 0.0,
            imb_history: VecDeque::with_capacity(200),
            up_imb_history: VecDeque::with_capacity(200),
            dn_imb_history: VecDeque::with_capacity(200),
            prev_comb_imb: 1.0,
            sess_up_prices: VecDeque::with_capacity(200),
            sess_dn_prices: VecDeque::with_capacity(200),
            sess_mids: VecDeque::with_capacity(200),
            sess_spreads: VecDeque::with_capacity(200),
            sess_trades_up: 0, sess_trades_dn: 0,
            sess_vol_up: 0.0, sess_vol_dn: 0.0,
            sess_max_spread: 0.0, sess_min_spread: 1.0,
            ofi_up_history: VecDeque::with_capacity(200),
            ofi_dn_history: VecDeque::with_capacity(200),
            micro_up_history: VecDeque::with_capacity(200),
            micro_dn_history: VecDeque::with_capacity(200),
        }
    }

    pub fn add_log(&mut self, text: impl Into<String>, color: Color) {
        self.log.push_front(api::LogEntry::new(text.into(), color));
        if self.log.len() > 100 { self.log.pop_back(); }
    }

    pub fn add_trade_log(&mut self, text: impl Into<String>, color: Color) {
        self.trade_log.push_front(TradeLogEntry::new(text.into(), color));
        if self.trade_log.len() > 60 { self.trade_log.pop_back(); }
    }

    fn add_warning(&mut self, text: impl Into<String>) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        self.warnings.push_front(format!("{} {}", ts, text.into()));
        if self.warnings.len() > 20 { self.warnings.pop_back(); }
    }

    pub fn reset_manual(&mut self) {
        self.mt_state = 0;
        self.mt_order_id.clear();
        self.mt_exit_order_id.clear();
        self.mt_sl_order_id.clear();
        self.mt_sl_price = 0.0;
        self.mt_order_seen = false;
        self.mt_exit_order_seen = false;
        self.mt_last_fill_pct = 0.0;
        self.mt_exit_price = 0.0;
        self.mt_exit_placed_at = Instant::now();
        self.mt_tsl_pct = 0.0;
        self.mt_tsl_high = 0.0;
        self.mt_tsl_low = 1.0;
        self.mt_fill_avg = 0.0;
        self.mt_fill_count = 0;
    }
}

// ═══════════════════════════════════════════════════════════════════
// HFT STATE APPLICATION
// ═══════════════════════════════════════════════════════════════════

fn apply_hft_state(new_hft: &HftState, s: &mut State) {
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

    s.session_vol_cum += new_hft.clob_trade_up_vol + new_hft.clob_trade_dn_vol;

    {
        let up = new_hft.clob_trade_up;
        let dn = new_hft.clob_trade_dn;
        if up > 0.0 { s.sess_up_prices.push_back(up); }
        if dn > 0.0 { s.sess_dn_prices.push_back(dn); }
        s.sess_mids.push_back(new_hft.mid);
        s.sess_spreads.push_back(new_hft.spread);
        if up > 0.0 || dn > 0.0 {
            if new_hft.clob_trade_up_vol > 0.0 { s.sess_trades_up += 1; s.sess_vol_up += new_hft.clob_trade_up_vol; }
            if new_hft.clob_trade_dn_vol > 0.0 { s.sess_trades_dn += 1; s.sess_vol_dn += new_hft.clob_trade_dn_vol; }
        }
        if new_hft.spread > s.sess_max_spread { s.sess_max_spread = new_hft.spread; }
        if new_hft.spread > 0.0 && new_hft.spread < s.sess_min_spread { s.sess_min_spread = new_hft.spread; }
        while s.sess_up_prices.len() > 200 { s.sess_up_prices.pop_front(); }
        while s.sess_dn_prices.len() > 200 { s.sess_dn_prices.pop_front(); }
        while s.sess_mids.len() > 200 { s.sess_mids.pop_front(); }
        while s.sess_spreads.len() > 200 { s.sess_spreads.pop_front(); }
    }

    {
        s.ofi_up_history.push_back(new_hft.ofi_up);
        s.ofi_dn_history.push_back(new_hft.ofi_dn);
        s.micro_up_history.push_back(new_hft.micro_price_up);
        s.micro_dn_history.push_back(new_hft.micro_price_dn);
        while s.ofi_up_history.len() > 200 { s.ofi_up_history.pop_front(); }
        while s.ofi_dn_history.len() > 200 { s.ofi_dn_history.pop_front(); }
        while s.micro_up_history.len() > 200 { s.micro_up_history.pop_front(); }
        while s.micro_dn_history.len() > 200 { s.micro_dn_history.pop_front(); }
    }

    {
        let up_bid_v: f64 = new_hft.depth_up_bids.iter().map(|(_, s)| s).sum();
        let up_ask_v: f64 = new_hft.depth_up_asks.iter().map(|(_, s)| s).sum();
        let dn_bid_v: f64 = new_hft.depth_dn_bids.iter().map(|(_, s)| s).sum();
        let dn_ask_v: f64 = new_hft.depth_dn_asks.iter().map(|(_, s)| s).sum();
        let up_imb_val = if up_ask_v > 0.0 { up_bid_v / up_ask_v } else { 1.0 };
        let dn_imb_val = if dn_ask_v > 0.0 { dn_bid_v / dn_ask_v } else { 1.0 };
        let total_bull = up_bid_v + dn_ask_v;
        let total_bear = up_ask_v + dn_bid_v;
        let comb_imb = if total_bear > 0.0 { total_bull / total_bear } else { 1.0 };
        s.prev_comb_imb = if s.imb_history.is_empty() { comb_imb } else { *s.imb_history.back().unwrap_or(&1.0) };
        s.imb_history.push_back(comb_imb);
        s.up_imb_history.push_back(up_imb_val);
        s.dn_imb_history.push_back(dn_imb_val);
        if s.imb_history.len() > 200 { s.imb_history.pop_front(); }
        if s.up_imb_history.len() > 200 { s.up_imb_history.pop_front(); }
        if s.dn_imb_history.len() > 200 { s.dn_imb_history.pop_front(); }
    }

    let secs = new_hft.secs_left;
    if s.prev_secs_left >= 0 && secs > s.prev_secs_left + 60 {
        s.session_open_up = new_hft.clob_trade_up;
        s.session_open_dn = new_hft.clob_trade_dn;
        if s.mt_state > 0 {
            s.add_trade_log("SESSION RESET — posicion manual cerrada".to_string(), Color::Yellow);
            s.reset_manual();
        }
        s.sess_up_prices.clear(); s.sess_dn_prices.clear();
        s.sess_mids.clear(); s.sess_spreads.clear();
        s.sess_trades_up = 0; s.sess_trades_dn = 0;
        s.sess_vol_up = 0.0; s.sess_vol_dn = 0.0;
        s.sess_max_spread = 0.0; s.sess_min_spread = 1.0;
        s.ofi_up_history.clear(); s.ofi_dn_history.clear();
        s.micro_up_history.clear(); s.micro_dn_history.clear();
        s.add_log("SESSION RESET — new orderbook".to_string(), Color::Yellow);
    }
    if s.session_open_up == 0.0 && new_hft.clob_trade_up > 0.0 {
        s.session_open_up = new_hft.clob_trade_up;
        s.session_open_dn = new_hft.clob_trade_dn;
    }
    s.prev_secs_left = secs;
}

// ═══════════════════════════════════════════════════════════════════
// POLLER TASK — runs independently, never blocks main loop
// ═══════════════════════════════════════════════════════════════════

async fn run_poller(tx: mpsc::UnboundedSender<PollUpdate>) {
    loop {
        let start = Instant::now();

        // Collect all poll results concurrently (each bounded to 2s by api.rs)
        let (btc, health, hft, orders, sessions) = tokio::join!(
            http_get::<BtcInfo>("/api/btc"),
            http_get::<HealthInfo>("/api/health"),
            http_get::<HftState>("/api/hft/latest"),
            http_get::<Vec<OrderInfo>>("/api/orders"),
            http_get::<Vec<SessionInfo>>("/api/sessions"),
        );

        if let Some(data) = btc { let _ = tx.send(PollUpdate::Btc(data)); }
        if let Some(data) = health { let _ = tx.send(PollUpdate::Health(data)); }
        if let Some(data) = hft { let _ = tx.send(PollUpdate::Hft(data)); }
        if let Some(data) = orders { let _ = tx.send(PollUpdate::Orders(data)); }
        if let Some(data) = sessions { let _ = tx.send(PollUpdate::Sessions(data)); }

        // Tick every 500ms — fast enough for HFT, doesn't overload backend
        let elapsed = start.elapsed();
        if elapsed < Duration::from_millis(500) {
            tokio::time::sleep(Duration::from_millis(500) - elapsed).await;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// MAIN — pure render loop, zero HTTP I/O
// ═══════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> io::Result<()> {

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let (ws_tx, mut ws_rx) = mpsc::channel::<WsMsg>(256);
    let (poll_tx, mut poll_rx) = mpsc::unbounded_channel::<PollUpdate>();

    // WebSocket task
    tokio::spawn(async move {
        loop {
            if let Ok((ws, _)) = connect_async(WS_URL).await {
                let (_, mut read) = ws.split();
                let _ = ws_tx.send(WsMsg { msg_type: Some("connected".into()), ..Default::default() }).await;
                while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = read.next().await {
                    if let Ok(msg) = serde_json::from_str::<WsMsg>(&text) {
                        let _ = ws_tx.send(msg).await;
                    } else if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text) {
                        if raw.get("type").and_then(|v| v.as_str()) == Some("order_result") {
                            let _ = ws_tx.send(WsMsg {
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

    // HTTP poller task — runs independently, never blocks the main loop
    tokio::spawn(run_poller(poll_tx));

    let (kb_tx, mut kb_rx) = mpsc::channel::<KeyCode>(16);
    tokio::spawn(async move { loop { if let Ok(Event::Key(k)) = event::read() { let _ = kb_tx.send(k.code).await; } } });

    let mut s = State::new();
    s.add_log("ZZIGNAL MONITOR".to_string(), Color::Magenta);

    // ═══════════════════════ MAIN LOOP ═══════════════════════
    // ZERO HTTP I/O — only channel drains, keyboard, and render
    loop {
        // ── Drain poll updates (capped) ──
        let mut poll_count = 0u32;
        let poll_cap = 10;
        while let Ok(update) = poll_rx.try_recv() {
            poll_count += 1;
            if poll_count > poll_cap { break; }
            let api_lat = s.last_api_ok.elapsed().as_millis() as u64;
            s.api_pings.push_front(api_lat);
            if s.api_pings.len() > 20 { s.api_pings.pop_back(); }
            s.last_api_ok = Instant::now();

            match update {
                PollUpdate::Btc(data) => {
                    s.btc = data.price;
                    if data.open > 0.0 { s.btc_open = data.open; }
                    s.btc_history.push_back((data.price, Instant::now()));
                    if s.btc_history.len() > 5 { s.btc_history.pop_front(); }
                    if s.btc_history.len() >= 2 {
                        let (p0, t0) = s.btc_history.front().unwrap();
                        let (p1, t1) = s.btc_history.back().unwrap();
                        let dt = t1.duration_since(*t0).as_secs_f64().max(0.1);
                        let new_vel = (p1 - p0) / dt;
                        s.btc_acceleration = (new_vel - s.last_btc_vel) / dt;
                        s.last_btc_vel = new_vel;
                        s.btc_velocity = new_vel;
                    }
                }
                PollUpdate::Health(data) => { s.bal = data.balance; }
                PollUpdate::Hft(data) => {
                    apply_hft_state(&data, &mut s);
                    commands::check_alerts(&mut s);
                }
                PollUpdate::Orders(orders) => {
                    let nc = orders.len() as i64;
                    if nc != s.orders { s.add_log(format!("Orders: {} -> {}", s.orders, nc), Color::Cyan); }
                    s.orders = nc;
                    s.open_orders = orders;
                    // Fill tracking: only when actively trading (pending/exit), 1s timeout
                    if s.mt_state == 1 || s.mt_state == 3 {
                        let _ = tokio::time::timeout(Duration::from_secs(1), commands::track_manual_fills(&mut s)).await;
                    }
                    // SL trigger: only when position active and SL set, 1s timeout
                    if s.mt_state == 2 && s.mt_sl_price > 0.0 {
                        let _ = tokio::time::timeout(Duration::from_secs(1), commands::check_sl_trigger(&mut s)).await;
                    }
                }
                PollUpdate::Sessions(data) => { s.sessions = data; }
            }
        }

        // ── Drain WS messages (capped, hft_state deferred to last) ──
        let mut ws_count = 0u32;
        let ws_cap = 15;
        while let Ok(msg) = ws_rx.try_recv() {
            ws_count += 1;
            if ws_count > ws_cap { break; }
            let ws_lat = s.last_ws.elapsed().as_millis() as u64;
            s.ws_pings.push_front(ws_lat);
            if s.ws_pings.len() > 20 { s.ws_pings.pop_back(); }
            s.last_ws = Instant::now();
            match msg.msg_type.as_deref() {
                Some("connected") => { s.connected = true; s.add_log("WS OK".to_string(), Color::Green); }
                Some("snapshot") => { s.bal = msg.balance.unwrap_or(s.bal); s.btc = msg.btc.unwrap_or(s.btc); }
                Some("btc_price") => {
                    s.btc = msg.price.unwrap_or(s.btc);
                    if let Some(open) = msg.open {
                        if open > 0.0 { s.btc_open = open; }
                    }
                }
                Some("balance") => {
                    let old = s.bal; s.bal = msg.balance.unwrap_or(s.bal);
                    if (s.bal - old).abs() > 0.01 { s.add_log(format!("USD ${:.2} ({:+.2})", s.bal, s.bal-old), if s.bal>old{Color::Green}else{Color::Red}); }
                }
                Some("order_result") => {
                    let ok = msg.success.unwrap_or(false);
                    let txt = msg.message.unwrap_or_default();
                    s.add_log(format!("{} {}", if ok {"OK"}else{"FAIL"}, txt), if ok{Color::Green}else{Color::Red});
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

        // ── Keyboard input ──
        while let Ok(k) = kb_rx.try_recv() {
            if s.input_mode == InputMode::Command {
                match k {
                    KeyCode::Esc => { s.input_mode = InputMode::Normal; s.input_buf.clear(); }
                    KeyCode::Enter => {
                        let c = s.input_buf.trim().to_string();
                        if !c.is_empty() {
                            s.command_history.push_front(c.clone());
                            if s.command_history.len() > 50 { s.command_history.pop_back(); }
                            s.history_cursor = None;
                        }
                        let _ = tokio::time::timeout(Duration::from_secs(2), commands::dispatch(&c, &mut s)).await;
                        s.input_mode = InputMode::Normal; s.input_buf.clear();
                    }
                    KeyCode::Up => {
                        let hist_len = s.command_history.len();
                        if hist_len == 0 { continue; }
                        let idx = match s.history_cursor {
                            None => 0,
                            Some(i) => (i + 1).min(hist_len - 1),
                        };
                        s.history_cursor = Some(idx);
                        s.input_buf = s.command_history.get(idx).cloned().unwrap_or_default();
                    }
                    KeyCode::Down => {
                        match s.history_cursor {
                            None | Some(0) => { s.history_cursor = None; s.input_buf.clear(); }
                            Some(i) => {
                                s.history_cursor = Some(i - 1);
                                s.input_buf = s.command_history.get(i - 1).cloned().unwrap_or_default();
                            }
                        }
                    }
                    KeyCode::Backspace => { s.input_buf.pop(); }
                    KeyCode::Char(c) => { if s.input_buf.len() < 25 { s.input_buf.push(c); } }
                    _ => {}
                }
                continue;
            }

            match k {
                KeyCode::Esc | KeyCode::Char('q') => {
                    if s.show_man { s.show_man = false; continue; }
                    disable_raw_mode()?;
                    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
                    terminal.show_cursor()?;
                    return Ok(());
                }
                KeyCode::Tab => { s.tab = (s.tab + 1) % 2; }
                KeyCode::Char('/') => { s.input_mode = InputMode::Command; s.input_buf.clear(); }
                KeyCode::Char('s') => {
                    let has_active = s.sessions.iter().any(|sess| sess.status == "recording");
                    if has_active {
                        s.add_log("Session 15-min YA activa — conectado a sesion existente".to_string(), Color::Cyan);
                    } else {
                        s.add_log("Starting 15-min session...".to_string(), Color::Green);
                        let now = chrono::Utc::now();
                        let name = now.format("BTC15-Manual-%Y%m%dT%H%M").to_string();
                        let body = format!(r#"{{"name":"{}","duration_min":15,"depth_levels":50,"indefinite":true}}"#, name);
                        let result = tokio::time::timeout(Duration::from_secs(2), http_post("/api/sessions/start", &body)).await;
                        match result {
                            Ok(Err(e)) => s.add_log(format!("Session start FAIL: {}", e), Color::Red),
                            Err(_) => s.add_log("Session start TIMEOUT".to_string(), Color::Red),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        // ── Render ──
        s.pulse_tick = s.pulse_tick.wrapping_add(1);
        terminal.draw(|f| ui::draw(f, &s))?;
        tokio::time::sleep(Duration::from_millis(33)).await;
    }
}
