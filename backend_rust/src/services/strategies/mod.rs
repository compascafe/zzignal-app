/// Trading strategy implementations.
/// - `framework`  — Shared strategy trait and evaluation pipeline
/// - `t5`         — T-5 Certainty (Wisdom v2)
/// - `t3`         — T-3 Aggressive (Wisdom v3)
/// - `pnr`        — Hydra No Return PnR analysis
/// - `odiseo`     — Odiseo 83 bidirectional momentum
/// - `filters`    — Liquidity filters for Odiseo variants

pub mod filters;
pub mod framework;
pub mod odiseo;
pub mod pnr;
pub mod t3;
pub mod t5;
