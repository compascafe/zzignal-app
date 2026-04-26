//! Auto-Execution Engine — Bot de trading automático.
//!
//! Rule engine: IF condition THEN action.
//! Evalúa condiciones sobre el order book y ejecuta órdenes automáticamente.
//! Soporta: limit, market, scalp orders con cooldown, max positions y stop loss.

pub mod models;
pub mod engine;
pub mod repository;
pub mod api;
pub mod scheduler;

