use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::controllers::worker::{BookSnapshot, CmdMsg, MarketInfo, OpenOrder, RecentFill};
use crate::models::hft::BinanceDepth;
use crate::models::hft::CsvRecord;
use crate::models::hft::PriceRingBuffer;
use crate::services::binance::BinanceTickEvent;
use crate::services::metrics::TrackingState;
use crate::services::session::SessionManager;

/// Latest snapshot of the derived microstructure state, served by
/// `GET /api/hft/latest` and rendered by the TUI dashboard.
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
    pub btc_volume_24h: f64,
    pub btc_vol_1m: f64,
    pub btc_vol_ses: f64,
    pub btc_vol: f64,
    pub spoof: u8,
    pub dump_score: u8,
    pub ask_wall: u8,
    pub tick_gap_ms: i64,
    pub secs_left: i32,
    pub depth_up_bids: Vec<(f64, f64)>,
    pub depth_up_asks: Vec<(f64, f64)>,
    pub depth_dn_bids: Vec<(f64, f64)>,
    pub depth_dn_asks: Vec<(f64, f64)>,
    pub ofi_up: f64,
    pub ofi_dn: f64,
    pub micro_price_up: f64,
    pub micro_price_dn: f64,
}

pub struct AppState {
    pub status: RwLock<String>,
    pub market: RwLock<Option<MarketInfo>>,
    pub book_up: RwLock<Option<BookSnapshot>>,
    pub book_down: RwLock<Option<BookSnapshot>>,
    pub balance: RwLock<Option<f64>>,
    pub btc_price: RwLock<Option<f64>>,
    pub btc_volume: RwLock<f64>,
    pub btc_vol_window: RwLock<VecDeque<(i64, f64)>>,
    pub btc_vol_1m: RwLock<f64>,
    pub btc_vol_ses: RwLock<f64>,
    pub btc_open: RwLock<Option<f64>>,
    pub trade_window_up: RwLock<VecDeque<(f64, f64)>>,
    pub trade_window_dn: RwLock<VecDeque<(f64, f64)>>,
    pub trade_min_vol: RwLock<f64>,
    pub trade_window_n: RwLock<usize>,
    pub open_orders: RwLock<Vec<OpenOrder>>,
    pub recent_fills: RwLock<Vec<RecentFill>>,

    pub cmd_tx: mpsc::UnboundedSender<CmdMsg>,
    pub broadcast_tx: broadcast::Sender<String>,

    pub recording_sessions: RwLock<Vec<i32>>,
    pub mem_hft: RwLock<Vec<CsvRecord>>,
    pub binance_depth: Arc<RwLock<Option<BinanceDepth>>>,
    pub binance_ring: Arc<PriceRingBuffer>,
    pub tracking_state: Arc<TrackingState>,
    pub tick_tx: mpsc::UnboundedSender<BinanceTickEvent>,

    pub session_manager: Arc<SessionManager>,
    pub tick_drain: Arc<AtomicBool>,
    pub latest_hft: RwLock<LatestHftState>,
}

impl AppState {
    pub fn new(
        cmd_tx: mpsc::UnboundedSender<CmdMsg>,
        broadcast_tx: broadcast::Sender<String>,
        binance_depth: Arc<RwLock<Option<BinanceDepth>>>,
        binance_ring: Arc<PriceRingBuffer>,
        tracking_state: Arc<TrackingState>,
        tick_tx: mpsc::UnboundedSender<BinanceTickEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            status: RwLock::new("Initializing".into()),
            market: RwLock::new(None),
            book_up: RwLock::new(None),
            book_down: RwLock::new(None),
            balance: RwLock::new(None),
            btc_price: RwLock::new(None),
            btc_volume: RwLock::new(0.0),
            btc_vol_window: RwLock::new(VecDeque::with_capacity(256)),
            btc_vol_1m: RwLock::new(0.0),
            btc_vol_ses: RwLock::new(0.0),
            btc_open: RwLock::new(None),
            trade_window_up: RwLock::new(VecDeque::with_capacity(10)),
            trade_window_dn: RwLock::new(VecDeque::with_capacity(10)),
            trade_min_vol: RwLock::new(5.0),
            trade_window_n: RwLock::new(10),
            open_orders: RwLock::new(vec![]),
            recent_fills: RwLock::new(vec![]),
            cmd_tx,
            broadcast_tx,
            recording_sessions: RwLock::new(vec![]),
            mem_hft: RwLock::new(vec![]),
            binance_depth,
            binance_ring,
            tracking_state,
            tick_tx,
            session_manager: Arc::new(SessionManager::new("sessions")),
            tick_drain: Arc::new(AtomicBool::new(false)),
            latest_hft: RwLock::new(LatestHftState::default()),
        })
    }
}
