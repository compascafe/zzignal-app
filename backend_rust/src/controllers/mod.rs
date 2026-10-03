/// Controller layer: HTTP API, WebSocket handler, and background worker.
///
/// Bridges external I/O (network) with the domain services:
/// - `api`    — Axum router with REST endpoints + WebSocket upgrade handler
/// - `worker` — Async event loop: CLOB WS, BTC price stream, order execution
pub mod api;
pub mod worker;
