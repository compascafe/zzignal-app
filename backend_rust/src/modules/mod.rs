// ─── v4 Architecture: 4 Layers ─────────────────────────────────────────────
// Layer 1: Data   — Raw market data in memory (orderbook + binance)
// Layer 2: Analysis — Metrics, indicators, signals
// Layer 3: Trading   — Strategies & execution
// Layer 4: Recording — CSV, DB persistence, reinforcement learning
//
// Core: Orchestration (worker, API, state)
// DB:   Legacy PostgreSQL (migrating to recording/db_store)

pub mod data;
pub mod analysis;
pub mod trading;
pub mod recording;

pub mod core;
pub mod db;
pub mod hft; // legacy — migrating to layers above

#[cfg(any(feature = "premium-collector", feature = "premium-patterns", feature = "premium-executor"))]
pub mod premium;
