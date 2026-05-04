//! Hydra — T-5 (85%) + T-3 (90%) Certainty Strategies
//!
//! Layer 3: Trading Strategy. Paper trading at prediction time.
//! Re-exports from modules/hft/t5_strategy.rs and t3_strategy.rs (migración progresiva).

// Hydra 85: T-5 — enters at 85% certainty, 5 min before session close
pub use crate::modules::hft::t5_strategy::T5Manager as Hydra85Manager;

// Hydra 90: T-3 — enters at 90% certainty, 3 min before session close
pub use crate::modules::hft::t3_strategy::T3Manager as Hydra90Manager;
