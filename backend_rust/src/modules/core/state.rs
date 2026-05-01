use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tokio::sync::{broadcast, mpsc, RwLock};
use sqlx::PgPool;

use crate::modules::core::worker::{BtcPriceProvider, BookSnapshot, Candle, CandleInterval, CmdMsg, MarketInfo, OpenOrder, RecentFill};
use crate::modules::db::models::{RecordingSession, SessionSnapshot, SessionTrade};
use crate::modules::hft::types::BinanceDepth;
use crate::modules::hft::ring_buffer::PriceRingBuffer;
use crate::modules::hft::metrics::TrackingState;
use crate::modules::hft::binance_depth::BinanceTickEvent;
use crate::modules::hft::logger::CsvLogger;
use crate::modules::hft::types::CsvRecord;
use crate::modules::hft::executor::StrategyManager;
use crate::modules::hft::session_manager::SessionManager;
use crate::modules::hft::adaptive_risk_engine::AdaptiveRiskEngine;
use crate::modules::hft::adaptive_risk_engine::MacroContext;

pub struct AppState {
    // Estado en memoria (actualizado por el consumer de AppMsg)
    pub status:          RwLock<String>,
    pub market:          RwLock<Option<MarketInfo>>,
    pub book_up:         RwLock<Option<BookSnapshot>>,
    pub book_down:       RwLock<Option<BookSnapshot>>,
    pub balance:         RwLock<Option<f64>>,
    pub btc_price:       RwLock<Option<f64>>,
    pub btc_open:        RwLock<Option<f64>>,
    pub last_trade_up:   RwLock<Option<f64>>,
    pub last_trade_down: RwLock<Option<f64>>,
    pub open_orders:     RwLock<Vec<OpenOrder>>,
    pub recent_fills:    RwLock<Vec<RecentFill>>,
    pub candles:         RwLock<Vec<Candle>>,

    pub interval_arc:    Arc<Mutex<CandleInterval>>,

    pub btc_provider:    RwLock<BtcPriceProvider>,
    pub btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,

    pub cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
    pub broadcast_tx:    broadcast::Sender<String>,
    pub shutdown_tx:     broadcast::Sender<()>,

    pub db:              Option<PgPool>,

    pub recording_sessions: RwLock<Vec<i32>>,
    pub mem_sessions:    RwLock<Vec<RecordingSession>>,
    pub mem_snapshots:   RwLock<Vec<SessionSnapshot>>,
    pub mem_trades:      RwLock<Vec<SessionTrade>>,

    // ─── HFT Module ─────────────────────────────────────────────────────────
    /// Buffer en memoria de HFT snapshots (fallback si no hay PostgreSQL)
    pub mem_hft:         RwLock<Vec<CsvRecord>>,
    pub binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
    pub binance_ring:    Arc<PriceRingBuffer>,
    pub tracking_state:  Arc<TrackingState>,
    pub csv_logger:      Arc<CsvLogger>,
    /// Canal para ticks de Binance (HFT → consumer)
    pub tick_tx:         mpsc::UnboundedSender<BinanceTickEvent>,

    /// Multi-strategy paper trading executor (Imbalance Divergence + Liquidity Grabbing)
    pub strategy_manager: Arc<Mutex<StrategyManager>>,

    /// Per-session CSV file manager (strict file isolation)
    pub session_manager: Arc<SessionManager>,

    /// Flag: set by scheduler on new session start → consumer drains tick channel
    pub tick_drain: Arc<AtomicBool>,

    /// Adaptive Risk Engine: macro 24h warm‑up + Conformal Prediction + feedback
    pub adaptive_engine: Arc<tokio::sync::Mutex<AdaptiveRiskEngine>>,

    /// Shared dynamic macro context (RSI, VFI confidence, accuracy factor) — updated each minute
    pub macro_ctx: Arc<RwLock<MacroContext>>,

    /// Latency tracking (ms) for health endpoint
    pub latency_binance: RwLock<u64>,
    pub latency_poly:    RwLock<u64>,

    #[cfg(feature = "premium-patterns")]
    pub patterns_config: RwLock<crate::modules::premium::patterns::models::DetectorConfig>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
        broadcast_tx:    broadcast::Sender<String>,
        shutdown_tx:     broadcast::Sender<()>,
        interval_arc:    Arc<Mutex<CandleInterval>>,
        db:              Option<PgPool>,
        btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,
        binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
        binance_ring:    Arc<PriceRingBuffer>,
        tracking_state:  Arc<TrackingState>,
        csv_logger:      Arc<CsvLogger>,
        tick_tx:         mpsc::UnboundedSender<BinanceTickEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            status:            RwLock::new("Initializing".into()),
            market:            RwLock::new(None),
            book_up:           RwLock::new(None),
            book_down:         RwLock::new(None),
            balance:           RwLock::new(None),
            btc_price:         RwLock::new(None),
            btc_open:          RwLock::new(None),
            last_trade_up:     RwLock::new(None),
            last_trade_down:   RwLock::new(None),
            open_orders:       RwLock::new(vec![]),
            recent_fills:      RwLock::new(vec![]),
            candles:           RwLock::new(vec![]),
            interval_arc,
            btc_provider:      RwLock::new(BtcPriceProvider::Coinbase),
            btc_provider_tx,
            cmd_tx,
            broadcast_tx,
            shutdown_tx,
            db,
            recording_sessions: RwLock::new(vec![]),
            mem_sessions:      RwLock::new(vec![]),
            mem_snapshots:     RwLock::new(vec![]),
            mem_trades:        RwLock::new(vec![]),
            mem_hft:           RwLock::new(vec![]),
            binance_depth,
            binance_ring,
            tracking_state,
            csv_logger,
            tick_tx,
            session_manager:   Arc::new(SessionManager::new("sessions")),
            strategy_manager:  Arc::new(Mutex::new(StrategyManager::new())),
            tick_drain:        Arc::new(AtomicBool::new(false)),
            adaptive_engine:   Arc::new(tokio::sync::Mutex::new(AdaptiveRiskEngine::new())),
            macro_ctx:         Arc::new(RwLock::new(MacroContext::default())),
            latency_binance:   RwLock::new(0),
            latency_poly:      RwLock::new(0),
            #[cfg(feature = "premium-patterns")]
            patterns_config: RwLock::new(crate::modules::premium::patterns::models::DetectorConfig::default()),
        })
    }
}
