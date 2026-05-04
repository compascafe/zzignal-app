//! DB Store — PostgreSQL Persistence
//!
//! Layer 4: Persistence. Writes candles, fills, snapshots, btc_ticks
//! and session data to PostgreSQL.
//! Re-exports from modules/core/persistence.rs (migración progresiva).

pub use crate::modules::core::persistence;
