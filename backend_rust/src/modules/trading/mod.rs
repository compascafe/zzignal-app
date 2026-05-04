//! Layer 3: Trading — Estrategias y Ejecución
//!
//! Recibe métricas de Layer 2 (analysis) + datos de Layer 1.
//! Toma decisiones de entrada/salida para cada estrategia (simulada o real).
//! No persiste datos (eso es Layer 4).

pub mod engine;
pub mod fenix;
pub mod hercules;
pub mod hydra;
pub mod pnr;
pub mod cerbero;
pub mod imba_liqb;
