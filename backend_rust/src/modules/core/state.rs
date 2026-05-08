use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, mpsc, RwLock};
use sqlx::PgPool;

use crate::modules::core::credentials::ClobCredentials;
use crate::modules::core::worker::{BtcPriceProvider, BookSnapshot, Candle, CandleInterval, CmdMsg, MarketInfo, OpenOrder, RecentFill};
use crate::modules::db::models::{RecordingSession, SessionSnapshot, SessionTrade};
use crate::modules::hft::types::BinanceDepth;
use crate::modules::hft::ring_buffer::PriceRingBuffer;
use crate::modules::hft::metrics::TrackingState;
use crate::modules::hft::binance_depth::BinanceTickEvent;
use crate::modules::hft::types::CsvRecord;
use crate::modules::hft::types::PolyDepthFrame;
use crate::modules::hft::executor::StrategyManager;
use crate::modules::hft::session_manager::SessionManager;
use crate::modules::hft::adaptive_risk_engine::AdaptiveRiskEngine;
use crate::modules::hft::adaptive_risk_engine::MacroContext;
use crate::modules::hft::t5_strategy::T5Manager;
use crate::modules::hft::t3_strategy::T3Manager;
use crate::modules::hft::pnr_strategy::PnrManager;
use crate::modules::hft::insight_strategies::InsightManager;
use crate::modules::hft::fenix_trading::FenixTradingManager;
use crate::modules::hft::odiseo_strategies::OdiseoTradingManager;

pub struct AppState {
    // Estado en memoria (actualizado por el consumer de AppMsg)
    pub status:          RwLock<String>,
    pub market:          RwLock<Option<MarketInfo>>,
    pub book_up:         RwLock<Option<BookSnapshot>>,
    pub book_down:       RwLock<Option<BookSnapshot>>,
    pub balance:         RwLock<Option<f64>>,
    pub btc_price:       RwLock<Option<f64>>,
    pub btc_volume:      RwLock<f64>,
    pub btc_open:        RwLock<Option<f64>>,
    pub trade_window_up:   RwLock<VecDeque<(f64,f64)>>, // (price, size) — last N UP trades with vol>=min_vol
    pub trade_window_dn:   RwLock<VecDeque<(f64,f64)>>, // last N DOWN trades
    pub trade_min_vol:   RwLock<f64>,   // minimum trade size to qualify (default 50)
    pub trade_window_n:  RwLock<usize>, // max trades to keep in window (default 10)
    pub open_orders:     RwLock<Vec<OpenOrder>>,
    pub recent_fills:    RwLock<Vec<RecentFill>>,
    pub candles:         RwLock<Vec<Candle>>,

    pub interval_arc:    Arc<Mutex<CandleInterval>>,

    pub btc_provider:    RwLock<BtcPriceProvider>,
    pub btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,

    pub cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
    pub broadcast_tx:    broadcast::Sender<String>,

    pub creds:           ClobCredentials,

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

    /// T-5 Certainty Strategy — Wisdom v2 (demo paper-trading)
    pub t5_manager: Arc<T5Manager>,

    /// T-3 Aggressive Strategy — Wisdom v3 (demo paper-trading)
    pub t3_manager: Arc<T3Manager>,

    /// PNR Analysis — Wisdom v4 (Hydra No Return)
    pub pnr_manager: Arc<PnrManager>,

    /// Insight Strategies — Cerbero & Fenix families
    pub insight_manager: Arc<InsightManager>,

    /// Fenix Trading — paper-trading simulation ($20 per strategy)
    pub fenix_trading: Arc<FenixTradingManager>,
    /// Odiseo Trading — bid-floor momentum paper-trading (bidirectional, 3-layer SL)
    pub odiseo_trading: Arc<OdiseoTradingManager>,

    /// Historial de snapshots completos del orderbook de Polymarket (buffer circular, últimos 300)
    pub poly_depth_history: RwLock<VecDeque<PolyDepthFrame>>,

    /// Modo diagnóstico: solo captura datos crudos + health, sin estrategias.
    pub diagnostic_mode: std::sync::atomic::AtomicBool,

    /// Latency tracking (ms) for health endpoint
    pub latency_binance: RwLock<u64>,
    pub latency_poly:    RwLock<u64>,
    pub prev_btc_vel:    RwLock<f64>,  // previous tick's btc_vel for acceleration calc

    #[cfg(feature = "premium-patterns")]
    pub patterns_config: RwLock<crate::modules::premium::patterns::models::DetectorConfig>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cmd_tx:          mpsc::UnboundedSender<CmdMsg>,
        broadcast_tx:    broadcast::Sender<String>,
        interval_arc:    Arc<Mutex<CandleInterval>>,
        db:              Option<PgPool>,
        btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,
        binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
        binance_ring:    Arc<PriceRingBuffer>,
        tracking_state:  Arc<TrackingState>,
        tick_tx:         mpsc::UnboundedSender<BinanceTickEvent>,
        creds:           ClobCredentials,
    ) -> Arc<Self> {
        let cmd_tx_clone = cmd_tx.clone();
        Arc::new(Self {
            status:            RwLock::new("Initializing".into()),
            market:            RwLock::new(None),
            book_up:           RwLock::new(None),
            book_down:         RwLock::new(None),
            balance:           RwLock::new(None),
            btc_price:         RwLock::new(None),
            btc_volume:        RwLock::new(0.0),
            btc_open:          RwLock::new(None),
            trade_window_up:   RwLock::new(VecDeque::with_capacity(10)),
            trade_window_dn:   RwLock::new(VecDeque::with_capacity(10)),
            trade_min_vol:     RwLock::new(50.0),
            trade_window_n:    RwLock::new(10),
            open_orders:       RwLock::new(vec![]),
            recent_fills:      RwLock::new(vec![]),
            candles:           RwLock::new(vec![]),
            interval_arc,
            btc_provider:      RwLock::new(BtcPriceProvider::Coinbase),
            btc_provider_tx,
            cmd_tx,
            broadcast_tx,
            creds,
            db,
            recording_sessions: RwLock::new(vec![]),
            mem_sessions:      RwLock::new(vec![]),
            mem_snapshots:     RwLock::new(vec![]),
            mem_trades:        RwLock::new(vec![]),
            mem_hft:           RwLock::new(vec![]),
            binance_depth,
            binance_ring,
            tracking_state,
            tick_tx,
            session_manager:   Arc::new(SessionManager::new("sessions")),
            strategy_manager:  Arc::new(Mutex::new(StrategyManager::new())),
            tick_drain:        Arc::new(AtomicBool::new(false)),
            adaptive_engine:   Arc::new(tokio::sync::Mutex::new(AdaptiveRiskEngine::new())),
            macro_ctx:         Arc::new(RwLock::new(MacroContext::default())),
            t5_manager:        Arc::new(T5Manager::new()),
            t3_manager:        Arc::new(T3Manager::new()),
            pnr_manager:       Arc::new(PnrManager::new()),
            insight_manager:   Arc::new(InsightManager::new()),
            fenix_trading:     Arc::new(FenixTradingManager::new()),
            odiseo_trading:    Arc::new(OdiseoTradingManager::new(Some(cmd_tx_clone))),
            poly_depth_history: RwLock::new(VecDeque::with_capacity(300)),
            diagnostic_mode: AtomicBool::new(false),
            latency_binance:   RwLock::new(0),
            latency_poly:      RwLock::new(0),
            prev_btc_vel:      RwLock::new(0.0),
            #[cfg(feature = "premium-patterns")]
            patterns_config: RwLock::new(crate::modules::premium::patterns::models::DetectorConfig::default()),
        })
    }
}
