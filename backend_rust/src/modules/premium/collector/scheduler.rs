use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracing::{error, info, warn};

use crate::modules::core::state::AppState;

use super::models::{CandleBuilder, OrderBookCandle, TIMEFRAMES};
use super::repository;

/// Corre en background:
/// - Cada ~1s captura el order book actual y actualiza las velas en construcción
/// - Cuando un timeframe se cierra, persiste la vela en DB
pub async fn run_collector(state: Arc<AppState>) {
    // Sólo funciona con DB (PostgreSQL requerido para persistir los datos)
    let Some(pool) = state.db.as_ref().map(|p| p.clone()) else {
        warn!("Collector: PostgreSQL no disponible. El módulo NO se iniciará.");
        return;
    };

    if let Err(e) = super::migrations::run(&pool).await {
        error!("Collector: migración fallida: {e}");
        return;
    }
    info!("Collector: migración OK, iniciando captura multi-timeframe...");

    let mut tick_timer = tokio::time::interval(Duration::from_millis(500));
    let mut builders: HashMap<String, CandleBuilder> = HashMap::new();
    // builders key: "{interval}_{side}" → CandleBuilder

    loop {
        tick_timer.tick().await;

        // Leer el order book actual del AppState
        let (up_bid, up_ask) = read_book(&state, "up").await;
        let (down_bid, down_ask) = read_book(&state, "down").await;

        let now = Utc::now();

        for &(interval_str, interval_secs) in TIMEFRAMES {
            // Calcular open_time para este intervalo (floor a la ventana)
            let open_time = floor_to_interval(now, interval_secs as i64);

            for (side, best_bid, best_ask) in [
                ("up", up_bid, up_ask),
                ("down", down_bid, down_ask),
            ] {
                let key = format!("{}_{}", interval_str, side);

                if let Some(builder) = builders.get_mut(&key) {
                    // Si la ventana cambió, finalizar vela actual y empezar nueva
                    if builder.open_time != open_time {
                        let finished = builders.remove(&key).unwrap();
                        let candle = finished.finish();
                        tokio::spawn(persist_candle(pool.clone(), candle));

                        // Empezar nueva vela
                        builders.insert(
                            key.clone(),
                            CandleBuilder::new(
                                interval_str.to_string(),
                                side.to_string(),
                                open_time,
                                best_bid,
                                best_ask,
                            ),
                        );
                    } else {
                        // Actualizar vela existente
                        // En una versión optimizada, calcularíamos bid_vol/ask_vol sumando los niveles.
                        // Por ahora usamos best_bid_sz + best_ask_sz (volumen en el top level).
                        let bid_vol = best_bid; // placeholder: en prod se suma la profundidad real
                        let ask_vol = best_ask; // placeholder
                        builder.update(best_bid, best_ask, bid_vol, ask_vol);
                    }
                } else {
                    // Primera vela para este intervalo+side
                    builders.insert(
                        key,
                        CandleBuilder::new(
                            interval_str.to_string(),
                            side.to_string(),
                            open_time,
                            best_bid,
                            best_ask,
                        ),
                    );
                }
            }
        }
    }
}

/// Lee el best bid y best ask de un side del order book
async fn read_book(state: &AppState, side: &str) -> (f64, f64) {
    match side {
        "up" => {
            let book = state.book_up.read().await;
            match &*book {
                Some(b) => (
                    b.bids.first().map(|l| l.price).unwrap_or(0.0),
                    b.asks.first().map(|l| l.price).unwrap_or(0.0),
                ),
                None => (0.0, 0.0),
            }
        }
        "down" => {
            let book = state.book_down.read().await;
            match &*book {
                Some(b) => (
                    b.bids.first().map(|l| l.price).unwrap_or(0.0),
                    b.asks.first().map(|l| l.price).unwrap_or(0.0),
                ),
                None => (0.0, 0.0),
            }
        }
        _ => (0.0, 0.0),
    }
}

/// Redondea un timestamp al inicio del intervalo
fn floor_to_interval(ts: DateTime<Utc>, interval_secs: i64) -> DateTime<Utc> {
    let epoch = ts.timestamp();
    let floored = (epoch / interval_secs) * interval_secs;
    DateTime::<Utc>::from_timestamp(floored, 0).unwrap_or(ts)
}

/// Persiste una vela en background
async fn persist_candle(pool: sqlx::PgPool, candle: OrderBookCandle) {
    if let Err(e) = repository::insert_candle(&pool, &candle).await {
        warn!("Collector: error persistiendo vela {}:{} @ {}: {e}",
            candle.interval, candle.side, candle.open_time);
    } else {
        info!("Collector: vela {}:{} @ {} ({} ticks)",
            candle.interval, candle.side,
            candle.open_time.format("%H:%M"),
            candle.tick_count);
    }
}
