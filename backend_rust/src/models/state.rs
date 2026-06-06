use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::models::credentials::ClobCredentials;
use crate::controllers::worker::{BookSnapshot, Candle, CandleInterval, CmdMsg, MarketInfo, OpenOrder, RecentFill};
use crate::models::hft::BinanceDepth;
use crate::models::hft::PriceRingBuffer;
use crate::services::metrics::TrackingState;
use crate::services::binance::BinanceTickEvent;
use crate::models::hft::CsvRecord;
use crate::models::hft::PolyDepthFrame;
use crate::services::session::SessionManager;

#[derive(Debug, Clone, Serialize, Default)]
pub struct LatestHftState {
    pub time: String,
    pub event: String,
    pub btc_price: f64,
    pub mid: f64,
    pub spread: f64,
    pub imbalance: f64,
    pub bid_vol: f64,
    pub ask_vol: f64,
    pub clob_trade_up: f64,
    pub clob_trade_dn: f64,
    pub clob_trade_up_vol: f64,
    pub clob_trade_dn_vol: f64,
    pub btc_vel: f64,
    pub btc_acel: f64,
    pub btc_volatility: f64,
    pub btc_volume_24h: f64,
    pub btc_vol_1m: f64,
    pub btc_vol_ses: f64,
    pub btc_vol: f64,
    pub spoof: u8,
    pub dump_score: u8,
    pub ask_wall: u8,
    pub tick_gap_ms: i64,
    pub secs_left: i32,
    pub od83_up: u8,
    pub od83_dn: u8,
    pub hd65_up: u8,
    pub hd65_dn: u8,
    pub od83_up_bal: f64,
    pub od83_dn_bal: f64,
    pub hd65_up_bal: f64,
    pub hd65_dn_bal: f64,
    pub od83_filters: u16,
    pub od83_event: String,
    pub hd65_event: String,
    pub sen_up: u8,
    pub sen_dn: u8,
    pub sen_up_bal: f64,
    pub sen_dn_bal: f64,
    pub sen_event: String,
    pub sen_clob_delta: f64,
    pub sen_btc_vel: f64,
    pub depth_up_bids: Vec<(f64,f64)>,
    pub depth_up_asks: Vec<(f64,f64)>,
    pub depth_dn_bids: Vec<(f64,f64)>,
    pub depth_dn_asks: Vec<(f64,f64)>,
    pub ofi_up: f64,
    pub ofi_dn: f64,
    pub micro_price_up: f64,
    pub micro_price_dn: f64,
}

pub struct AppState {
    pub status:          RwLock<String>,
    pub market:          RwLock<Option<MarketInfo>>,
    pub book_up:         RwLock<Option<BookSnapshot>>,
    pub book_down:       RwLock<Option<BookSnapshot>>,
    pub balance:         RwLock<Option<f64>>,
    pub btc_price:       RwLock<Option<f64>>,
    pub btc_volume:      RwLock<f64>,
    pub btc_vol_window:  RwLock<VecDeque<(i64, f64)>>,
    pub btc_vol_1m:      RwLock<f64>,
    pub btc_vol_ses:     RwLock<f64>,
    pub btc_open:        RwLock<Option<f64>>,
    pub trade_window_up:   RwLock<VecDeque<(f64,f64)>>,
    pub trade_window_dn:   RwLock<VecDeque<(f64,f64)>>,
    pub raw_trade_up:      RwLock<f64>,
    pub raw_trade_dn:      RwLock<f64>,
    pub best_bid_up:       RwLock<f64>,
    pub best_bid_dn:       RwLock<f64>,
    pub prev_raw_up:       RwLock<f64>,
    pub prev_raw_dn:       RwLock<f64>,
    pub trade_min_vol:   RwLock<f64>,
    pub trade_window_n:  RwLock<usize>,
    pub open_orders:     RwLock<Vec<OpenOrder>>,
    pub recent_fills:    RwLock<Vec<RecentFill>>,
    pub candles:         RwLock<Vec<Candle>>,

    pub interval_arc:    Arc<Mutex<CandleInterval>>,

    pub cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
    pub broadcast_tx:    broadcast::Sender<String>,

    pub creds:           ClobCredentials,

    pub recording_sessions: RwLock<Vec<i32>>,
    pub mem_hft:         RwLock<Vec<CsvRecord>>,
    pub binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
    pub binance_ring:    Arc<PriceRingBuffer>,
    pub tracking_state:  Arc<TrackingState>,
    pub tick_tx:         mpsc::UnboundedSender<BinanceTickEvent>,

    pub session_manager: Arc<SessionManager>,
    pub tick_drain: Arc<AtomicBool>,
    pub poly_depth_history: RwLock<VecDeque<PolyDepthFrame>>,
    pub diagnostic_mode: std::sync::atomic::AtomicBool,
    pub latest_hft: RwLock<LatestHftState>,
    pub latency_binance: RwLock<u64>,
    pub latency_poly:    RwLock<u64>,
    pub prev_btc_vel:    RwLock<f64>,
    pub btc_vol_ema:     RwLock<f64>,
}

impl AppState {
    pub fn new(
        cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
        broadcast_tx:    broadcast::Sender<String>,
        interval_arc:    Arc<Mutex<CandleInterval>>,
        binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
        binance_ring:    Arc<PriceRingBuffer>,
        tracking_state:  Arc<TrackingState>,
        tick_tx:         mpsc::UnboundedSender<BinanceTickEvent>,
        creds:           ClobCredentials,
    ) -> Arc<Self> {
        Arc::new(Self {
            status:            RwLock::new("Initializing".into()),
            market:            RwLock::new(None),
            book_up:           RwLock::new(None),
            book_down:         RwLock::new(None),
            balance:           RwLock::new(None),
            btc_price:         RwLock::new(None),
            btc_volume:        RwLock::new(0.0),
            btc_vol_window:    RwLock::new(VecDeque::with_capacity(256)),
            btc_vol_1m:        RwLock::new(0.0),
            btc_vol_ses:       RwLock::new(0.0),
            btc_open:          RwLock::new(None),
            trade_window_up:   RwLock::new(VecDeque::with_capacity(10)),
            trade_window_dn:   RwLock::new(VecDeque::with_capacity(10)),
            raw_trade_up:      RwLock::new(0.0),
            raw_trade_dn:      RwLock::new(0.0),
            best_bid_up:       RwLock::new(0.0),
            best_bid_dn:       RwLock::new(0.0),
            prev_raw_up:       RwLock::new(0.0),
            prev_raw_dn:       RwLock::new(0.0),
            trade_min_vol:     RwLock::new(5.0),
            trade_window_n:    RwLock::new(10),
            open_orders:       RwLock::new(vec![]),
            recent_fills:      RwLock::new(vec![]),
            candles:           RwLock::new(vec![]),
            interval_arc,
            cmd_tx,
            broadcast_tx,
            creds,
            recording_sessions: RwLock::new(vec![]),
            mem_hft:           RwLock::new(vec![]),
            binance_depth,
            binance_ring,
            tracking_state,
            tick_tx,
            session_manager:   Arc::new(SessionManager::new("sessions")),
            tick_drain:        Arc::new(AtomicBool::new(false)),
            poly_depth_history: RwLock::new(VecDeque::with_capacity(300)),
            diagnostic_mode: AtomicBool::new(false),
            latest_hft: RwLock::new(LatestHftState::default()),
            latency_binance:   RwLock::new(0),
            latency_poly:      RwLock::new(0),
            prev_btc_vel:      RwLock::new(0.0),
            btc_vol_ema:       RwLock::new(0.0),
        })
    }
}
