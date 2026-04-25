use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    http::StatusCode,
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
    Path(id): Path<i32>,
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

// ─── Recording Sessions ───────────────────────────────────────────────────────

use crate::modules::db::models::NewSession;

pub fn session_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/sessions",              get(list_sessions))
        .route("/api/sessions/active",       get(get_active_session))
        .route("/api/sessions/start",        post(start_session))
        .route("/api/sessions/{id}/stop",    post(stop_session))
        .route("/api/sessions/{id}",         delete(delete_session))
        .route("/api/sessions/{id}/export",  get(export_session))
        .route("/api/sessions/{id}/snapshots", get(session_snapshots))
        .route("/api/sessions/{id}/trades",  get(session_trades))
        .with_state(state)
}

async fn list_sessions(State(s): State<Arc<AppState>>) -> Json<Value> {
    match repository::list_sessions(s.db.as_ref(), 100).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn get_active_session(State(s): State<Arc<AppState>>) -> Json<Value> {
    match repository::get_active_session(s.db.as_ref()).await {
        Ok(Some(row)) => Json(json!(row)),
        Ok(None)      => Json(json!(null)),
        Err(e)        => Json(json!({"error": e.to_string()})),
    }
}

async fn start_session(
    State(s): State<Arc<AppState>>,
    Json(body): Json<NewSession>,
) -> Json<Value> {
    // Verificar que no haya una sesión activa
    if let Ok(Some(_)) = repository::get_active_session(s.db.as_ref()).await {
        return Json(json!({"ok": false, "error": "Ya existe una sesión de grabación activa" }));
    }

    let btc_price = *s.btc_price.read().await;
    let duration = body.duration_min.max(1).min(60 * 24); // max 24h
    let depth = body.depth_levels.max(5).min(50);

    match repository::create_session(s.db.as_ref(), &body.name, duration, depth, btc_price).await {
        Ok(id) => {
            // Set active session in AppState
            *s.recording_session.write().await = Some(id);
            Json(json!({"ok": true, "id": id, "status": "recording" }))
        }
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

async fn stop_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    let final_price = *s.btc_price.read().await;
    match repository::stop_session(s.db.as_ref(), id, final_price, final_price).await {
        Ok(_) => {
            *s.recording_session.write().await = None;
            Json(json!({"ok": true, "message": "Sesión finalizada" }))
        }
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

async fn delete_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::delete_session(s.db.as_ref(), id).await {
        Ok(_) => Json(json!({"ok": true })),
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

async fn session_snapshots(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::list_session_snapshots(s.db.as_ref(), id).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn session_trades(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::list_session_trades(s.db.as_ref(), id).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

// ─── Export Session (CSV) ─────────────────────────────────────────────────────

async fn export_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Query(q): Query<ExportQuery>,
) -> Response {
    let format = q.format.as_deref().unwrap_or("json");

    let snapshots = match repository::list_session_snapshots(s.db.as_ref(), id).await {
        Ok(rows) => rows,
        Err(e)   => return Json(json!({"error": e.to_string()})).into_response(),
    };

    if format == "csv" {
        let mut csv = String::from("ts,side,best_bid,best_bid_sz,best_ask,best_ask_sz,spread,mid_price,bid_volume,ask_volume,btc_price\n");
        for snap in snapshots {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                snap.ts.to_rfc3339(),
                snap.side,
                snap.best_bid.unwrap_or(0.0),
                snap.best_bid_sz.unwrap_or(0.0),
                snap.best_ask.unwrap_or(0.0),
                snap.best_ask_sz.unwrap_or(0.0),
                snap.spread.unwrap_or(0.0),
                snap.mid_price.unwrap_or(0.0),
                snap.bid_volume.unwrap_or(0.0),
                snap.ask_volume.unwrap_or(0.0),
                snap.btc_price.unwrap_or(0.0),
            ));
        }
        return (
            StatusCode::OK,
            [("Content-Type", "text/csv"), ("Content-Disposition", &format!("attachment; filename=\"session_{}.csv\"", id))],
            csv,
        ).into_response();
    }

    // JSON default
    Json(json!({ "snapshots": snapshots })).into_response()
}

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,  // "csv" | "json"
}
