//! zzignal-core — BTC 15-min Polymarket Trading Engine
//!
//! Architecture: MVC (Model-View-Controller)
//!
//! - `models/`       — Data types, state, credentials
//! - `views/`        — Terminal UI (ratatui, optional via "tui" feature)
//! - `controllers/`  — REST API, WebSocket handlers, worker loop
//! - `services/`     — Business logic: Binance, Polymarket, metrics, strategies
//! - `db/`           — PostgreSQL persistence layer
//! - `utils/`        — Ring buffer, persistence helpers

pub mod models;
#[cfg(feature = "tui")]
pub mod views;
pub mod controllers;
pub mod services;
pub mod db;
pub mod utils;

#[cfg(any(
    feature = "premium-collector",
    feature = "premium-patterns",
    feature = "premium-executor"
))]
pub mod premium;
