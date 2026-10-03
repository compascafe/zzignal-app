/// Service layer: domain logic with minimal I/O coupling.
///
/// Each module encapsulates one concern:
/// - `binance`    — BTC depth (top-20) + ticker WebSocket stream
/// - `btc_stream` — Binance aggTrade stream → BTC price ticks
/// - `metrics`    — CsvRecord builders (mid, spread, velocity, imbalance, spoof)
/// - `pipeline`   — capture orchestration + latest HFT state
/// - `session`    — per-session CSV file manager
/// - `perf`       — hot-path latency counters exposed via `/api/perf`
pub mod binance;
pub mod btc_stream;
pub mod metrics;
pub mod perf;
pub mod pipeline;
pub mod session;
