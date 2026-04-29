use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

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
        .route("/api/sessions/stop-all",    post(stop_all_sessions))
        .route("/api/sessions/cleanup",     post(cleanup_sessions))
        .route("/api/sessions/{id}",         delete(delete_session))
        .route("/api/sessions/{id}",         patch(update_session_tag))
        .route("/api/sessions/{id}/export",  get(export_session))
        .route("/api/sessions/{id}/snapshots", get(session_snapshots))
        .route("/api/sessions/{id}/trades",  get(session_trades))
        .route("/api/sessions/{id}/children", get(session_children))
        .with_state(state)
}

async fn list_sessions(State(s): State<Arc<AppState>>) -> Json<Value> {
    match repository::list_sessions(&s, 100).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn get_active_session(State(s): State<Arc<AppState>>) -> Json<Value> {
    match repository::get_active_session(&s).await {
        Ok(Some(row)) => Json(json!(row)),
        Ok(None)      => Json(json!(null)),
        Err(e)        => Json(json!({"error": e.to_string()})),
    }
}

async fn start_session(
    State(s): State<Arc<AppState>>,
    Json(body): Json<NewSession>,
) -> Json<Value> {
    use chrono::{Duration, Timelike};

    let now = Utc::now();
    let depth = body.depth_levels.max(5).min(50);

    // ── Indefinite mode: parent + auto-generated children ────────────────────
    if body.indefinite.unwrap_or(false) {
        let chunk_min = body.duration_min.max(1); // chunk size: 5, 15, etc. Default 15
        let parent_start = body.scheduled_start.unwrap_or(now);
        let parent_end = parent_start + Duration::days(365);
        let parent_name = if body.name.is_empty() {
            format!("BTC{}-Indefinida-{}", chunk_min, parent_start.format("%Y%m%dT%H%M"))
        } else {
            body.name.clone()
        };

        // 1. Create parent — duration_min stores the chunk size for auto-generation
        let parent_id = match repository::create_session(&s, &parent_name, parent_start, parent_end, chunk_min, depth, None).await {
            Ok(id) => id,
            Err(e) => return Json(json!({"ok": false, "error": format!("Parent: {}", e)})),
        };

        // 2. Start parent immediately
        let btc_price = *s.btc_price.read().await;
        if let Err(e) = repository::start_session_recording(&s, parent_id, btc_price).await {
            warn!("No se pudo iniciar padre #{}: {}", parent_id, e);
        }

        // 3. Create first child
        let child_start = parent_start;
        let child_end = child_start + Duration::minutes(chunk_min as i64);
        let child_name = child_session_name(child_start, chunk_min);
        let child_id = match repository::create_session(&s, &child_name, child_start, child_end, chunk_min, depth, Some(parent_id)).await {
            Ok(id) => id,
            Err(e) => {
                let _ = repository::stop_session(&s, parent_id, None, None).await;
                return Json(json!({"ok": false, "error": format!("First child: {}", e)}));
            }
        };

        // 4. Start child immediately
        if let Err(e) = repository::start_session_recording(&s, child_id, btc_price).await {
            warn!("No se pudo iniciar hijo #{}: {}", child_id, e);
        }
        *s.recording_session.write().await = Some(child_id);

        let children = repository::list_session_children(&s, parent_id).await.unwrap_or_default();
        return Json(json!({
            "ok": true,
            "id": parent_id,
            "status": "recording",
            "indefinite": true,
            "chunk_min": chunk_min,
            "scheduled_start": parent_start.to_rfc3339(),
            "scheduled_end": parent_end.to_rfc3339(),
            "child_ids": vec![child_id],
            "children": children,
            "active_child_id": child_id,
            "message": format!("Sesión INDEFINIDA iniciada. Hijos de {}min auto-generados. Primer hijo: {} ({})",
                chunk_min, child_name, child_start.format("%H:%M")),
        }));
    }

    // ── Non-indefinite (existing logic) ──────────────────────────────────────
    let (scheduled_start, scheduled_end) = if let (Some(start), Some(end)) = (body.scheduled_start, body.scheduled_end) {
        (start, end)
    } else if let Some(start) = body.scheduled_start {
        let end = start + Duration::minutes(body.duration_min.max(1) as i64);
        (start, end)
    } else {
        let minute = now.minute();
        let next_min = ((minute / 15) + 1) * 15;
        let start = if next_min >= 60 {
            now.with_minute(0).unwrap() + Duration::hours(1)
        } else {
            now.with_minute(next_min).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        };
        let end = start + Duration::minutes(body.duration_min.max(15) as i64);
        (start, end)
    };

    let name = if body.name.is_empty() {
        format!("BTC-{}", scheduled_start.format("%H%M"))
    } else {
        body.name
    };
    let total_duration = ((scheduled_end - scheduled_start).num_seconds() / 60).max(1) as i32;

    if total_duration > 15 {
        match repository::create_session_batch(&s, &name, scheduled_start, total_duration, depth, 15).await {
            Ok((parent_id, child_ids)) => {
                let children = repository::list_session_children(&s, parent_id).await.unwrap_or_default();
                Json(json!({
                    "ok": true,
                    "id": parent_id,
                    "status": "scheduled",
                    "scheduled_start": scheduled_start.to_rfc3339(),
                    "scheduled_end": scheduled_end.to_rfc3339(),
                    "child_ids": child_ids,
                    "children": children,
                    "total_duration_min": total_duration,
                    "chunk_duration_min": 15,
                    "message": format!("Sesión de {}min creada con {} bloques de 15min cada uno. Programada para {} → {}",
                        total_duration, child_ids.len(), scheduled_start.format("%H:%M"), scheduled_end.format("%H:%M"))
                }))
            }
            Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
        }
    } else {
        let duration = total_duration.max(1);
        match repository::create_session(&s, &name, scheduled_start, scheduled_end, duration, depth, None).await {
            Ok(id) => Json(json!({
                "ok": true,
                "id": id,
                "status": "scheduled",
                "scheduled_start": scheduled_start.to_rfc3339(),
                "scheduled_end": scheduled_end.to_rfc3339(),
                "indefinite": false,
                "child_ids": [],
                "children": [],
                "message": format!("Sesión programada para {} → {}", scheduled_start.format("%H:%M"), scheduled_end.format("%H:%M"))
            })),
            Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
        }
    }
}

/// Genera nombre de sesión hijo: BTC{chunk_min}{YYYYMMDD}_{HHMM}UTC
pub(crate) fn child_session_name(start: DateTime<Utc>, chunk_min: i32) -> String {
    format!("BTC{}{}_{}UTC", chunk_min, start.format("%Y%m%d"), start.format("%H%M"))
}

async fn stop_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    let final_price = *s.btc_price.read().await;

    // Cascada: si es un padre, detener también todos los hijos activos
    if let Ok(children) = repository::list_session_children(&s, id).await {
        for child in &children {
            if child.status == "recording" || child.status == "scheduled" {
                let _ = repository::stop_session(&s, child.id, final_price, final_price).await;
            }
        }
    }

    match repository::stop_session(&s, id, final_price, final_price).await {
        Ok(_) => {
            *s.recording_session.write().await = None;
            Json(json!({"ok": true, "message": "Sesión finalizada" }))
        }
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

async fn stop_all_sessions(
    State(s): State<Arc<AppState>>,
) -> Json<Value> {
    let btc_price = *s.btc_price.read().await;
    // Stop recording sessions
    let mut stopped = 0;
    let mut cancelled = 0;

    if let Some(pool) = s.db.as_ref() {
        // Stop all 'recording' sessions
        let recording: Vec<(i32,)> = sqlx::query_as(
            "SELECT id FROM recording_sessions WHERE status = 'recording'"
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        for (id,) in &recording {
            if repository::stop_session(&s, *id, btc_price, btc_price).await.is_ok() {
                stopped += 1;
            }
        }

        // Cancel all 'scheduled' sessions
        let result = sqlx::query(
            "UPDATE recording_sessions SET status = 'cancelled' WHERE status = 'scheduled'"
        )
        .execute(pool)
        .await;
        if let Ok(r) = result {
            cancelled = r.rows_affected() as i32;
        }
    }

    // Also handle in-memory
    {
        let mut sessions = s.mem_sessions.write().await;
        for s in sessions.iter_mut() {
            if s.status == "recording" {
                s.status = "completed".into();
                s.stopped_at = Some(Utc::now());
                s.final_price = btc_price;
                s.btc_price_end = btc_price;
                stopped += 1;
            } else if s.status == "scheduled" {
                s.status = "cancelled".into();
                cancelled += 1;
            }
        }
    }

    *s.recording_session.write().await = None;

    Json(json!({
        "ok": true,
        "stopped": stopped,
        "cancelled": cancelled,
        "message": format!("{} sesiones detenidas, {} canceladas", stopped, cancelled)
    }))
}

async fn cleanup_sessions(
    State(s): State<Arc<AppState>>,
) -> Json<Value> {
    if let Some(pool) = s.db.as_ref() {
        // Delete cancelled sessions + completed with no data (cascades to snapshots/trades)
        let result = sqlx::query(
            r#"
            DELETE FROM recording_sessions
            WHERE status IN ('cancelled')
               OR (status = 'completed' AND tick_count = 0 AND trade_count = 0)
            "#
        )
        .execute(pool)
        .await;

        match result {
            Ok(r) => Json(json!({
                "ok": true,
                "deleted": r.rows_affected(),
                "message": format!("{} sesiones eliminadas", r.rows_affected())
            })),
            Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
        }
    } else {
        // In-memory cleanup
        let mut sessions = s.mem_sessions.write().await;
        let before = sessions.len();
        sessions.retain(|s| {
            !(s.status == "cancelled" || (s.status == "completed" && s.tick_count == 0 && s.trade_count == 0))
        });
        let deleted = before - sessions.len();

        Json(json!({
            "ok": true,
            "deleted": deleted,
            "message": format!("{} sesiones eliminadas", deleted)
        }))
    }
}

async fn delete_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::delete_session(&s, id).await {
        Ok(_) => Json(json!({"ok": true })),
        Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
struct TagUpdate {
    tag:       Option<String>,
    tag_color: Option<String>,
}

async fn update_session_tag(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(body): Json<TagUpdate>,
) -> Json<Value> {
    match repository::update_session_tag(&s, id, body.tag, body.tag_color).await {
        Ok(true)  => Json(json!({"ok": true})),
        Ok(false) => Json(json!({"ok": false, "error": "session not found"})),
        Err(e)    => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn session_snapshots(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::list_session_snapshots(&s, id).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn session_trades(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::list_session_trades(&s, id).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn session_children(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    match repository::list_session_children(&s, id).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

// ─── Export Session (CSV / JSON / Parquet) ────────────────────────────────────

async fn export_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Response {
    let session = match repository::get_session_by_id(&s, id).await {
        Ok(Some(session)) => session,
        Ok(None) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => return Json(json!({"error": e.to_string()})).into_response(),
    };

    let rows = repository::query_hft_snapshots(s.db.as_ref(), id).await.unwrap_or_default();

    // Fallback to in-memory buffer if DB is empty
    let has_data = !rows.is_empty();
    let mem_rows = if !has_data {
        let mem = s.mem_hft.read().await;
        mem.iter().cloned().collect::<Vec<_>>()
    } else {
        vec![]
    };

    let mut csv = String::new();
    // Metadata header
    csv.push_str("# Session Metadata\n");
    csv.push_str(&format!("# session_id={}\n", session.id));
    csv.push_str(&format!("# name={}\n", session.name));
    csv.push_str(&format!("# scheduled_start={}\n", session.scheduled_start.to_rfc3339()));
    csv.push_str(&format!("# scheduled_end={}\n", session.scheduled_end.to_rfc3339()));
    csv.push_str(&format!("# started_at={}\n", session.started_at.map_or("".into(), |t| t.to_rfc3339())));
    csv.push_str(&format!("# stopped_at={}\n", session.stopped_at.map_or("".into(), |t| t.to_rfc3339())));
    csv.push_str(&format!("# duration_min={}\n", session.duration_min));
    csv.push_str(&format!("# btc_price_start={}\n", session.btc_price_start.unwrap_or(0.0)));
    csv.push_str(&format!("# btc_price_end={}\n", session.btc_price_end.unwrap_or(0.0)));
    csv.push_str(&format!("# final_price={}\n", session.final_price.unwrap_or(0.0)));
    csv.push_str(&format!("# outcome_result={}\n", session.outcome_result.unwrap_or_default()));
    csv.push_str(&format!("# status={}\n", session.status));
    csv.push_str(&format!("# tick_count={}\n", session.tick_count));
    csv.push_str(&format!("# trade_count={}\n", session.trade_count));
    csv.push_str(&format!("#\n"));
    // Unified 20-column header
    csv.push_str("ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,trade_price,trade_size,is_informed\n");

    if has_data {
        for r in &rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                r.ts.to_rfc3339(), "", "BOOK_UPDATE",
                r.binance_lag_ms.unwrap_or(0),
                r.btc_price_binance.unwrap_or(0.0),
                r.binance_micro_price_at_t.unwrap_or(0.0),
                0.0, 0.0, 0.0,
                0.0, 0.0,
                r.poly_mid_price.unwrap_or(0.0),
                0.0, 0.0, 0.0,
                r.poly_imbalance.unwrap_or(0.0),
                "", 0.0, 0.0, 0,
            ));
        }
    } else {
        for r in &mem_rows {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                r.ts_local, r.ts_exchange, r.event_type.as_str(),
                r.latencia_ms, r.binance_price, r.binance_micro_price,
                r.binance_imbalance, r.binance_vol_100ms, r.binance_vol_24h,
                r.poly_bid, r.poly_ask, r.poly_mid, r.poly_spread,
                r.poly_bid_vol_all, r.poly_ask_vol_all, r.poly_imbalance,
                r.trade_side, r.trade_price, r.trade_size, r.is_informed,
            ));
        }
    }

    (StatusCode::OK,
     [("Content-Type", "text/csv"),
      ("Content-Disposition", &format!("attachment; filename=\"session_{}_hft.csv\"", id))],
     csv).into_response()
}
