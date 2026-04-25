use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::modules::core::state::AppState;
use crate::modules::db::models::ScheduledExecution;
use crate::modules::db::repository;

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/db/snapshots",         get(list_snapshots))
        .route("/api/db/snapshots/latest",  get(latest_snapshot))
        .route("/api/db/snapshots/test",    post(test_snapshot))
        .route("/api/db/executions",        get(list_executions))
        .route("/api/db/executions",        post(create_execution))
        .route("/api/db/executions/{id}",   delete(delete_execution))
        .route("/api/db/executions/test",   post(test_execution))
        .with_state(state)
}

// ─── Snapshots ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SnapshotQuery {
    side:  Option<String>,
    limit: Option<i64>,
    from:  Option<DateTime<Utc>>,
    to:    Option<DateTime<Utc>>,
}

async fn list_snapshots(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SnapshotQuery>,
) -> Json<Value> {
    let limit = q.limit.unwrap_or(100).min(1000);
    match repository::query_snapshots(s.db.as_ref(), q.side.as_deref(), limit, q.from, q.to).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn latest_snapshot(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SnapshotQuery>,
) -> Json<Value> {
    let side = q.side.as_deref().unwrap_or("up");
    match repository::query_latest_snapshot(s.db.as_ref(), side).await {
        Ok(Some(row)) => Json(json!(row)),
        Ok(None)      => Json(json!(null)),
        Err(e)        => Json(json!({"error": e.to_string()})),
    }
}

// ─── Scheduled Executions ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExecutionQuery {
    status: Option<String>,
    limit:  Option<i64>,
}

async fn list_executions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ExecutionQuery>,
) -> Json<Value> {
    let limit = q.limit.unwrap_or(100).min(1000);
    match repository::query_executions(s.db.as_ref(), q.status.as_deref(), limit).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn create_execution(
    State(s): State<Arc<AppState>>,
    Json(body): Json<ScheduledExecution>,
) -> Json<Value> {
    match repository::insert_execution(s.db.as_ref(), &body).await {
        Ok(id) => Json(json!({"ok": true, "id": id })),
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

async fn delete_execution(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<Value> {
    // Primero intentar cancelar; si ya no está pending, eliminar
    if let Err(e) = repository::cancel_execution(s.db.as_ref(), id).await {
        return Json(json!({"ok": false, "error": e.to_string() }));
    }
    // Si cancel_execution afectó 0 filas (ya no era pending), hacer DELETE
    if let Err(e) = repository::delete_execution(s.db.as_ref(), id).await {
        return Json(json!({"ok": false, "error": e.to_string() }));
    }
    Json(json!({"ok": true }))
}

// ─── Test / Debug ─────────────────────────────────────────────────────────────

/// Fuerza un snapshot inmediato del order book actual en memoria → DB
async fn test_snapshot(State(s): State<Arc<AppState>>) -> Json<Value> {
    let pool = s.db.as_ref();
    if pool.is_none() {
        return Json(json!({"ok": false, "error": "DB no configurada" }));
    }

    let mut results = vec![];
    for (side, book_lock) in [("up", &s.book_up), ("down", &s.book_down)] {
        let book = book_lock.read().await.clone();
        match book {
            Some(b) => {
                let best_bid    = b.bids.first().map(|l| l.price);
                let best_bid_sz = b.bids.first().map(|l| l.size);
                let best_ask    = b.asks.first().map(|l| l.price);
                let best_ask_sz = b.asks.first().map(|l| l.size);
                let spread = best_bid.and_then(|bb| best_ask.map(|ba| ba - bb));
                let depth_bids = json!(b.bids.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
                let depth_asks = json!(b.asks.iter().take(5).map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());

                match repository::insert_snapshot(
                    pool, side, best_bid, best_bid_sz, best_ask, best_ask_sz, spread,
                    Some(depth_bids), Some(depth_asks),
                ).await {
                    Ok(_) => results.push(json!({"side": side, "ok": true })),
                    Err(e) => results.push(json!({"side": side, "ok": false, "error": e.to_string() })),
                }
            }
            None => results.push(json!({"side": side, "ok": false, "error": "book vacío" })),
        }
    }
    Json(json!({"ok": true, "results": results }))
}

/// Crea una ejecución de prueba programada para dentro de 1 minuto
async fn test_execution(State(s): State<Arc<AppState>>) -> Json<Value> {
    let test_exec = ScheduledExecution {
        scheduled_at: Utc::now() + chrono::Duration::minutes(1),
        side:         "buy".into(),
        outcome:      "up".into(),
        order_type:   "limit".into(),
        price:        Some(0.5000),
        size:         Some(1.0),
        amount_usdc:  None,
        target_price: None,
        notes:        Some("TEST — generada manualmente".into()),
    };
    match repository::insert_execution(s.db.as_ref(), &test_exec).await {
        Ok(id) => Json(json!({"ok": true, "id": id, "message": "Ejecución de prueba creada para dentro de 1 minuto" })),
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}
