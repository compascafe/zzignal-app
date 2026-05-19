/// Service layer: pure business logic with no I/O coupling.
///
/// Each module encapsulates one domain concern:
/// - `binance`       — BTC depth + aggTrade WebSocket streams
/// - `polymarket`    — CLOB integration (merged from data/poly_orderbook)
/// - `metrics`       — HFT metric computation (mid, spread, velocity, OFI)
/// - `indicators`    — Technical indicators (BB, RSI, MACD)
/// - `signals`       — Trading signal generation from indicators
/// - `engine`        — Multi-strategy paper-trading executor
/// - `session`       — Per-session CSV file manager
/// - `risk`          — Adaptive risk engine with Conformal Prediction
/// - `perf`          — Performance tracking and health endpoints
/// - `logger`        — HFT event logger
/// - `strategies/`   — Strategy implementations

pub mod binance;
pub mod engine;
pub mod indicators;
pub mod logger;
pub mod metrics;
pub mod perf;
pub mod risk;
pub mod session;
pub mod signals;
pub mod strategies;
