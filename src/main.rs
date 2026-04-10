/// Polymarket BTC 15-min — ThinkorSwim layout
/// Cada panel (Grafico, Ordenes, Trading) es una ventana OS independiente
/// via egui show_viewport_immediate + wgpu backend (Metal en macOS).
mod credentials;
mod worker;

use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use eframe::egui::{self, Color32, FontId, RichText, Vec2, ViewportBuilder, ViewportId};
use egui_plot::{Bar, BarChart, Line, Plot, PlotBounds, PlotPoints, Polygon};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::warn;
use worker::{
    AppMsg, BookSnapshot, Candle, CandleInterval, CmdMsg, ConnStatus, MarketInfo, OpenOrder,
    OrderSide, Outcome, PriceLevel, RecentFill,
};
use crate::credentials::ClobCredentials;

const APP_TITLE: &str = "Polymarket BTC 15-min";
const MAX_LEVELS: usize = 12;

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] .env no cargado: {e}");
    }
    tracing_subscriber::fmt()
        .with_env_filter("polymarket_dashboard=info,warn")
        .init();

    let creds = match ClobCredentials::from_env() {
        Ok(c) => { tracing::info!("Wallet: {}", c.display_address()); Some(Arc::new(c)) }
        Err(e) => { warn!("Credenciales no disponibles: {:#}", e); None }
    };

    let (tx, rx) = mpsc::channel::<AppMsg>();
    let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel::<CmdMsg>();
    let interval_arc = Arc::new(Mutex::new(CandleInterval::OneSecond));

    if let Some(creds_arc) = creds.clone() {
        let iv = Arc::clone(&interval_arc);
        std::thread::Builder::new()
            .name("tokio-worker".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime")
                    .block_on(worker::run(tx, creds_arc, cmd_rx, iv));
            })
            .expect("spawn worker");
    } else {
        drop(cmd_rx);
    }

    let native_opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([520.0, 210.0])
            .with_min_inner_size([400.0, 160.0]),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_opts,
        Box::new(move |cc| {
            setup_style(&cc.egui_ctx);
            Ok(Box::new(TradingApp::new(rx, creds, cmd_tx, interval_arc)))
        }),
    )
}

// ─── Estilo ───────────────────────────────────────────────────────────────────

fn setup_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Heading,   FontId::proportional(18.0)),
        (egui::TextStyle::Body,      FontId::monospace(13.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
        (egui::TextStyle::Button,    FontId::proportional(13.0)),
        (egui::TextStyle::Small,     FontId::proportional(11.0)),
    ].into();
    ctx.set_style(style);
}

// ─── Configuracion de indicadores ─────────────────────────────────────────────

#[derive(Clone)]
struct IndicatorSettings {
    bb_period: usize, bb_std: f64,
    macd_fast: usize, macd_slow: usize, macd_signal: usize,
    rsi_period: usize, rsi_ob: f64, rsi_os: f64,
    vfi_period: usize, vfi_coeff: f64, vfi_vcoeff: f64, vfi_smooth: usize,
}
impl Default for IndicatorSettings {
    fn default() -> Self {
        Self {
            bb_period: 20, bb_std: 2.0,
            macd_fast: 12, macd_slow: 26, macd_signal: 9,
            rsi_period: 14, rsi_ob: 70.0, rsi_os: 30.0,
            vfi_period: 130, vfi_coeff: 0.2, vfi_vcoeff: 2.5, vfi_smooth: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ChartTab { Macd, Rsi, Vfi, Depth }

// ─── Estado compartido con cada viewport ─────────────────────────────────────

#[derive(Clone)]
struct ChartState {
    tab:           ChartTab,
    settings:      IndicatorSettings,
    settings_open: bool,
    interval:      CandleInterval,
}

#[derive(Clone)]
struct TradingState {
    outcome:          Outcome,
    amount:           String,
    price:            String,
    feedback:         String,
    fills_show_all:   bool,
    // Scalp mode
    scalp_profit_pct: f64,   // % de ganancia objetivo (modo pct)
    scalp_profit_pts: f64,   // puntos absolutos objetivo (modo pts)
    scalp_use_pct:    bool,  // true = % mode, false = points mode
}

// ─── Estado principal ─────────────────────────────────────────────────────────

struct TradingApp {
    rx:              mpsc::Receiver<AppMsg>,
    cmd_tx:          tokio_mpsc::UnboundedSender<CmdMsg>,
    creds:           Option<Arc<ClobCredentials>>,
    conn_status:     ConnStatus,
    market:          Option<MarketInfo>,
    book_up:         Option<BookSnapshot>,
    book_down:       Option<BookSnapshot>,
    last_update:     Option<std::time::Instant>,
    balance:         Option<f64>,
    last_trade_up:   Option<f64>,
    last_trade_down: Option<f64>,
    btc_price:       Option<f64>,
    btc_open:        Option<f64>,
    open_orders:     Vec<OpenOrder>,
    recent_fills:    Vec<RecentFill>,
    candles:         Vec<Candle>,
    current_interval: CandleInterval,
    interval_arc:    Arc<Mutex<CandleInterval>>,

    // Mutable state shared with viewports via Arc<Mutex<>>
    chart_st:   Arc<Mutex<ChartState>>,
    trading_st: Arc<Mutex<TradingState>>,

    // Ventanas abiertas (flags para el status bar)
    chart_open:   bool,
    orders_open:  bool,
    trading_open: bool,

    // Contador de frames: los primeros frames se omiten los viewports para
    // que el atlas pre-calentado se commitee al GPU antes del primer render.
    frame_count: u32,
}

impl TradingApp {
    fn new(
        rx: mpsc::Receiver<AppMsg>,
        creds: Option<Arc<ClobCredentials>>,
        cmd_tx: tokio_mpsc::UnboundedSender<CmdMsg>,
        interval_arc: Arc<Mutex<CandleInterval>>,
    ) -> Self {
        Self {
            rx, cmd_tx, creds,
            conn_status: ConnStatus::Initializing,
            market: None, book_up: None, book_down: None,
            last_update: None, balance: None,
            last_trade_up: None, last_trade_down: None,
            btc_price: None, btc_open: None,
            open_orders: Vec::new(), recent_fills: Vec::new(),
            candles: Vec::new(),
            current_interval: CandleInterval::OneSecond,
            interval_arc,
            chart_st: Arc::new(Mutex::new(ChartState {
                tab: ChartTab::Macd,
                settings: IndicatorSettings::default(),
                settings_open: false,
                interval: CandleInterval::OneSecond,
            })),
            trading_st: Arc::new(Mutex::new(TradingState {
                outcome: Outcome::Up,
                amount:  "10.00".into(),
                price:   "0.50".into(),
                feedback: String::new(),
                fills_show_all:   false,
                scalp_profit_pct: 5.0,
                scalp_profit_pts: 0.05,
                scalp_use_pct:    true,
            })),
            chart_open: true, orders_open: true, trading_open: true,
            frame_count: 0,
        }
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                AppMsg::Status(s) => {
                    if matches!(s, ConnStatus::MarketFound(_)) {
                        self.btc_open = None; self.btc_price = None;
                        self.book_up = None; self.book_down = None;
                    }
                    if let ConnStatus::MarketFound(ref info) = s {
                        self.market = Some(info.clone());
                    }
                    self.conn_status = s;
                }
                AppMsg::BookUp(b)        => { self.book_up    = Some(b); self.last_update = Some(std::time::Instant::now()); }
                AppMsg::BookDown(b)      => { self.book_down  = Some(b); }
                AppMsg::LastTradeUp(p)   => { self.last_trade_up   = Some(p); }
                AppMsg::LastTradeDown(p) => { self.last_trade_down = Some(p); }
                AppMsg::Balance(b)       => { self.balance = Some(b); }
                AppMsg::BtcOpen(p)       => { self.btc_open  = Some(p); }
                AppMsg::BtcPrice(p)      => { self.btc_price = Some(p); }
                AppMsg::OrderResult(m)   => {
                    if let Ok(mut ts) = self.trading_st.lock() { ts.feedback = m; }
                }
                AppMsg::OpenOrders(v)  => { self.open_orders  = v; }
                AppMsg::RecentFills(v) => { self.recent_fills = v; }
                AppMsg::Candles(v)     => { self.candles = v; }
                AppMsg::CandleUpdate(c) => {
                    const MAX: usize = 1000;
                    if let Some(last) = self.candles.last_mut() {
                        if last.open_time == c.open_time { *last = c; }
                        else {
                            self.candles.push(c);
                            if self.candles.len() > MAX { self.candles.remove(0); }
                        }
                    } else { self.candles.push(c); }
                }
            }
        }
    }

    fn send_cmd(&self, cmd: CmdMsg) { let _ = self.cmd_tx.send(cmd); }
}

// ─── UI loop ─────────────────────────────────────────────────────────────────

impl eframe::App for TradingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_messages();
        ctx.request_repaint_after(Duration::from_millis(16));
        self.frame_count += 1;

        // ── Pre-calentar el atlas de fuentes (frame 1 solamente) ────────────
        // ctx.fonts() sólo está disponible dentro del event loop (no en setup).
        // Forzamos la carga de todos los glifos en el frame 1 para que el atlas
        // crezca UNA sola vez (en el end_frame del frame 1) antes de que los
        // sub-viewports comiencen a renderizar — evita el panic de epaint en
        // macOS/wgpu: "Partial texture update outside bounds of Managed(0)".
        if self.frame_count == 1 {
            let warmup = concat!(
                " !\"#$%&'()*+,-./0123456789:;<=>?@",
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`",
                "abcdefghijklmnopqrstuvwxyz{|}~",
                "▲▼←→●◌",
                "UPDOWNBTCUSDTBUYSELLMKTLMTSCALPCancel",
            );
            ctx.fonts(|fonts| {
                for &size in &[9.0_f32, 9.5, 10.0, 10.5, 11.0, 11.5, 12.0,
                               12.5, 13.0, 13.5, 14.0, 15.0, 16.0, 17.0, 18.0] {
                    let _ = fonts.layout_no_wrap(
                        warmup.to_string(),
                        egui::FontId::proportional(size),
                        egui::Color32::WHITE,
                    );
                    let _ = fonts.layout_no_wrap(
                        warmup.to_string(),
                        egui::FontId::monospace(size),
                        egui::Color32::WHITE,
                    );
                }
            });
        }

        // Sync interval changes from chart viewport back to worker
        {
            let new_iv = self.chart_st.lock().map(|cs| cs.interval).unwrap_or(self.current_interval);
            if new_iv != self.current_interval {
                self.current_interval = new_iv;
                if let Ok(mut g) = self.interval_arc.lock() { *g = new_iv; }
                self.candles.clear();
            }
        }

        // ── Sub-viewports: esperar 2 frames para que el atlas pre-calentado
        // se commitee al GPU antes del primer render de cada viewport.
        if self.frame_count < 3 {
            egui::CentralPanel::default().show(ctx, |ui| { draw_control_bar(ui, self); });
            return;
        }

        // ── Viewport: Grafico ───────────────────────────────────────────────
        let chart_close = Arc::new(AtomicBool::new(false));
        if self.chart_open {
            let candles    = self.candles.clone();
            let book_up    = self.book_up.clone();
            let book_down  = self.book_down.clone();
            let chart_st   = Arc::clone(&self.chart_st);
            let close_flag = Arc::clone(&chart_close);

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("vp_chart"),
                ViewportBuilder::default()
                    .with_title("BTC/USDT — Grafico")
                    .with_inner_size([980.0, 620.0])
                    .with_min_inner_size([500.0, 300.0]),
                move |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        close_flag.store(true, Ordering::Relaxed);
                    }
                    vp_ctx.request_repaint_after(Duration::from_millis(16));
                    egui::CentralPanel::default().show(vp_ctx, |ui| {
                        if let Ok(mut cs) = chart_st.lock() {
                            draw_chart_viewport(ui, &candles, book_up.as_ref(), book_down.as_ref(), &mut cs);
                        }
                    });
                },
            );
        }
        if chart_close.load(Ordering::Relaxed) { self.chart_open = false; }

        // ── Viewport: Ordenes ───────────────────────────────────────────────
        let orders_close = Arc::new(AtomicBool::new(false));
        if self.orders_open {
            let book_up      = self.book_up.clone();
            let book_down    = self.book_down.clone();
            let market       = self.market.clone();
            let open_orders  = self.open_orders.clone();
            let recent_fills = self.recent_fills.clone();
            let ltu          = self.last_trade_up;
            let ltd          = self.last_trade_down;
            let cmd_tx       = self.cmd_tx.clone();
            let trading_st   = Arc::clone(&self.trading_st);
            let close_flag   = Arc::clone(&orders_close);
            // Position computed from fills
            let (pos_up, pos_dn) = position_from_fills(&self.recent_fills);
            let ltu2 = self.last_trade_up;
            let ltd2 = self.last_trade_down;

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("vp_orders"),
                ViewportBuilder::default()
                    .with_title("Ordenes — Book y Posicion")
                    .with_inner_size([480.0, 760.0])
                    .with_min_inner_size([320.0, 300.0]),
                move |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        close_flag.store(true, Ordering::Relaxed);
                    }
                    vp_ctx.request_repaint_after(Duration::from_millis(16));
                    egui::CentralPanel::default().show(vp_ctx, |ui| {
                        egui::ScrollArea::vertical().auto_shrink([false;2]).show(ui, |ui| {
                            draw_book_dual(ui, book_up.as_ref(), book_down.as_ref(),
                                          market.as_ref(), ltu, ltd);
                            ui.separator();
                            draw_position_panel(
                                ui, pos_up, pos_dn, ltu2, ltd2,
                                &open_orders, &recent_fills,
                                &trading_st,
                                &cmd_tx,
                            );
                        });
                    });
                },
            );
        }
        if orders_close.load(Ordering::Relaxed) { self.orders_open = false; }

        // ── Viewport: Trading ───────────────────────────────────────────────
        let trading_close = Arc::new(AtomicBool::new(false));
        if self.trading_open {
            let has_creds  = self.creds.is_some();
            let cmd_tx     = self.cmd_tx.clone();
            let trading_st = Arc::clone(&self.trading_st);
            let close_flag = Arc::clone(&trading_close);
            let market     = self.market.clone();

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("vp_trading"),
                ViewportBuilder::default()
                    .with_title("Trading — Ejecutar Ordenes")
                    .with_inner_size([700.0, 190.0])
                    .with_min_inner_size([500.0, 150.0]),
                move |vp_ctx, _class| {
                    if vp_ctx.input(|i| i.viewport().close_requested()) {
                        close_flag.store(true, Ordering::Relaxed);
                    }
                    vp_ctx.request_repaint_after(Duration::from_millis(16));
                    egui::CentralPanel::default().show(vp_ctx, |ui| {
                        draw_trading_viewport(ui, has_creds, &cmd_tx, &trading_st, market.as_ref());
                    });
                },
            );
        }
        if trading_close.load(Ordering::Relaxed) { self.trading_open = false; }

        // ── Panel central: barra de control ────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            draw_control_bar(ui, self);
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers de posicion
// ─────────────────────────────────────────────────────────────────────────────

fn position_from_fills(fills: &[RecentFill]) -> (f64, f64) {
    let mut pos_up = 0.0_f64;
    let mut pos_dn = 0.0_f64;
    for f in fills {
        let sign = if matches!(f.side, OrderSide::Buy) { 1.0 } else { -1.0 };
        if f.outcome.to_lowercase().contains("up") { pos_up += sign * f.size; }
        else { pos_dn += sign * f.size; }
    }
    (pos_up, pos_dn)
}

// ─────────────────────────────────────────────────────────────────────────────
// Barra de control (ventana principal pequeña)
// ─────────────────────────────────────────────────────────────────────────────

fn draw_control_bar(ui: &mut egui::Ui, app: &mut TradingApp) {
    ui.add_space(6.0);

    // Fila 1: titulo + conn badge
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Polymarket BTC 15-min").size(17.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            draw_conn_badge(ui, &app.conn_status);
        });
    });
    ui.separator();

    // Fila 2: wallet + balance
    ui.horizontal(|ui| {
        ui.label(RichText::new("Wallet:").color(Color32::GRAY).size(11.0));
        match &app.creds {
            Some(c) => {
                ui.label(RichText::new(c.display_address()).monospace()
                    .color(Color32::from_rgb(100, 185, 255)).size(12.0));
                ui.label(RichText::new("●").color(Color32::from_rgb(0, 210, 100)).size(11.0));
            }
            None => { ui.label(RichText::new("Sin credenciales").color(Color32::YELLOW).size(11.0)); }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match app.balance {
                Some(b) => {
                    ui.label(RichText::new(format!("${:.2}", b)).monospace()
                        .color(Color32::from_rgb(255, 215, 0)).size(15.0).strong());
                    ui.label(RichText::new("USDC:").color(Color32::GRAY).size(11.0));
                }
                None => { ui.label(RichText::new("—").color(Color32::GRAY).size(11.0)); }
            }
        });
    });

    // Fila 3: BTC precio + mercado countdown
    ui.horizontal(|ui| {
        match (app.btc_open, app.btc_price) {
            (Some(open), Some(cur)) => {
                let d = cur - open;
                let pct = d / open * 100.0;
                let (arrow, col) = if d >= 0.0 { ("▲", Color32::from_rgb(0, 220, 100)) }
                                   else { ("▼", Color32::from_rgb(240, 70, 70)) };
                ui.label(RichText::new(format!("BTC ${:.2} {}", cur, arrow)).size(15.0).strong().color(col));
                let sign = if d >= 0.0 { "+" } else { "" };
                ui.label(RichText::new(format!("({sign}{:.2}%)", pct)).size(11.0).color(col));
            }
            (_, Some(cur)) => {
                ui.label(RichText::new(format!("BTC ${:.2}", cur)).size(15.0).strong().color(Color32::WHITE));
            }
            _ => { ui.label(RichText::new("BTC: ...").color(Color32::GRAY).size(12.0)); }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(info) = &app.market {
                let rem = info.end_date - Utc::now();
                let m = rem.num_minutes(); let s = rem.num_seconds() % 60;
                let col = match m { 0..=1 => Color32::RED, 2..=4 => Color32::YELLOW, _ => Color32::WHITE };
                ui.label(RichText::new(format!("{:02}:{:02}", m, s)).color(col).size(14.0).strong().monospace());
                ui.label(RichText::new("Cierra:").color(Color32::GRAY).size(10.0));
            }
        });
    });

    ui.separator();

    // Fila 4: botones de ventanas
    ui.horizontal(|ui| {
        ui.label(RichText::new("Ventanas:").color(Color32::GRAY).size(11.0));
        ui.add_space(4.0);
        for (label, open) in [
            ("[G] Grafico",  &mut app.chart_open),
            ("[O] Ordenes",  &mut app.orders_open),
            ("[T] Trading",  &mut app.trading_open),
        ] {
            let fill = if *open { Color32::from_rgb(30, 80, 140) } else { Color32::from_rgb(40, 40, 40) };
            if ui.add(egui::Button::new(RichText::new(label).size(12.0)).fill(fill)).clicked() {
                *open = !*open;
            }
        }
    });
}

fn draw_conn_badge(ui: &mut egui::Ui, status: &ConnStatus) {
    let (color, label) = match status {
        ConnStatus::Live            => (Color32::from_rgb(0, 210, 100), "● LIVE"),
        ConnStatus::Reconnecting(_) => (Color32::YELLOW,                "◌ RECON"),
        ConnStatus::Error(_)        => (Color32::RED,                   "✕ ERROR"),
        _                          => (Color32::GRAY,                   "○ INIT"),
    };
    ui.label(RichText::new(label).color(color).size(12.0).strong());
}

// ─────────────────────────────────────────────────────────────────────────────
// Viewport: Grafico
// ─────────────────────────────────────────────────────────────────────────────

fn draw_chart_viewport(
    ui: &mut egui::Ui,
    candles: &[Candle],
    book_up: Option<&BookSnapshot>,
    book_down: Option<&BookSnapshot>,
    cs: &mut ChartState,
) {
    let w = ui.available_width();

    // Barra de intervalos
    ui.horizontal(|ui| {
        ui.label(RichText::new("BTC/USDT").color(Color32::GRAY).size(11.0).strong());
        ui.add_space(6.0);
        for iv in [
            CandleInterval::OneSecond, CandleInterval::OneMinute,
            CandleInterval::FiveMinutes, CandleInterval::FifteenMinutes, CandleInterval::OneHour,
        ] {
            let sel = cs.interval == iv;
            let (tc, fill) = if sel { (Color32::BLACK, Color32::from_rgb(100, 200, 255)) }
                             else   { (Color32::GRAY,  Color32::TRANSPARENT) };
            if ui.add(egui::Button::new(RichText::new(iv.label()).color(tc).size(11.0)).fill(fill))
                .clicked() && !sel
            {
                cs.interval = iv;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{} velas · Binance", candles.len()))
                .color(Color32::GRAY).size(10.0));
        });
    });
    ui.separator();

    if candles.is_empty() {
        ui.add_space(40.0);
        ui.label(RichText::new("Cargando velas...").color(Color32::GRAY).size(12.0));
        return;
    }

    draw_candlestick_bb(ui, candles, &cs.settings, w, 280.0);
    draw_volume_bars(ui, candles, w, 65.0);
    draw_indicator_tabs(ui, &mut cs.tab, &mut cs.settings, &mut cs.settings_open);
    match cs.tab {
        ChartTab::Macd  => draw_macd(ui, candles, &cs.settings, 130.0),
        ChartTab::Rsi   => draw_rsi(ui, candles, &cs.settings, 130.0),
        ChartTab::Vfi   => draw_vfi(ui, candles, &cs.settings, 130.0),
        ChartTab::Depth => draw_depth(ui, book_up, book_down, 150.0),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Viewport: Ordenes (order books + posicion + fills)
// ─────────────────────────────────────────────────────────────────────────────

fn draw_book_dual(
    ui: &mut egui::Ui,
    book_up: Option<&BookSnapshot>,
    book_down: Option<&BookSnapshot>,
    market: Option<&MarketInfo>,
    last_trade_up: Option<f64>,
    last_trade_down: Option<f64>,
) {
    let total_w = ui.available_width();
    let half_w  = (total_w - 16.0) / 2.0;
    let out_up   = market.map(|m| m.outcome_up.as_str()).unwrap_or("UP");
    let out_down = market.map(|m| m.outcome_down.as_str()).unwrap_or("DOWN");

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half_w);
            draw_single_book(ui, book_up, half_w, out_up, Color32::from_rgb(0, 200, 80), last_trade_up);
        });
        ui.add_space(4.0); ui.separator(); ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.set_width(half_w);
            draw_single_book(ui, book_down, half_w, out_down, Color32::from_rgb(220, 100, 20), last_trade_down);
        });
    });
}

fn draw_single_book(
    ui: &mut egui::Ui,
    book: Option<&BookSnapshot>,
    width: f32,
    label: &str,
    label_color: Color32,
    last_trade: Option<f64>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(label_color).size(15.0).strong());
        if let Some(ltp) = last_trade {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("ult {:.4}", ltp)).color(label_color).size(11.0));
        }
        if let Some(b) = book {
            let best_bid = b.bids.iter().map(|l| l.price).fold(f64::NEG_INFINITY, f64::max);
            let best_ask = b.asks.iter().map(|l| l.price).fold(f64::INFINITY, f64::min);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if best_ask < f64::INFINITY {
                    ui.label(RichText::new(format!("Ask {:.4}", best_ask)).color(Color32::from_rgb(220,60,60)).size(11.0));
                }
                if best_bid > f64::NEG_INFINITY {
                    ui.label(RichText::new(format!("Bid {:.4}", best_bid)).color(Color32::from_rgb(0,200,80)).size(11.0));
                }
            });
        }
    });

    match book {
        Some(b) => {
            let mut bids = b.bids.clone();
            let mut asks = b.asks.clone();
            bids.sort_by(|a, x| x.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
            asks.sort_by(|a, x| a.price.partial_cmp(&x.price).unwrap_or(std::cmp::Ordering::Equal));
            bids.truncate(MAX_LEVELS); asks.truncate(MAX_LEVELS);
            let max_bid = bids.iter().map(|l| l.size).fold(0.0_f64, f64::max);
            let max_ask = asks.iter().map(|l| l.size).fold(0.0_f64, f64::max);
            let col_w   = (width - 8.0) / 2.0;
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    book_header(ui, "BIDS", Color32::from_rgb(0, 210, 100));
                    ui.separator();
                    for lvl in &bids { book_row(ui, lvl, true, max_bid, col_w); }
                });
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    book_header(ui, "ASKS", Color32::from_rgb(220, 60, 60));
                    ui.separator();
                    for lvl in &asks { book_row(ui, lvl, false, max_ask, col_w); }
                });
            });
        }
        None => {
            ui.add_space(12.0);
            ui.label(RichText::new("Esperando...").color(Color32::GRAY).size(11.0));
        }
    }
}

fn book_header(ui: &mut egui::Ui, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(color).strong().size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("SIZE").color(Color32::GRAY).size(10.0));
            ui.add_space(36.0);
            ui.label(RichText::new("PRECIO").color(Color32::GRAY).size(10.0));
        });
    });
}

fn book_row(ui: &mut egui::Ui, lvl: &PriceLevel, is_bid: bool, max_size: f64, col_w: f32) {
    let row_h = 18.0_f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(col_w, row_h), egui::Sense::hover());
    if !ui.is_rect_visible(rect) { return; }
    if max_size > 0.0 {
        let frac = (lvl.size / max_size).min(1.0) as f32;
        let bar_color = if is_bid { Color32::from_rgba_unmultiplied(0, 180, 80, 40) }
                        else      { Color32::from_rgba_unmultiplied(220, 50, 50, 40) };
        ui.painter().rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(col_w * frac, row_h)), 0.0, bar_color);
    }
    let text_color = if is_bid { Color32::from_rgb(0, 220, 100) } else { Color32::from_rgb(240, 70, 70) };
    ui.painter().text(rect.left_center() + Vec2::new(4.0, 0.0), egui::Align2::LEFT_CENTER,
        format!("{:.4}", lvl.price), FontId::monospace(11.5), text_color);
    ui.painter().text(rect.right_center() - Vec2::new(4.0, 0.0), egui::Align2::RIGHT_CENTER,
        format!("{:.1}", lvl.size), FontId::monospace(11.5), Color32::from_rgb(190, 190, 190));
}

// ── Posicion + Ordenes + Fills (dentro del viewport ordenes) ──────────────────

#[allow(clippy::too_many_arguments)]
fn draw_position_panel(
    ui: &mut egui::Ui,
    pos_up: f64,
    pos_dn: f64,
    last_trade_up: Option<f64>,
    last_trade_down: Option<f64>,
    open_orders: &[OpenOrder],
    recent_fills: &[RecentFill],
    trading_st: &Arc<Mutex<TradingState>>,
    cmd_tx: &tokio_mpsc::UnboundedSender<CmdMsg>,
) {
    // ── Posicion: tarjetas visuales ───────────────────────────────────────────
    ui.label(RichText::new("POSICIONES ABIERTAS").color(Color32::GRAY).size(10.5).strong());
    ui.add_space(3.0);

    let has_pos = pos_up.abs() > 0.001 || pos_dn.abs() > 0.001;
    if !has_pos {
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 28, 35))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .rounding(4.0)
            .show(ui, |ui: &mut egui::Ui| {
            ui.label(RichText::new("Sin posicion abierta").color(Color32::GRAY).size(11.0));
        });
    }

    if pos_up.abs() > 0.001 {
        let val = pos_up * last_trade_up.unwrap_or(0.5);
        let px_ref = last_trade_up.unwrap_or(0.5);
        egui::Frame::none()
            .fill(Color32::from_rgb(0, 45, 18))
            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
            .rounding(4.0)
            .show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("UP").color(Color32::from_rgb(80, 255, 130)).size(15.0).strong());
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{:.2} shares", pos_up))
                    .color(Color32::WHITE).size(14.0).monospace().strong());
                ui.add_space(6.0);
                ui.label(RichText::new(format!("~${:.2}", val))
                    .color(Color32::from_rgb(180, 240, 200)).size(13.0).monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(
                        RichText::new("MKT SELL").color(Color32::WHITE).size(11.0).strong())
                        .fill(Color32::from_rgb(160, 0, 0)).min_size(Vec2::new(72.0, 22.0))).clicked()
                    {
                        let _ = cmd_tx.send(CmdMsg::PlaceMarketOrder {
                            side: OrderSide::Sell, outcome: Outcome::Up, amount_usdc: pos_up,
                        });
                        if let Ok(mut ts) = trading_st.lock() {
                            ts.feedback = format!("SELL UP MKT {:.2} sh", pos_up);
                        }
                    }
                    ui.add_space(4.0);
                    let lmt_px = (px_ref * 0.99).max(0.01);
                    if ui.add(egui::Button::new(
                        RichText::new(format!("SELL LMT {:.4}", lmt_px)).color(Color32::BLACK).size(11.0))
                        .fill(Color32::from_rgb(200, 100, 20)).min_size(Vec2::new(90.0, 22.0))).clicked()
                    {
                        let _ = cmd_tx.send(CmdMsg::PlaceLimitOrder {
                            side: OrderSide::Sell, outcome: Outcome::Up, price: lmt_px, size: pos_up,
                        });
                        if let Ok(mut ts) = trading_st.lock() {
                            ts.feedback = format!("SELL UP LMT {:.2} sh @ {:.4}", pos_up, lmt_px);
                        }
                    }
                });
            });
        });
        ui.add_space(3.0);
    }

    if pos_dn.abs() > 0.001 {
        let val = pos_dn * last_trade_down.unwrap_or(0.5);
        let px_ref = last_trade_down.unwrap_or(0.5);
        egui::Frame::none()
            .fill(Color32::from_rgb(50, 20, 0))
            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
            .rounding(4.0)
            .show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("DOWN").color(Color32::from_rgb(255, 140, 50)).size(15.0).strong());
                ui.add_space(6.0);
                ui.label(RichText::new(format!("{:.2} shares", pos_dn))
                    .color(Color32::WHITE).size(14.0).monospace().strong());
                ui.add_space(6.0);
                ui.label(RichText::new(format!("~${:.2}", val))
                    .color(Color32::from_rgb(240, 200, 160)).size(13.0).monospace());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(
                        RichText::new("MKT SELL").color(Color32::WHITE).size(11.0).strong())
                        .fill(Color32::from_rgb(160, 0, 0)).min_size(Vec2::new(72.0, 22.0))).clicked()
                    {
                        let _ = cmd_tx.send(CmdMsg::PlaceMarketOrder {
                            side: OrderSide::Sell, outcome: Outcome::Down, amount_usdc: pos_dn,
                        });
                        if let Ok(mut ts) = trading_st.lock() {
                            ts.feedback = format!("SELL DOWN MKT {:.2} sh", pos_dn);
                        }
                    }
                    ui.add_space(4.0);
                    let lmt_px = (px_ref * 0.99).max(0.01);
                    if ui.add(egui::Button::new(
                        RichText::new(format!("SELL LMT {:.4}", lmt_px)).color(Color32::BLACK).size(11.0))
                        .fill(Color32::from_rgb(200, 100, 20)).min_size(Vec2::new(90.0, 22.0))).clicked()
                    {
                        let _ = cmd_tx.send(CmdMsg::PlaceLimitOrder {
                            side: OrderSide::Sell, outcome: Outcome::Down, price: lmt_px, size: pos_dn,
                        });
                        if let Ok(mut ts) = trading_st.lock() {
                            ts.feedback = format!("SELL DOWN LMT {:.2} sh @ {:.4}", pos_dn, lmt_px);
                        }
                    }
                });
            });
        });
        ui.add_space(3.0);
    }

    ui.separator();

    let total_w = ui.available_width();
    let col_w   = (total_w - 16.0) / 2.0;

    ui.horizontal(|ui| {
        // ── Ordenes abiertas ─────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(col_w);
            ui.label(RichText::new(format!("ORDENES ABIERTAS ({})", open_orders.len()))
                .color(Color32::from_rgb(255, 200, 50)).size(11.0).strong());
            ui.separator();
            if open_orders.is_empty() {
                ui.label(RichText::new("Sin ordenes abiertas").color(Color32::GRAY).size(11.0));
            } else {
                let mut to_cancel: Option<String> = None;
                for o in open_orders {
                    let is_buy = matches!(o.side, OrderSide::Buy);
                    let side_color = if is_buy { Color32::from_rgb(0, 200, 80) } else { Color32::from_rgb(220, 60, 60) };
                    let row_bg    = if is_buy { Color32::from_rgb(0, 35, 12) } else { Color32::from_rgb(45, 8, 8) };
                    let side_str  = if is_buy { "BUY" } else { "SEL" };
                    let pct = if o.size_orig > 0.0 { o.size_matched / o.size_orig * 100.0 } else { 0.0 };
                    let out_lbl   = &o.outcome[..o.outcome.len().min(4)];
                    let id_short  = &o.id[..o.id.len().min(8)];

                    egui::Frame::none()
                        .fill(row_bg)
                        .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                        .rounding(3.0)
                        .show(ui, |ui: &mut egui::Ui| {
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(RichText::new("X").color(Color32::WHITE).size(9.0))
                                .fill(Color32::from_rgb(130, 0, 0))
                                .min_size(Vec2::new(14.0, 14.0)))
                                .on_hover_text(format!("Cancelar {id_short}"))
                                .clicked()
                            {
                                to_cancel = Some(o.id.clone());
                            }
                            ui.label(RichText::new(format!("{:<4}", out_lbl)).color(Color32::WHITE).size(10.5).monospace().strong());
                            ui.label(RichText::new(format!("{:<3}", side_str)).color(side_color).size(10.5).monospace().strong());
                            ui.label(RichText::new(format!("{:.4}", o.price)).color(Color32::from_rgb(210,210,210)).size(10.0).monospace());
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("{:.1}/{:.1}", o.size_matched, o.size_orig)).color(Color32::GRAY).size(10.0).monospace());
                            ui.add_space(2.0);
                            let bar_color = if pct >= 100.0 { Color32::from_rgb(50, 200, 80) }
                                            else if pct > 0.0 { Color32::from_rgb(200, 170, 30) }
                                            else { Color32::from_rgb(80, 80, 80) };
                            ui.label(RichText::new(format!("{:.0}%", pct)).color(bar_color).size(9.5).monospace());
                        });
                    });
                    ui.add_space(1.0);
                }
                if let Some(id) = to_cancel {
                    let _ = cmd_tx.send(CmdMsg::CancelOrder { order_id: id });
                    if let Ok(mut ts) = trading_st.lock() { ts.feedback = "Cancelando orden...".into(); }
                }
            }
        });

        ui.separator();

        // ── Historial fills ──────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(col_w);

            let (show_all, fills_len) = trading_st.lock()
                .map(|ts| (ts.fills_show_all, recent_fills.len()))
                .unwrap_or((false, recent_fills.len()));

            let to_show = if show_all { recent_fills.len() } else { 10.min(recent_fills.len()) };

            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("HISTORIAL ({})", fills_len))
                    .color(Color32::from_rgb(255, 200, 50)).size(11.0).strong());
                if fills_len > 10 {
                    let label = if show_all { "▲ menos" } else { "▼ mas" };
                    if ui.small_button(label).clicked() {
                        if let Ok(mut ts) = trading_st.lock() { ts.fills_show_all = !ts.fills_show_all; }
                    }
                }
            });
            ui.separator();

            if recent_fills.is_empty() {
                ui.label(RichText::new("Sin trades").color(Color32::GRAY).size(11.0));
            } else {
                // Cabecera
                ui.label(RichText::new(format!("{:<4} {:<3} {:>5} {:>6} {:>5} {:>5}",
                    "OUT","SIDE","PX","SZ","HORA","SESION"))
                    .color(Color32::GRAY).size(9.5).monospace());
                ui.separator();

                let mut net = 0.0_f64;
                for f in recent_fills.iter().take(to_show) {
                    let side_color = if matches!(f.side, OrderSide::Buy) {
                        Color32::from_rgb(0, 200, 80)
                    } else {
                        Color32::from_rgb(220, 60, 60)
                    };
                    let side_str = if matches!(f.side, OrderSide::Buy) { "BUY" } else { "SEL" };
                    let total = f.price * f.size;
                    if matches!(f.side, OrderSide::Buy) { net += total; } else { net -= total; }
                    let out_lbl = &f.outcome[..f.outcome.len().min(4)];

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:<4}", out_lbl)).color(Color32::WHITE).size(10.0).monospace());
                        ui.label(RichText::new(format!("{:<3}", side_str)).color(side_color).size(10.0).monospace().strong());
                        ui.label(RichText::new(format!("{:.3}", f.price)).color(Color32::from_rgb(200,200,200)).size(10.0).monospace());
                        ui.label(RichText::new(format!("{:.1}", f.size)).color(Color32::GRAY).size(10.0).monospace());
                        ui.label(RichText::new(&f.time).color(Color32::from_rgb(160,160,200)).size(9.5).monospace());
                        ui.label(RichText::new(&f.session).color(Color32::from_rgb(120,180,120)).size(9.5).monospace());
                    });
                }

                ui.separator();
                let net_col = if net >= 0.0 { Color32::from_rgb(220, 160, 50) } else { Color32::from_rgb(100, 220, 120) };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Net:").color(Color32::GRAY).size(10.0));
                    ui.label(RichText::new(format!("${:.2}", net)).color(net_col).size(11.0).monospace().strong());
                });
            }
        });
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Viewport: Trading — Panel de ejecucion de ordenes
// ─────────────────────────────────────────────────────────────────────────────

fn draw_trading_viewport(
    ui: &mut egui::Ui,
    has_creds: bool,
    cmd_tx: &tokio_mpsc::UnboundedSender<CmdMsg>,
    trading_st: &Arc<Mutex<TradingState>>,
    market: Option<&MarketInfo>,
) {
    let Ok(mut ts) = trading_st.lock() else { return; };

    // ── SELECTOR DE OUTCOME — BIG & OBVIOUS ──────────────────────────────────
    let (oc_bg, oc_arrow, oc_text, oc_subtext) = if ts.outcome == Outcome::Up {
        (Color32::from_rgb(0, 100, 30), "▲", "UP", "BTC SUBE — compras el outcome UP")
    } else {
        (Color32::from_rgb(120, 40, 0), "▼", "DOWN", "BTC BAJA — compras el outcome DOWN")
    };
    let market_name = market.map(|m|
        if ts.outcome == Outcome::Up { m.outcome_up.as_str() } else { m.outcome_down.as_str() }
    ).unwrap_or(oc_text);

    egui::Frame::none().fill(oc_bg).inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{oc_arrow}  {oc_text}  [{market_name}]"))
                .color(Color32::WHITE).size(18.0).strong());
            ui.add_space(8.0);
            ui.label(RichText::new(oc_subtext).color(Color32::from_rgb(200, 220, 200)).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dn_fill = if ts.outcome == Outcome::Down { Color32::from_rgb(180, 70, 0) }
                              else { Color32::from_rgb(60, 35, 25) };
                if ui.add(egui::Button::new(RichText::new("▼ DOWN").color(Color32::WHITE).size(13.0).strong())
                    .fill(dn_fill).min_size(Vec2::new(80.0, 28.0))).clicked() {
                    ts.outcome = Outcome::Down;
                }
                let up_fill = if ts.outcome == Outcome::Up { Color32::from_rgb(0, 140, 45) }
                              else { Color32::from_rgb(25, 40, 25) };
                if ui.add(egui::Button::new(RichText::new("▲ UP").color(Color32::WHITE).size(13.0).strong())
                    .fill(up_fill).min_size(Vec2::new(70.0, 28.0))).clicked() {
                    ts.outcome = Outcome::Up;
                }
                ui.label(RichText::new("Cambiar:").color(Color32::GRAY).size(11.0));
            });
        });
    });

    ui.add_space(5.0);

    ui.add_space(6.0);

    // ── Inputs: monto y precio ────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label(RichText::new("Monto:").color(Color32::GRAY).size(12.0));
        ui.add_enabled(has_creds, egui::TextEdit::singleline(&mut ts.amount)
            .desired_width(85.0).hint_text("USDC / shares"));
        ui.add_space(8.0);
        ui.label(RichText::new("Precio:").color(Color32::GRAY).size(12.0));
        ui.add_enabled(has_creds, egui::TextEdit::singleline(&mut ts.price)
            .desired_width(72.0).hint_text("0.01-0.99"));
    });

    ui.add_space(5.0);

    let outcome    = ts.outcome;
    let amount_str = ts.amount.clone();
    let price_str  = ts.price.clone();
    let (oc_label, buy_col) = if outcome == Outcome::Up {
        ("UP ▲", Color32::from_rgb(0, 160, 60))
    } else {
        ("DN ▼", Color32::from_rgb(0, 120, 50))
    };

    // ── Botones Limit / Market ────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui.add_enabled(has_creds, egui::Button::new(
            RichText::new(format!("BUY {oc_label} LMT")).color(Color32::BLACK).size(13.0).strong())
            .fill(buy_col).min_size(Vec2::new(120.0, 26.0))).clicked()
        {
            if let (Ok(sz), Ok(px)) = (amount_str.trim().parse::<f64>(), price_str.trim().parse::<f64>()) {
                let _ = cmd_tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Buy, outcome, price: px, size: sz });
                ts.feedback = format!("BUY {oc_label} LMT {sz} @ {px}");
            } else { ts.feedback = "⚠ Valores invalidos".into(); }
        }
        ui.add_space(4.0);
        if ui.add_enabled(has_creds, egui::Button::new(
            RichText::new(format!("SELL {oc_label} LMT")).color(Color32::BLACK).size(13.0).strong())
            .fill(Color32::from_rgb(200, 50, 50)).min_size(Vec2::new(120.0, 26.0))).clicked()
        {
            if let (Ok(sz), Ok(px)) = (amount_str.trim().parse::<f64>(), price_str.trim().parse::<f64>()) {
                let _ = cmd_tx.send(CmdMsg::PlaceLimitOrder { side: OrderSide::Sell, outcome, price: px, size: sz });
                ts.feedback = format!("SELL {oc_label} LMT {sz} @ {px}");
            } else { ts.feedback = "⚠ Valores invalidos".into(); }
        }
        ui.add_space(8.0);
        if ui.add_enabled(has_creds, egui::Button::new(
            RichText::new(format!("BUY {oc_label} MKT")).color(Color32::BLACK).size(12.0))
            .fill(Color32::from_rgb(0, 130, 50))).clicked()
        {
            if let Ok(am) = amount_str.trim().parse::<f64>() {
                let _ = cmd_tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Buy, outcome, amount_usdc: am });
                ts.feedback = format!("BUY {oc_label} MKT ${am}");
            } else { ts.feedback = "⚠ Monto invalido".into(); }
        }
        ui.add_space(4.0);
        if ui.add_enabled(has_creds, egui::Button::new(
            RichText::new(format!("SELL {oc_label} MKT")).color(Color32::BLACK).size(12.0))
            .fill(Color32::from_rgb(160, 30, 30))).clicked()
        {
            if let Ok(am) = amount_str.trim().parse::<f64>() {
                let _ = cmd_tx.send(CmdMsg::PlaceMarketOrder { side: OrderSide::Sell, outcome, amount_usdc: am });
                ts.feedback = format!("SELL {oc_label} MKT ${am}");
            } else { ts.feedback = "⚠ Monto invalido".into(); }
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    // ── SCALP MODE ────────────────────────────────────────────────────────────
    egui::Frame::none()
        .fill(Color32::from_rgb(18, 28, 45))
        .inner_margin(egui::Margin::symmetric(8.0, 7.0))
        .rounding(5.0)
        .show(ui, |ui: &mut egui::Ui| {
        ui.label(RichText::new("SCALP MODE").color(Color32::from_rgb(255, 215, 50)).size(13.0).strong());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Target:").color(Color32::GRAY).size(11.0));
            ui.add_space(4.0);
            let pct_fill = if ts.scalp_use_pct  { Color32::from_rgb(50, 120, 200) } else { Color32::from_rgb(28, 38, 55) };
            let pts_fill = if !ts.scalp_use_pct { Color32::from_rgb(50, 120, 200) } else { Color32::from_rgb(28, 38, 55) };
            if ui.add(egui::Button::new(RichText::new("%").color(Color32::WHITE).size(11.0))
                .fill(pct_fill).min_size(Vec2::new(22.0, 18.0))).clicked() { ts.scalp_use_pct = true; }
            if ui.add(egui::Button::new(RichText::new("pts").color(Color32::WHITE).size(11.0))
                .fill(pts_fill).min_size(Vec2::new(28.0, 18.0))).clicked() { ts.scalp_use_pct = false; }
            ui.add_space(6.0);
            if ts.scalp_use_pct {
                ui.label(RichText::new(format!("{:.1}%", ts.scalp_profit_pct))
                    .color(Color32::from_rgb(100, 220, 255)).size(12.0).monospace());
                ui.add(egui::Slider::new(&mut ts.scalp_profit_pct, 0.5..=20.0).step_by(0.5).show_value(false));
            } else {
                ui.label(RichText::new(format!("+{:.3}", ts.scalp_profit_pts))
                    .color(Color32::from_rgb(100, 220, 255)).size(12.0).monospace());
                ui.add(egui::Slider::new(&mut ts.scalp_profit_pts, 0.005..=0.20).step_by(0.005).show_value(false));
            }
        });
        ui.add_space(4.0);
        if let Ok(px) = price_str.trim().parse::<f64>() {
            let target = if ts.scalp_use_pct { px * (1.0 + ts.scalp_profit_pct / 100.0) }
                         else                { px + ts.scalp_profit_pts };
            ui.label(RichText::new(format!("Compra @ {px:.4}  ->  Venta @ {target:.4}"))
                .color(Color32::from_rgb(140, 200, 140)).size(11.0));
            ui.add_space(4.0);
        }
        let scalp_label = format!("SCALP BUY {oc_label}  (vende automatico al fill)");
        let scw = ui.available_width();
        if ui.add_enabled(has_creds, egui::Button::new(
            RichText::new(&scalp_label).color(Color32::BLACK).size(13.0).strong())
            .fill(Color32::from_rgb(30, 185, 100))
            .min_size(Vec2::new(scw, 30.0))).clicked()
        {
            if let (Ok(sz), Ok(px)) = (amount_str.trim().parse::<f64>(), price_str.trim().parse::<f64>()) {
                let target = if ts.scalp_use_pct { px * (1.0 + ts.scalp_profit_pct / 100.0) }
                             else                { px + ts.scalp_profit_pts };
                let _ = cmd_tx.send(CmdMsg::ScalpBuy { outcome, price: px, size: sz, target_price: target });
                ts.feedback = format!("SCALP {oc_label} {sz} @ {px:.4} -> sell @ {target:.4}");
            } else { ts.feedback = "⚠ Valores invalidos para scalp".into(); }
        }
    });

    ui.add_space(6.0);

    // ── Feedback ─────────────────────────────────────────────────────────────
    if !ts.feedback.is_empty() {
        let col = if ts.feedback.starts_with('✓') { Color32::from_rgb(0, 220, 100) }
                  else if ts.feedback.starts_with("⚠") || ts.feedback.starts_with('✗') { Color32::from_rgb(240, 100, 60) }
                  else { Color32::GRAY };
        ui.label(RichText::new(&ts.feedback).color(col).size(11.0));
        ui.add_space(4.0);
    }

    // ── PANIC CASH OUT ────────────────────────────────────────────────────────
    let pw = ui.available_width();
    if ui.add_enabled(has_creds, egui::Button::new(
        RichText::new("CANCELAR TODO  +  CASH OUT EMERGENCIA")
            .color(Color32::WHITE).size(14.0).strong())
        .fill(Color32::from_rgb(185, 0, 0))
        .min_size(Vec2::new(pw, 40.0))).clicked()
    {
        let _ = cmd_tx.send(CmdMsg::CancelMarket);
        ts.feedback = "CANCELANDO TODAS LAS ORDENES...".into();
    }

    if !has_creds {
        ui.add_space(4.0);
        ui.label(RichText::new("Configure las credenciales en .env para operar").color(Color32::YELLOW).size(11.0));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Graficos: Tabs de indicadores
// ─────────────────────────────────────────────────────────────────────────────

fn draw_indicator_tabs(
    ui: &mut egui::Ui,
    tab: &mut ChartTab,
    settings: &mut IndicatorSettings,
    settings_open: &mut bool,
) {
    ui.horizontal(|ui| {
        for (t, label) in [(ChartTab::Macd,"MACD"),(ChartTab::Rsi,"RSI"),(ChartTab::Vfi,"VFI"),(ChartTab::Depth,"Prof.")] {
            let sel = *tab == t;
            let (tc, fill) = if sel { (Color32::BLACK, Color32::from_rgb(100, 200, 255)) }
                             else   { (Color32::GRAY, Color32::TRANSPARENT) };
            if ui.add(egui::Button::new(RichText::new(label).color(tc).size(11.0)).fill(fill)).clicked() {
                *tab = t;
            }
        }
        ui.add_space(10.0);
        let cfg_col = if *settings_open { Color32::from_rgb(255, 200, 50) } else { Color32::GRAY };
        if ui.button(RichText::new("⚙ Config").color(cfg_col).size(11.0)).clicked() {
            *settings_open = !*settings_open;
        }
    });
    if *settings_open {
        draw_settings_panel(ui, settings, tab);
    }
}

fn draw_settings_panel(ui: &mut egui::Ui, s: &mut IndicatorSettings, tab: &ChartTab) {
    egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
        ui.add_space(4.0);
        match tab {
            ChartTab::Macd => {
                ui.label(RichText::new("MACD").color(Color32::from_rgb(100, 200, 255)).strong());
                ui.horizontal(|ui| {
                    ui.label("Fast:"); ui.add(egui::Slider::new(&mut s.macd_fast, 2..=50));
                    ui.label("Slow:"); ui.add(egui::Slider::new(&mut s.macd_slow, 5..=100));
                    ui.label("Sig:");  ui.add(egui::Slider::new(&mut s.macd_signal, 2..=30));
                });
            }
            ChartTab::Rsi => {
                ui.label(RichText::new("RSI").color(Color32::from_rgb(180, 120, 255)).strong());
                ui.horizontal(|ui| {
                    ui.label("Per:"); ui.add(egui::Slider::new(&mut s.rsi_period, 2..=50));
                    ui.label("OB:");  ui.add(egui::Slider::new(&mut s.rsi_ob, 50.0..=95.0));
                    ui.label("OS:");  ui.add(egui::Slider::new(&mut s.rsi_os, 5.0..=50.0));
                });
            }
            ChartTab::Vfi => {
                ui.label(RichText::new("VFI (Katsanos)").color(Color32::from_rgb(255, 180, 50)).strong());
                ui.horizontal(|ui| {
                    ui.label("Per:"); ui.add(egui::Slider::new(&mut s.vfi_period, 10..=300));
                    ui.label("Coe:"); ui.add(egui::Slider::new(&mut s.vfi_coeff, 0.01..=1.0));
                    ui.label("VC:");  ui.add(egui::Slider::new(&mut s.vfi_vcoeff, 0.5..=10.0));
                    ui.label("Sm:"); ui.add(egui::Slider::new(&mut s.vfi_smooth, 1..=20));
                });
            }
            ChartTab::Depth => {
                ui.label(RichText::new("Bollinger (sobre velas)").color(Color32::from_rgb(100, 160, 255)).strong());
                ui.horizontal(|ui| {
                    ui.label("Per:"); ui.add(egui::Slider::new(&mut s.bb_period, 5..=100));
                    ui.label("SD:");  ui.add(egui::Slider::new(&mut s.bb_std, 0.5..=4.0));
                });
            }
        }
        ui.add_space(4.0);
    });
}

// ─── Candlestick + Bollinger ──────────────────────────────────────────────────

fn draw_candlestick_bb(ui: &mut egui::Ui, candles: &[Candle], s: &IndicatorSettings, width: f32, height: f32) {
    let n = candles.len();
    let bar_w = 0.6_f64;
    let view = 120_usize;

    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let bands = bollinger_bands(&closes, s.bb_period, s.bb_std);
    let bb_off = n.saturating_sub(bands.len());

    Plot::new("btc_candles_bb").width(width).height(height)
        .show_axes([false, true]).show_grid([false, true])
        .allow_drag(true).allow_zoom(true)
        .show(ui, |plot_ui| {
            for (i, c) in candles.iter().enumerate() {
                let x = i as f64;
                let is_bull = c.close >= c.open;
                let col = if is_bull { Color32::from_rgb(0, 200, 80) } else { Color32::from_rgb(220, 60, 60) };
                let (lo, hi) = if is_bull { (c.open, c.close) } else { (c.close, c.open) };
                let body = Polygon::new(PlotPoints::new(vec![
                    [x - bar_w/2.0, lo],[x + bar_w/2.0, lo],
                    [x + bar_w/2.0, hi],[x - bar_w/2.0, hi],
                ])).fill_color(col).stroke(egui::Stroke::NONE);
                plot_ui.polygon(body);
                plot_ui.line(Line::new(PlotPoints::new(vec![[x,hi],[x,c.high]])).color(col).width(1.0));
                plot_ui.line(Line::new(PlotPoints::new(vec![[x,lo],[x,c.low ]])).color(col).width(1.0));
            }
            if !bands.is_empty() {
                let bb_col = Color32::from_rgba_unmultiplied(100, 160, 255, 180);
                plot_ui.line(Line::new(bands.iter().enumerate().map(|(i,(m,_,_))| [(bb_off+i) as f64,*m]).collect::<PlotPoints>())
                    .color(Color32::from_rgb(255,200,50)).width(1.0).name("SMA"));
                plot_ui.line(Line::new(bands.iter().enumerate().map(|(i,(_,u,_))| [(bb_off+i) as f64,*u]).collect::<PlotPoints>())
                    .color(bb_col).width(1.0).name("BB+"));
                plot_ui.line(Line::new(bands.iter().enumerate().map(|(i,(_,_,l))| [(bb_off+i) as f64,*l]).collect::<PlotPoints>())
                    .color(bb_col).width(1.0).name("BB-"));
            }
            let x_max = n as f64;
            let x_min = (x_max - view as f64).max(0.0);
            let start = x_min as usize;
            let prices: Vec<f64> = candles[start..].iter().flat_map(|c| [c.low, c.high]).collect();
            let y_min = prices.iter().cloned().fold(f64::INFINITY, f64::min);
            let y_max = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let m = (y_max - y_min) * 0.06;
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([x_min, y_min-m], [x_max, y_max+m]));
        });
}

// ─── Volumen ──────────────────────────────────────────────────────────────────

fn draw_volume_bars(ui: &mut egui::Ui, candles: &[Candle], width: f32, height: f32) {
    let n = candles.len();
    let view = 120_usize;
    let x_min = (n as f64 - view as f64).max(0.0);
    let x_max = n as f64;
    let max_vol = candles[x_min as usize..].iter().map(|c| c.volume).fold(0.0_f64, f64::max);
    let y_max = if max_vol > 0.0 { max_vol * 1.15 } else { 1.0 };

    let bars: Vec<Bar> = candles.iter().enumerate().map(|(i, c)| {
        let col = if c.close >= c.open { Color32::from_rgba_unmultiplied(0,180,80,200) }
                  else { Color32::from_rgba_unmultiplied(200,50,50,200) };
        Bar::new(i as f64, c.volume).width(0.8).fill(col).stroke(egui::Stroke::NONE)
    }).collect();

    ui.label(RichText::new(format!("Vol BTC  max {:.1}", max_vol)).color(Color32::GRAY).size(9.0));
    Plot::new("btc_volume").width(width).height(height)
        .show_axes([false, true]).show_grid([false, false])
        .allow_drag(false).allow_zoom(false)
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(BarChart::new(bars).name("Vol"));
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([x_min, 0.0], [x_max, y_max]));
        });
}

// ─── Indicadores ─────────────────────────────────────────────────────────────

fn ema(data: &[f64], period: usize) -> Vec<f64> {
    if data.len() < period { return vec![]; }
    let k = 2.0 / (period as f64 + 1.0);
    let mut r = Vec::with_capacity(data.len());
    r.push(data[..period].iter().sum::<f64>() / period as f64);
    for &v in &data[period..] {
        let prev = *r.last().unwrap();
        r.push(v * k + prev * (1.0 - k));
    }
    r
}

fn rsi_series(closes: &[f64], period: usize) -> Vec<(f64, f64)> {
    if closes.len() < period + 1 { return vec![]; }
    let (mut ag, mut al) = (0.0_f64, 0.0_f64);
    for i in 1..=period {
        let d = closes[i] - closes[i-1];
        if d > 0.0 { ag += d } else { al -= d }
    }
    ag /= period as f64; al /= period as f64;
    let mut out = vec![(period as f64, if al == 0.0 { 100.0 } else { 100.0 - 100.0/(1.0+ag/al) })];
    for i in (period+1)..closes.len() {
        let d = closes[i] - closes[i-1];
        ag = (ag*(period as f64 - 1.0) + if d>0.0 {d} else {0.0}) / period as f64;
        al = (al*(period as f64 - 1.0) + if d<0.0 {-d} else {0.0}) / period as f64;
        out.push((i as f64, if al == 0.0 { 100.0 } else { 100.0 - 100.0/(1.0+ag/al) }));
    }
    out
}

fn bollinger_bands(closes: &[f64], period: usize, std_mult: f64) -> Vec<(f64, f64, f64)> {
    if closes.len() < period { return vec![]; }
    (period..=closes.len()).map(|i| {
        let w = &closes[i-period..i];
        let mean = w.iter().sum::<f64>() / period as f64;
        let std = (w.iter().map(|&v| (v-mean).powi(2)).sum::<f64>() / period as f64).sqrt();
        (mean, mean + std_mult*std, mean - std_mult*std)
    }).collect()
}

fn draw_macd(ui: &mut egui::Ui, candles: &[Candle], s: &IndicatorSettings, height: f32) {
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let ef = ema(&closes, s.macd_fast);
    let es = ema(&closes, s.macd_slow);
    let min_len = ef.len().min(es.len());
    let off = closes.len() - min_len;
    let macd: Vec<f64> = (0..min_len).map(|i| ef[ef.len()-min_len+i] - es[es.len()-min_len+i]).collect();
    let signal = ema(&macd, s.macd_signal);
    let sig_off = macd.len() - signal.len();
    let hist: Vec<(f64,f64)> = signal.iter().enumerate().map(|(i,&sv)| {
        ((off+sig_off+i) as f64, macd[sig_off+i] - sv)
    }).collect();

    Plot::new("macd").height(height).show_axes([false,true]).show_grid([false,true])
        .allow_drag(true).allow_zoom(true)
        .show(ui, |plot_ui| {
            let bars: Vec<Bar> = hist.iter().map(|&(x,h)| {
                let col = if h >= 0.0 { Color32::from_rgb(0,180,80) } else { Color32::from_rgb(200,50,50) };
                Bar::new(x, h).width(0.6).fill(col).stroke(egui::Stroke::NONE)
            }).collect();
            plot_ui.bar_chart(BarChart::new(bars).name("Hist"));
            plot_ui.line(Line::new(macd.iter().enumerate().map(|(i,&v)| [(off+i) as f64,v]).collect::<PlotPoints>())
                .color(Color32::from_rgb(100,180,255)).width(1.5).name("MACD"));
            plot_ui.line(Line::new(signal.iter().enumerate().map(|(i,&v)| [(off+sig_off+i) as f64,v]).collect::<PlotPoints>())
                .color(Color32::from_rgb(255,160,50)).width(1.5).name("Signal"));

            let n = closes.len() as f64;
            let x0 = (n - 120.0).max(0.0);
            let y_lo = hist.iter().filter(|(x,_)| *x>=x0).map(|(_,h)| *h).fold(0.0_f64, f64::min);
            let y_hi = hist.iter().filter(|(x,_)| *x>=x0).map(|(_,h)| *h).fold(0.0_f64, f64::max);
            let margin = (y_hi - y_lo).abs() * 0.15 + 0.001;
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([x0, y_lo-margin], [n, y_hi+margin]));
        });
}

fn draw_rsi(ui: &mut egui::Ui, candles: &[Candle], s: &IndicatorSettings, height: f32) {
    let closes: Vec<f64> = candles.iter().map(|c| c.close).collect();
    let series = rsi_series(&closes, s.rsi_period);
    Plot::new("rsi").height(height).show_axes([false,true]).show_grid([false,true])
        .include_y(0.0).include_y(100.0).allow_drag(true).allow_zoom(true)
        .show(ui, |plot_ui| {
            let n = closes.len() as f64;
            let x0 = (n - 120.0).max(0.0);
            plot_ui.line(Line::new(PlotPoints::new(vec![[x0,s.rsi_ob],[n,s.rsi_ob]]))
                .color(Color32::from_rgba_unmultiplied(220,60,60,120)).width(1.0));
            plot_ui.line(Line::new(PlotPoints::new(vec![[x0,s.rsi_os],[n,s.rsi_os]]))
                .color(Color32::from_rgba_unmultiplied(0,200,80,120)).width(1.0));
            plot_ui.line(Line::new(series.iter().map(|&(x,y)| [x,y]).collect::<PlotPoints>())
                .color(Color32::from_rgb(180,120,255)).width(2.0).name(format!("RSI {}",s.rsi_period)));
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([x0,0.0],[n,100.0]));
        });
}

fn vfi_series(candles: &[Candle], period: usize, coeff: f64, vcoeff: f64, smooth: usize) -> Vec<f64> {
    let n = candles.len();
    if n < period + 2 { return vec![]; }
    let typ: Vec<f64> = candles.iter().map(|c| (c.high+c.low+c.close)/3.0).collect();
    let inter: Vec<f64> = (1..n).map(|i| (typ[i]/typ[i-1]).ln()).collect();
    let sw = period.min(30);
    let vinter: Vec<f64> = (0..inter.len()).map(|i| {
        let st = if i+1>=sw { i+1-sw } else { 0 };
        let sl = &inter[st..=i];
        let m = sl.iter().sum::<f64>() / sl.len() as f64;
        (sl.iter().map(|&v| (v-m).powi(2)).sum::<f64>() / sl.len() as f64).sqrt()
    }).collect();
    let vols: Vec<f64> = candles.iter().map(|c| c.volume).collect();
    let vave = ema(&vols, period);
    let vave_off = vols.len() - vave.len();
    let mut mfv: Vec<f64> = Vec::with_capacity(inter.len());
    for i in 0..inter.len() {
        let cutoff = coeff * vinter[i] * typ[i+1];
        let vi = if i >= vave_off { vave[i-vave_off] } else { vave[0] };
        let vc = candles[i+1].volume.min(vi * vcoeff);
        mfv.push(if inter[i] > cutoff { vc } else if inter[i] < -cutoff { -vc } else { 0.0 });
    }
    let mut vfi_raw: Vec<f64> = Vec::new();
    for i in (period-1)..mfv.len() {
        let sum: f64 = mfv[i+1-period..=i].iter().sum();
        let vi = if i >= vave_off { vave[i-vave_off] } else { vave[0] };
        vfi_raw.push(if vi > 0.0 { sum/vi } else { 0.0 });
    }
    if smooth <= 1 || vfi_raw.len() < smooth { return vfi_raw; }
    ema(&vfi_raw, smooth)
}

fn draw_vfi(ui: &mut egui::Ui, candles: &[Candle], s: &IndicatorSettings, height: f32) {
    let series = vfi_series(candles, s.vfi_period, s.vfi_coeff, s.vfi_vcoeff, s.vfi_smooth);
    if series.is_empty() {
        ui.label(RichText::new(format!("VFI: necesita >={} velas", s.vfi_period+2)).color(Color32::GRAY).size(11.0));
        return;
    }
    let n_total = candles.len();
    let offset  = n_total - series.len();
    let view    = 120_usize;
    let x_max   = n_total as f64;
    let x_min   = (x_max - view as f64).max(0.0);

    let signal  = ema(&series, s.vfi_smooth.max(3));
    let sig_off = series.len() - signal.len();

    let hist_bars: Vec<Bar> = signal.iter().enumerate().map(|(i, &sig)| {
        let diff = series[sig_off+i] - sig;
        let x = (offset+sig_off+i) as f64;
        let col = if diff >= 0.0 { Color32::from_rgba_unmultiplied(0,160,80,160) }
                  else { Color32::from_rgba_unmultiplied(180,40,40,160) };
        Bar::new(x, diff).width(0.7).fill(col).stroke(egui::Stroke::NONE)
    }).collect();

    // Bounds correctos con margen proporcional al rango
    let vis_start = (x_min as usize).saturating_sub(offset);
    let vis_vals: Vec<f64> = series[vis_start..].to_vec();
    let y_min_v = vis_vals.iter().cloned().fold(0.0_f64, f64::min);
    let y_max_v = vis_vals.iter().cloned().fold(0.0_f64, f64::max);
    let margin  = (y_max_v - y_min_v).abs() * 0.12 + 0.05;
    let y_lo    = (y_min_v - margin).min(-0.1);
    let y_hi    = (y_max_v + margin).max(0.1);

    Plot::new("vfi").height(height).show_axes([false,true]).show_grid([false,true])
        .allow_drag(true).allow_zoom(true)
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(PlotPoints::new(vec![[x_min,0.0],[x_max,0.0]]))
                .color(Color32::from_rgba_unmultiplied(150,150,150,80)).width(1.0));
            plot_ui.bar_chart(BarChart::new(hist_bars).name("VFI-Sig"));
            plot_ui.line(Line::new(series.iter().enumerate().map(|(i,&v)| [(offset+i) as f64,v]).collect::<PlotPoints>())
                .color(Color32::from_rgb(255,180,50)).width(2.0).name(format!("VFI {}",s.vfi_period)));
            plot_ui.line(Line::new(signal.iter().enumerate().map(|(i,&v)| [(offset+sig_off+i) as f64,v]).collect::<PlotPoints>())
                .color(Color32::from_rgb(100,220,255)).width(1.5).name(format!("Sig {}",s.vfi_smooth)));
            plot_ui.set_plot_bounds(PlotBounds::from_min_max([x_min,y_lo],[x_max,y_hi]));
        });
}

// ─── Profundidad ──────────────────────────────────────────────────────────────

fn draw_depth(ui: &mut egui::Ui, book_up: Option<&BookSnapshot>, book_down: Option<&BookSnapshot>, height: f32) {
    let total_w = ui.available_width();
    let half_w  = (total_w - 16.0) / 2.0;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(half_w);
            draw_depth_single(ui, book_up, half_w, height,
                Color32::from_rgb(0,200,80), Color32::from_rgb(220,60,60), "UP");
        });
        ui.add_space(8.0); ui.separator(); ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.set_width(half_w);
            draw_depth_single(ui, book_down, half_w, height,
                Color32::from_rgb(0,200,80), Color32::from_rgb(220,60,60), "DOWN");
        });
    });
}

fn draw_depth_single(
    ui: &mut egui::Ui, book: Option<&BookSnapshot>, width: f32, height: f32,
    bid_color: Color32, ask_color: Color32, label: &str,
) {
    ui.label(RichText::new(format!("Profundidad {label}")).color(Color32::GRAY).size(11.0));
    let Some(b) = book else {
        ui.label(RichText::new("Sin datos").color(Color32::GRAY).size(11.0));
        return;
    };
    let mut bids = b.bids.clone();
    let mut asks = b.asks.clone();
    bids.sort_by(|a, x| x.price.partial_cmp(&a.price).unwrap_or(std::cmp::Ordering::Equal));
    asks.sort_by(|a, x| a.price.partial_cmp(&x.price).unwrap_or(std::cmp::Ordering::Equal));
    let mut bid_cum = 0.0_f64;
    let bid_pts: PlotPoints = bids.iter().map(|l| { bid_cum += l.size; [l.price, bid_cum] }).collect();
    let mut ask_cum = 0.0_f64;
    let ask_pts: PlotPoints = asks.iter().map(|l| { ask_cum += l.size; [l.price, ask_cum] }).collect();
    let max_cum = bid_cum.max(ask_cum);
    let best_bid = bids.first().map(|l| l.price).unwrap_or(0.5);
    let best_ask = asks.first().map(|l| l.price).unwrap_or(0.5);
    let x_margin = (best_ask - best_bid).abs() * 8.0;
    Plot::new(format!("depth_{label}")).width(width).height(height)
        .show_axes([true,true]).show_grid([false,true]).allow_drag(true).allow_zoom(true)
        .show(ui, |plot_ui| {
            plot_ui.line(Line::new(bid_pts).color(bid_color).width(2.0).fill(0.0).name("Bids"));
            plot_ui.line(Line::new(ask_pts).color(ask_color).width(2.0).fill(0.0).name("Asks"));
            plot_ui.set_plot_bounds(PlotBounds::from_min_max(
                [(best_bid - x_margin).max(0.0), 0.0],
                [(best_ask + x_margin).min(1.0), max_cum * 1.1],
            ));
        });
}
