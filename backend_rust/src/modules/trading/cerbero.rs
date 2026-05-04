//! Cerbero — Range Observation Strategies
//!
//! Layer 3: Trading Strategy. Observes price in specific ranges.
//! Cerbero 70-80, 80-90, 90-98.
//! Re-exports from modules/hft/insight_strategies.rs (migración progresiva).

pub use crate::modules::hft::insight_strategies::InsightManager as CerberoManager;
