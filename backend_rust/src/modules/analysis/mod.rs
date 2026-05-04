//! Layer 2: Analysis — Métricas e Indicadores
//!
//! Recibe datos crudos de Layer 1 (poly_orderbook + binance_feed).
//! Produce métricas computadas (mid, spread, velocity, BB, RSI, MACD, master_signal).
//! Funciones puras siempre que sea posible. No modifica datos crudos.
//! No depende de Layer 3 o 4.

pub mod metrics;
pub mod indicators;
pub mod signals;
