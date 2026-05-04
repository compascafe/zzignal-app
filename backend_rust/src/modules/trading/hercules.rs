//! Hercules — Multi-Layer RL + Adaptive Risk Engine + Conformal Prediction
//!
//! Layer 3: Trading Strategy.
//! Re-exports from modules/hft/adaptive_risk_engine.rs (migración progresiva).

pub use crate::modules::hft::adaptive_risk_engine::{
    AdaptiveRiskEngine, MacroContext, MacroSnapshot,
    warmup_fetch_and_compute,
};
