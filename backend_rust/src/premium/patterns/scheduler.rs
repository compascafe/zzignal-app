use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};
use serde_json::json;

use crate::models::state::AppState;

use super::detector::SideDetector;
use super::repository;

/// Background task: cada 200ms lee el order book y ejecuta el detector de patrones.
/// Las señales se persisten en DB y se emiten vía broadcast al frontend.
pub async fn run_detector(state: Arc<AppState>) {
    let pool = match state.db.as_ref().map(|p| p.clone()) {
        Some(p) => p,
        None => {
            warn!("Patterns: PostgreSQL no disponible. El módulo NO se iniciará.");
            return;
        }
    };

    // Run migration
    if let Err(e) = sqlx::query(include_str!("migrations/001_pattern_signals.sql"))
        .execute(&pool).await
    {
        warn!("Patterns: migración fallida: {e}");
        return;
    }
    info!("Patterns: detector iniciado. Escuchando patrones en tiempo real...");

    let config = state.patterns_config.read().await.clone();
    let mut detectors: HashMap<String, SideDetector> = HashMap::new();
    detectors.insert("up".into(), SideDetector::new("up", config.clone()));
    detectors.insert("down".into(), SideDetector::new("down", config.clone()));

    let mut tick = tokio::time::interval(Duration::from_millis(200));

    loop {
        tick.tick().await;

        // Reload config (puede haber cambiado vía API)
        let config = state.patterns_config.read().await.clone();

        for side in ["up", "down"] {
            let (best_bid, best_bid_sz, best_ask, best_ask_sz, bids, asks) = {
                let book_guard = if side == "up" {
                    state.book_up.read().await
                } else {
                    state.book_down.read().await
                };
                match &*book_guard {
                    Some(b) => {
                        let best_bid = b.bids.first().map(|l| l.price).unwrap_or(0.0);
                        let best_bid_sz = b.bids.first().map(|l| l.size).unwrap_or(0.0);
                        let best_ask = b.asks.first().map(|l| l.price).unwrap_or(0.0);
                        let best_ask_sz = b.asks.first().map(|l| l.size).unwrap_or(0.0);
                        let bids = b.bids.clone();
                        let asks = b.asks.clone();
                        (best_bid, best_bid_sz, best_ask, best_ask_sz, bids, asks)
                    }
                    None => continue,
                }
            };

            let detector = detectors.entry(side.to_string())
                .or_insert_with(|| SideDetector::new(side, config.clone()));
            detector.config = config.clone();

            let signals = detector.feed(
                best_bid, best_bid_sz, best_ask, best_ask_sz,
                &bids, &asks,
            );

            for signal in signals {
                // Persistir en DB
                let pool = pool.clone();
                let signal_clone = signal.clone();
                tokio::spawn(async move {
                    if let Err(e) = repository::insert_signal(&pool, &signal_clone).await {
                        warn!("Patterns: error guardando señal: {e}");
                    }
                });

                // Broadcast al frontend
                let alert = detector.to_alert(&signal, best_bid, best_ask);
                let json = serde_json::to_string(&json!({
                    "type": "pattern_alert",
                    "pattern": alert.pattern,
                    "side": alert.side,
                    "severity": alert.severity,
                    "message": alert.message,
                    "price": alert.price,
                    "ts": alert.ts.to_rfc3339(),
                })).unwrap_or_default();
                let _ = state.broadcast_tx.send(json);
            }
        }
    }
}
