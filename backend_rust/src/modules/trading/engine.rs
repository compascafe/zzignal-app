//! Trading Engine — Orchestrates all strategies
//!
//! Layer 3: Central coordinator. Routes data from Layers 1+2 to all strategies
//! and collects trading decisions. Does NOT persist (Layer 4 does that).

use serde::Serialize;

use crate::modules::analysis::metrics::AnalysisSnapshot;

// ─── Engine ────────────────────────────────────────────────────────────────

/// Trading engine: coordina todas las estrategias y ejecuta órdenes (sim o real).
pub struct TradingEngine {
    pub config: EngineConfig,
    pub stats:   EngineStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineConfig {
    pub mode: TradingMode,
    pub capital_per_strategy: f64,
    pub max_concurrent_trades: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum TradingMode {
    PaperOnly,   // solo simulación (CSV)
    LiveOnly,    // solo ejecución real
    Hybrid,      // ambas
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            mode: TradingMode::PaperOnly,
            capital_per_strategy: 20.0,
            max_concurrent_trades: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStats {
    pub total_ticks_processed: u64,
    pub total_signals:         u64,
    pub total_trades_opened:   u64,
    pub total_trades_closed:   u64,
    pub cumulative_pnl:        f64,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            total_ticks_processed: 0,
            total_signals: 0,
            total_trades_opened: 0,
            total_trades_closed: 0,
            cumulative_pnl: 0.0,
        }
    }
}

impl TradingEngine {
    pub fn new() -> Self {
        Self {
            config: EngineConfig::default(),
            stats:  EngineStats::default(),
        }
    }

    pub fn with_config(config: EngineConfig) -> Self {
        Self { config, stats: EngineStats::default() }
    }

    /// Called on every tick. Routes to all active strategies.
    /// Returns a TradeReport with decisions from each strategy.
    pub fn on_tick(&mut self, _snapshot: &AnalysisSnapshot) -> TradeReport {
        self.stats.total_ticks_processed += 1;
        TradeReport::default()
    }

    /// Called at session close to settle open trades.
    pub fn on_session_close(&mut self, _outcome: &str) {
        // Settle all open trades
    }
}

// ─── Trade Report ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TradeReport {
    pub tick_timestamp_ms: i64,
    pub master_signal:     u8,
    pub strategies: Vec<StrategyDecision>,
}

impl Default for TradeReport {
    fn default() -> Self {
        Self { tick_timestamp_ms: 0, master_signal: 0, strategies: vec![] }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyDecision {
    pub strategy_code: String,
    pub active:        u8,   // 1 = trade open
    pub entry_price:   f64,
    pub unrealized_pnl: f64,
    pub skip_reason:   u8,   // 0=none, 1=trend, 2=spread, 3=volume
    pub target_price:  f64,
    pub exited:        u8,   // 1 = just exited
}

#[derive(Debug, Clone)]
pub struct StrategyTrade {
    pub code:        String,
    pub direction_up: bool,
    pub entry_price: f64,
    pub target_price: f64,
    pub opened_at_ms: i64,
    pub closed_at_ms: Option<i64>,
    pub pnl:         f64,
    pub settled:     bool,
}
