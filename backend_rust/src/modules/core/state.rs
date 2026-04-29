use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock, mpsc as tokio_mpsc};
use sqlx::PgPool;

use crate::modules::core::worker::{BtcPriceProvider, BookSnapshot, Candle, CandleInterval, CmdMsg, MarketInfo, OpenOrder, RecentFill};
use crate::modules::db::models::{RecordingSession, SessionSnapshot, SessionTrade};
use crate::modules::hft::types::BinanceDepth;
use crate::modules::hft::ring_buffer::PriceRingBuffer;
use crate::modules::hft::metrics::VpinState;
use crate::modules::hft::logger::CsvLogger;

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

    // Control del intervalo de velas (compartido con worker)
    pub interval_arc:    Arc<Mutex<CandleInterval>>,

    // Proveedor de precio BTC (compartido con worker via watch channel)
    pub btc_provider:    RwLock<BtcPriceProvider>,
    pub btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,

    // Comandos → worker
    pub cmd_tx:          tokio_mpsc::UnboundedSender<CmdMsg>,

    // Broadcast → clientes WebSocket
    pub broadcast_tx:    broadcast::Sender<String>,

    // Shutdown signal → tareas de fondo
    pub shutdown_tx:     broadcast::Sender<()>,

    // Base de datos (None si DATABASE_URL no está configurada)
    pub db:              Option<PgPool>,

    // Sesión de grabación activa (Session Recorder)
    pub recording_session: RwLock<Option<i32>>,

    // Buffers en memoria para Session Recorder (fallback si no hay DB)
    pub mem_sessions:    RwLock<Vec<RecordingSession>>,
    pub mem_snapshots:   RwLock<Vec<SessionSnapshot>>,
    pub mem_trades:      RwLock<Vec<SessionTrade>>,

    // ─── HFT Module ─────────────────────────────────────────────────────────
    /// Último snapshot completo del order book de Binance (top 20 niveles)
    pub binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,

    /// Ring buffer lock-free con histórico compacto de estados de Binance
    pub binance_ring:    Arc<PriceRingBuffer>,

    /// Estado acumulado de VPIN (ventana deslizante)
    pub vpin_state:      Arc<VpinState>,

    /// Logger CSV para métricas HFT (flush cada 60s)
    pub csv_logger:      Arc<CsvLogger>,

    // Pattern Detector config (solo disponible con premium-patterns)
    #[cfg(feature = "premium-patterns")]
    pub patterns_config: RwLock<crate::modules::premium::patterns::models::DetectorConfig>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cmd_tx:          tokio_mpsc::UnboundedSender<CmdMsg>,
        broadcast_tx:    broadcast::Sender<String>,
        shutdown_tx:     broadcast::Sender<()>,
        interval_arc:    Arc<Mutex<CandleInterval>>,
        db:              Option<PgPool>,
        btc_provider_tx: Arc<tokio::sync::watch::Sender<BtcPriceProvider>>,
        binance_depth:   Arc<RwLock<Option<BinanceDepth>>>,
        binance_ring:    Arc<PriceRingBuffer>,
        vpin_state:      Arc<VpinState>,
        csv_logger:      Arc<CsvLogger>,
    ) -> Arc<Self> {
        Arc::new(Self {
            status:          RwLock::new("Initializing".into()),
            market:          RwLock::new(None),
            book_up:         RwLock::new(None),
            book_down:       RwLock::new(None),
            balance:         RwLock::new(None),
            btc_price:       RwLock::new(None),
            btc_open:        RwLock::new(None),
            last_trade_up:   RwLock::new(None),
            last_trade_down: RwLock::new(None),
            open_orders:     RwLock::new(vec![]),
            recent_fills:    RwLock::new(vec![]),
            candles:         RwLock::new(vec![]),
            interval_arc,
            btc_provider:    RwLock::new(BtcPriceProvider::Coinbase),
            btc_provider_tx,
            cmd_tx,
            broadcast_tx,
            shutdown_tx,
            db,
            recording_session: RwLock::new(None),
            mem_sessions:    RwLock::new(vec![]),
            mem_snapshots:   RwLock::new(vec![]),
            mem_trades:      RwLock::new(vec![]),
            binance_depth,
            binance_ring,
            vpin_state,
            csv_logger,
            #[cfg(feature = "premium-patterns")]
            patterns_config: RwLock::new(crate::modules::premium::patterns::models::DetectorConfig::default()),
        })
    }
}
