//! CSV Writer — Per-Session File Recording
//!
//! Layer 4: Persistence. Writes CSV rows to per-session files.
//! Uses BufWriter for performance. Flush every 100 rows + background 15s.
//! Re-exports from modules/hft/session_manager.rs (migración progresiva).

pub use crate::modules::hft::session_manager::SessionManager as CsvWriter;
