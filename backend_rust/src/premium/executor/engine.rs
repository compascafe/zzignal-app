use std::sync::Arc;
use chrono::Utc;
use tracing::{info, warn};

use crate::models::state::AppState;
use crate::controllers::worker::{CmdMsg, OrderSide, Outcome};

use super::models::{Condition, ExecutionLog, Strategy};
use super::repository;

/// Evalúa si todas las condiciones de una estrategia se cumplen
pub fn evaluate_conditions(state: &AppState, conditions: &[Condition]) -> bool {
    if conditions.is_empty() {
        return false;
    }

    for cond in conditions {
        if !evaluate_single(state, cond) {
            return false; // AND logic — todas deben cumplirse
        }
    }
    true
}

fn evaluate_single(state: &AppState, cond: &Condition) -> bool {
    let book = if cond.side == "up" {
        state.book_up.try_read()
    } else {
        state.book_down.try_read()
    };
    let Ok(book_guard) = book else { return false };
    let book = match &*book_guard {
        Some(b) => b,
        None => return false,
    };

    let best_bid = book.bids.first().map(|l| l.price).unwrap_or(0.0);
    let best_ask = book.asks.first().map(|l| l.price).unwrap_or(0.0);
    let spread = best_ask - best_bid;
    let mid_price = (best_bid + best_ask) / 2.0;
    let bid_vol: f64 = book.bids.iter().map(|l| l.size).sum();
    let ask_vol: f64 = book.asks.iter().map(|l| l.size).sum();

    let current = match cond.metric.as_str() {
        "spread"       => spread,
        "mid_price"    => mid_price,
        "bid_volume"   => bid_vol,
        "ask_volume"   => ask_vol,
        "btc_price"    => state.btc_price.try_read().ok().and_then(|p| *p).unwrap_or(0.0),
        "imbalance"    => if ask_vol > 0.0 { bid_vol / ask_vol } else { 0.0 },
        "best_bid"     => best_bid,
        "best_ask"     => best_ask,
        _ => return false,
    };

    match cond.operator.as_str() {
        "gt"  => current >  cond.value,
        "lt"  => current <  cond.value,
        "gte" => current >= cond.value,
        "lte" => current <= cond.value,
        "eq"  => (current - cond.value).abs() < 0.0001,
        _ => false,
    }
}

/// Ejecuta la acción de una estrategia enviando el comando al worker
pub async fn execute_action(state: &Arc<AppState>, strategy: &Strategy) -> ExecutionLog {
    let btc_price = *state.btc_price.read().await;
    let action = strategy.action_parsed();

    let outcome = match action.outcome.as_str() {
        "up"   => Outcome::Up,
        "down" => Outcome::Down,
        _      => Outcome::Up,
    };

    let result = match action.order_type.as_str() {
        "limit" => {
            match (action.price, action.size) {
                (Some(price), Some(size)) => {
                    let _ = state.cmd_tx.send(CmdMsg::PlaceLimitOrder {
                        side: OrderSide::Buy,
                        outcome,
                        price,
                        size,
                    });
                    Ok(())
                }
                _ => Err("limit requires price and size".to_string()),
            }
        }
        "market" => {
            match action.amount_usdc {
                Some(amount) => {
                    let _ = state.cmd_tx.send(CmdMsg::PlaceMarketOrder {
                        side: OrderSide::Buy,
                        outcome,
                        amount_usdc: amount,
                    });
                    Ok(())
                }
                _ => Err("market requires amount_usdc".to_string()),
            }
        }
        "scalp" => {
            match (action.price, action.size, action.target_price) {
                (Some(price), Some(size), Some(target)) => {
                    let _ = state.cmd_tx.send(CmdMsg::ScalpBuy {
                        outcome,
                        price,
                        size,
                        target_price: target,
                    });
                    Ok(())
                }
                _ => Err("scalp requires price, size, target_price".to_string()),
            }
        }
        other => Err(format!("unknown order_type: {other}")),
    };

    let (status, reason) = match result {
        Ok(_) => ("success".to_string(), None),
        Err(msg) => ("failed".to_string(), Some(msg)),
    };

    let log = ExecutionLog {
        id: 0,
        strategy_id: strategy.id,
        strategy_name: strategy.name.clone(),
        outcome: action.outcome.clone(),
        side: "BUY".into(),
        order_type: action.order_type.clone(),
        price: action.price,
        size: action.size,
        result: status.clone(),
        reason,
        btc_price,
        ts: Utc::now(),
    };

    // Persistir execution log
    if let Some(pool) = state.db.as_ref() {
        if let Err(e) = repository::insert_execution_log(pool, &log).await {
            warn!("Executor: error guardando execution log: {e}");
        }
    }

    info!(
        "Executor: strategy '{}' → {} {} @ {:?} ({})",
        strategy.name,
        action.order_type,
        action.outcome,
        action.price,
        status,
    );

    log
}
