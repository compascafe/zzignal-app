//! Módulos Premium — Propietarios. Se compilan solo con feature flags.
//!
//! Cada módulo se vende por separado:
//!   premium-collector → Recolección de order book multi-timeframe
//!   premium-patterns  → Detección de patrones estadísticos
//!   premium-executor  → Auto-ejecución de estrategias (bot)
//!
//! Compilación:
//!   cargo build --features premium-collector
//!   cargo build --features premium-all
//!
//! Generar licencias:
//!   cargo run --bin license-gen -- --module collector --customer acme-corp

#[cfg(feature = "premium-collector")]
pub mod collector;

#[cfg(feature = "premium-patterns")]
pub mod patterns;

#[cfg(feature = "premium-executor")]
pub mod executor;

pub mod license;
