/// Service layer: pure business logic with no I/O coupling.
///
/// Each module encapsulates one domain concern:
/// - `binance`       — BTC depth + aggTrade WebSocket streams
/// - `metrics`       — HFT metric computation (mid, spread, velocity, OFI)
/// - `engine`        — Multi-strategy paper-trading executor
/// - `session`       — Per-session CSV file manager
/// - `risk`          — Adaptive risk engine with Conformal Prediction
/// - `perf`          — Performance tracking and health endpoints
/// - `pipeline`      — CSV capture + DB insert pipeline (extracted from main)
/// - `strategies/`   — Strategy implementations

pub mod binance;
pub mod metrics;
pub mod perf;
pub mod pipeline;
pub mod session;
pub mod strategies;
