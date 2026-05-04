//! Imbalance & Liquidity Strategies
//!
//! Layer 3: Trading Strategy.
//! Strategy A: Imbalance Divergence — trade when Binance imbalance diverges from Poly.
//! Strategy B: Liquidity Grabbing — detect and fade large liquidity walls.
//! Re-exports from modules/hft/executor.rs and metrics.rs (migración progresiva).

pub use crate::modules::hft::executor::StrategyManager as ImbaLiqbManager;
