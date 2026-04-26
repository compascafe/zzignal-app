//! Pattern Detector — Detección de patrones estadísticos en tiempo real.
//!
//! Escucha actualizaciones del order book y detecta:
//! - Walls: órdenes grandes que actúan como soporte/resistencia
//! - Spread anomalies: ensanchamiento repentino del spread
//! - Depth imbalance: desbalance entre bid volume y ask volume
//! - Spoofing: órdenes grandes que aparecen y desaparecen
//! - Momentum shifts: cambios de dirección del mid price
//!
//! Las señales se persisten en PostgreSQL y se envían al frontend vía WebSocket.

pub mod models;
pub mod detector;
pub mod repository;
pub mod api;
pub mod scheduler;

