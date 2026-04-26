//! Multi-Timeframe Order Book Collector
//!
//! Captura el order book (UP y DOWN) en tiempo real y lo agrega en
//! múltiples timeframes: 1m, 5m, 15m, 1h, 4h, 1d.
//!
//! Para cada timeframe y cada side (up/down), se genera una "vela" de order book:
//!   - best_bid (open, high, low, close)
//!   - best_ask (open, high, low, close)
//!   - spread (open, high, low, close)
//!   - mid_price (open, high, low, close)
//!   - bid_volume, ask_volume
//!   - tick_count

pub mod models;
pub mod repository;
pub mod scheduler;
pub mod api;
pub mod migrations;
