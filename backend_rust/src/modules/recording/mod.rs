//! Layer 4: Recording — Persistencia y Reinforcement Learning
//!
//! Recibe el CsvRecord completo + eventos de trading de Layer 3.
//! Escribe a disco (CSV por sesión) y a PostgreSQL.
//! RL Feedback Loop: lee accuracy histórica y ajusta parámetros.

pub mod csv_writer;
pub mod db_store;
pub mod rl_feedback;
