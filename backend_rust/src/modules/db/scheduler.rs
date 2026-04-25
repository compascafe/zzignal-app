use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};
use serde_json::json;

use crate::modules::core::state::AppState;
use crate::modules::core::worker::{CmdMsg, OrderSide, Outcome};
use crate::modules::db::repository;

/// Corre en background:
///  1. Cada 10s guarda snapshot del order book en PostgreSQL
///  2. Cada 5s revisa scheduled_executions pendientes y las ejecuta
pub async fn run_scheduler(state: Arc<AppState>) {
    let mut book_timer   = tokio::time::interval(Duration::from_secs(10));
    let mut exec_timer   = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = book_timer.tick() => {
                save_book_snapshot(Arc::clone(&state)).await;
            }
            _ = exec_timer.tick() => {
                process_pending_executions(Arc::clone(&state)).await;
            }
        }
    }
}

async fn save_book_snapshot(state: Arc<AppState>) {
    let pool = state.db.as_ref();
    if pool.is_none() { return; }

    let book_up   = state.book_up.read().await.clone();
    let book_down = state.book_down.read().await.clone();

    for (side, book) in [("up", book_up), ("down", book_down)] {
        if let Some(b) = book {
            let best_bid    = b.bids.first().map(|l| l.price);
            let best_bid_sz = b.bids.first().map(|l| l.size);
            let best_ask    = b.asks.first().map(|l| l.price);
            let best_ask_sz = b.asks.first().map(|l| l.size);
            let spread = best_bid.and_then(|bb| best_ask.map(|ba| ba - bb));

            let depth_bids = json!(b.bids.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
            let depth_asks = json!(b.asks.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());

            if let Err(e) = repository::insert_snapshot(
                pool, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread,
                Some(depth_bids), Some(depth_asks),
            ).await {
                warn!("DB snapshot {}: {}", side, e);
            }
        }
    }
}

async fn process_pending_executions(state: Arc<AppState>) {
    let pool = state.db.as_ref();
    if pool.is_none() { return; }

    let pending = match repository::query_pending_executions(pool).await {
        Ok(list) => list,
        Err(e)   => { warn!("Scheduler query pending: {}", e); return; }
    };

    for ex in pending {
        let id = ex.id;
        info!("Ejecutando scheduled #{}: {} {} @ {:?}", id, ex.side, ex.order_type, ex.price);

        let side    = parse_side(&ex.side);
        let outcome = parse_outcome(&ex.outcome);

        let result = match (side, outcome) {
            (Some(s), Some(o)) => {
                let cmd = match ex.order_type.as_str() {
                    "limit" => {
                        match (ex.price, ex.size) {
                            (Some(p), Some(sz)) => Some(CmdMsg::PlaceLimitOrder { side: s, outcome: o, price: p, size: sz }),
                            _ => None,
                        }
                    }
                    "market" => {
                        match ex.amount_usdc {
                            Some(amt) => Some(CmdMsg::PlaceMarketOrder { side: s, outcome: o, amount_usdc: amt }),
                            _ => None,
                        }
                    }
                    "scalp" => {
                        match (ex.price, ex.size, ex.target_price) {
                            (Some(p), Some(sz), Some(tp)) => Some(CmdMsg::ScalpBuy { outcome: o, price: p, size: sz, target_price: tp }),
                            _ => None,
                        }
                    }
                    _ => None,
                };

                match cmd {
                    Some(c) => {
                        let _ = state.cmd_tx.send(c);
                        Ok(())
                    }
                    None => Err("Parámetros incompletos".to_string()),
                }
            }
            _ => Err("side/outcome inválido".to_string()),
        };

        match result {
            Ok(_) => {
                if let Err(e) = repository::mark_executed(pool, id, None).await {
                    error!("Mark executed #{}: {}", id, e);
                }
            }
            Err(msg) => {
                warn!("Scheduled #{} falló: {}", id, msg);
                if let Err(e) = repository::mark_executed(pool, id, Some(&msg)).await {
                    error!("Mark failed #{}: {}", id, e);
                }
            }
        }
    }
}

fn parse_side(s: &str) -> Option<OrderSide> {
    match s.to_lowercase().as_str() {
        "buy"  => Some(OrderSide::Buy),
        "sell" => Some(OrderSide::Sell),
        _      => None,
    }
}

fn parse_outcome(s: &str) -> Option<Outcome> {
    match s.to_lowercase().as_str() {
        "up"   => Some(Outcome::Up),
        "down" => Some(Outcome::Down),
        _      => None,
    }
}
