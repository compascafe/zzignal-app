use std::io::Write;
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
use tracing::{info, warn};
use zip::write::SimpleFileOptions;

use crate::modules::core::state::AppState;
use crate::modules::db::models::ScheduledExecution;
use crate::modules::db::repository;
use crate::modules::db::scheduler::snap_to_next_chunk;
use crate::modules::hft::adaptive_risk_engine::warmup_fetch_and_compute;

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
        .route("/api/sessions/export-bulk", get(export_bulk_sessions))
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

        // 2. Start parent immediately (parent is a container — no CSV file, no recording_sessions)
        let btc_price = *s.btc_price.read().await;
        if let Err(e) = repository::start_session_recording(&s, parent_id, btc_price).await {
            warn!("No se pudo iniciar padre #{}: {}", parent_id, e);
        }

        // 3. Create first child — snap to NEXT chunk boundary (9:27 → 9:30)
        let child_start = snap_to_next_chunk(parent_start, chunk_min);
        let child_end = child_start + Duration::minutes(chunk_min as i64);
        let child_name = child_session_name(child_start, chunk_min);
        let child_id = match repository::create_session(&s, &child_name, child_start, child_end, chunk_min, depth, Some(parent_id)).await {
            Ok(id) => id,
            Err(e) => {
                let _ = repository::stop_session(&s, parent_id, None, None).await;
                return Json(json!({"ok": false, "error": format!("First child: {}", e)}));
            }
        };

        // 4. Adaptive Risk Engine: refresh warm‑up + feedback from history for this child
        {
            let recent_accuracy = repository::get_recent_accuracy(s.db.as_ref(), 16).await;
            let mut eng = s.adaptive_engine.lock().await;
            for correct in &recent_accuracy {
                eng.cp_mut().record_accuracy(*correct);
            }
            if eng.cp_mut().should_auto_widen() {
                eng.cp_mut().auto_widen();
            }
        }
        {
            let warm_state = Arc::clone(&s);
            let dur = chunk_min;
            tokio::spawn(async move {
                match warmup_fetch_and_compute().await {
                    Ok(result) => {
                        warm_state.adaptive_engine.lock().await.apply_warmup_result(result);
                        info!("AdaptiveRiskEngine: warm‑up refreshed for {}-min child (indefinite start)", dur);
                    }
                    Err(e) => warn!("AdaptiveRiskEngine refresh failed on indefinite start: {e}"),
                }
            });
        }

        // 5. Start child immediately
        s.recording_sessions.write().await.push(child_id);
        if let Err(e) = s.session_manager.start_session(child_id) {
            warn!("SessionManager start child #{}: {}", child_id, e);
        }
        if let Err(e) = repository::start_session_recording(&s, child_id, btc_price).await {
            warn!("No se pudo iniciar hijo #{}: {}", child_id, e);
        }

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
    let (scheduled_start, scheduled_end, effective_dur) = if let (Some(start), Some(end)) = (body.scheduled_start, body.scheduled_end) {
        let dur = ((end - start).num_seconds() / 60).max(1) as i32;
        (start, end, dur)
    } else if let Some(start) = body.scheduled_start {
        let dur = body.duration_min.max(1);
        let end = start + Duration::minutes(dur as i64);
        (start, end, dur)
    } else {
        let minute = now.minute();
        let requested = body.duration_min.max(1);

        // ── Smart grid alignment ───────────────────────────────────────────
        // 15-min sessions only start at :00, :15, :30, :45.
        // If we're not on a 15-min boundary, fall back to 5-min session.
        let dur = if requested == 15 && minute % 15 != 0 {
            info!("[SESSION ALIGN] {}min requested at :{:02} — not on 15-min grid, falling back to 5-min session", requested, minute);
            5i32
        } else {
            requested
        };
        let grid = if dur == 5 { 5 } else { 15 };

        let next_min = ((minute / grid) + 1) * grid;
        let start = if next_min >= 60 {
            now.with_minute(0).unwrap() + chrono::Duration::hours(1)
        } else {
            now.with_minute(next_min).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        };
        let end = start + chrono::Duration::minutes(dur as i64);
        (start, end, dur)
    };

    let name = if body.name.is_empty() {
        format!("BTC-{}", scheduled_start.format("%H%M"))
    } else {
        body.name
    };
    let total_duration = ((scheduled_end - scheduled_start).num_seconds() / 60).max(1) as i32;

    if total_duration > 15 {
        let chunk_min = effective_dur.max(1);
        match repository::create_session_batch(&s, &name, scheduled_start, total_duration, depth, chunk_min).await {
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
                    "chunk_duration_min": chunk_min,
                    "message": format!("Sesión de {}min creada con {} bloques de {}min cada uno. Programada para {} → {}",
                        total_duration, child_ids.len(), chunk_min, scheduled_start.format("%H:%M"), scheduled_end.format("%H:%M"))
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
                "message": format!("Sesión {}min programada para {} → {}{}",
                    effective_dur,
                    scheduled_start.format("%H:%M"),
                    scheduled_end.format("%H:%M"),
                    if effective_dur != body.duration_min { " (auto-ajustado de 15→5min: fuera de grid 15min)" } else { "" }
                )
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
                // Remove child from active recording sessions
                s.recording_sessions.write().await.retain(|&sid| sid != child.id);
            }
        }
    }

    match repository::stop_session(&s, id, final_price, final_price).await {
        Ok(_) => {
            s.recording_sessions.write().await.retain(|&sid| sid != id);
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

    s.recording_sessions.write().await.clear();
    s.tracking_state.reset_session_baselines();

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

fn build_csv_body(session: &crate::modules::db::models::RecordingSession, raw_data: &str) -> String {
    let mut csv = String::new();
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
    csv.push_str(&format!("# outcome_result={}\n", session.outcome_result.clone().unwrap_or_default()));
    csv.push_str(&format!("# status={}\n", session.status));
    csv.push_str(&format!("# tick_count={}\n", session.tick_count));
    csv.push_str(&format!("# trade_count={}\n", session.trade_count));
    csv.push_str("\n");
    csv.push_str(raw_data);
    csv
}

async fn build_fallback_csv(
    session: &crate::modules::db::models::RecordingSession,
    s: &Arc<AppState>,
    id: i32,
) -> String {
    use std::fmt::Write;

    let mut rows: Vec<CsvFallbackRow> = Vec::new();

    // 1) Rich session_snapshots (best_bid, best_ask, mid, spread, imbalance, btc_price, etc.)
    if let Ok(snaps) = repository::list_session_snapshots(s, id).await {
        for snap in &snaps {
            let poly_bid = snap.best_bid.unwrap_or(0.0);
            let poly_ask = snap.best_ask.unwrap_or(0.0);
            let poly_mid = snap.mid_price.unwrap_or(0.0);
            let poly_spread = snap.spread.unwrap_or(0.0);
            let poly_bid_vol = snap.bid_volume.unwrap_or(0.0);
            let poly_ask_vol = snap.ask_volume.unwrap_or(0.0);
            let poly_imb = snap.imbalance_ratio.unwrap_or(0.0);
            let binance_price = snap.btc_price.unwrap_or(0.0);

            rows.push(CsvFallbackRow {
                ts: snap.ts,
                event_type: "BOOK_UPDATE",
                binance_price,
                binance_micro_price: 0.0,
                binance_imbalance: 0.0,
                binance_vol_100ms: 0.0,
                binance_vol_24h: 0.0,
                poly_bid,
                poly_ask,
                poly_mid,
                poly_spread,
                poly_bid_vol_all: poly_bid_vol,
                poly_ask_vol_all: poly_ask_vol,
                poly_imbalance: poly_imb,
                trade_side: String::new(),
                trade_price: 0.0,
                trade_size: 0.0,
                is_informed: 0u8,
                latencia_ms: 0,
                macro_slope: 0.0,
                vfi_value: 0.0,
                macd_hist: 0.0,
                predicted_bias: String::new(),
                is_feedback_adjusted: 0,
                dynamic_rsi: 0.0,
                vfi_confidence: 0.0,
                db_accuracy_factor: 1.0,
            });
        }
    }

    // 2) Session trades (price, size, side)
    if let Ok(trades) = repository::list_session_trades(s, id).await {
        for t in &trades {
            rows.push(CsvFallbackRow {
                ts: t.ts,
                event_type: "TRADE",
                binance_price: t.btc_price.unwrap_or(0.0),
                binance_micro_price: 0.0,
                binance_imbalance: 0.0,
                binance_vol_100ms: 0.0,
                binance_vol_24h: 0.0,
                poly_bid: 0.0,
                poly_ask: 0.0,
                poly_mid: 0.0,
                poly_spread: 0.0,
                poly_bid_vol_all: 0.0,
                poly_ask_vol_all: 0.0,
                poly_imbalance: 0.0,
                trade_side: t.trade_side.clone(),
                trade_price: t.price,
                trade_size: t.size,
                is_informed: 0u8,
                latencia_ms: 0,
                macro_slope: 0.0,
                vfi_value: 0.0,
                macd_hist: 0.0,
                predicted_bias: String::new(),
                is_feedback_adjusted: 0,
                dynamic_rsi: 0.0,
                vfi_confidence: 0.0,
                db_accuracy_factor: 1.0,
            });
        }
    }

    // 3) If both session_snapshots and session_trades are empty, try legacy hft_snapshots / mem_hft
    if rows.is_empty() {
        let db_rows = repository::query_hft_snapshots(s.db.as_ref(), id).await.unwrap_or_default();
        let has_data = !db_rows.is_empty();
        let mem_rows = if !has_data {
            let mem = s.mem_hft.read().await;
            mem.iter()
                .filter(|r| r.session_id == id)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            vec![]
        };

        if has_data {
            for r in &db_rows {
                rows.push(CsvFallbackRow {
                    ts: r.ts,
                    event_type: "BOOK_UPDATE",
                    binance_price: r.btc_price_binance.unwrap_or(0.0),
                    binance_micro_price: r.binance_micro_price_at_t.unwrap_or(0.0),
                    binance_imbalance: 0.0,
                    binance_vol_100ms: 0.0,
                    binance_vol_24h: 0.0,
                    poly_bid: 0.0,
                    poly_ask: 0.0,
                    poly_mid: r.poly_mid_price.unwrap_or(0.0),
                    poly_spread: 0.0,
                    poly_bid_vol_all: 0.0,
                    poly_ask_vol_all: 0.0,
                    poly_imbalance: r.poly_imbalance.unwrap_or(0.0),
                    trade_side: String::new(),
                    trade_price: 0.0,
                    trade_size: 0.0,
                    is_informed: 0u8,
                    latencia_ms: r.binance_lag_ms.unwrap_or(0),
                    macro_slope: 0.0,
                    vfi_value: 0.0,
                    macd_hist: 0.0,
                    predicted_bias: String::new(),
                    is_feedback_adjusted: 0,
                    dynamic_rsi: 0.0,
                    vfi_confidence: 0.0,
                    db_accuracy_factor: 1.0,
                });
            }
        } else {
            for r in &mem_rows {
                rows.push(CsvFallbackRow {
                    ts: chrono::DateTime::parse_from_rfc3339(&r.ts_local)
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    event_type: r.event_type.as_str(),
                    binance_price: r.binance_price,
                    binance_micro_price: r.binance_micro_price,
                    binance_imbalance: r.binance_imbalance as f64,
                    binance_vol_100ms: r.binance_vol_100ms,
                    binance_vol_24h: r.binance_vol_24h,
                    poly_bid: r.poly_bid,
                    poly_ask: r.poly_ask,
                    poly_mid: r.poly_mid,
                    poly_spread: r.poly_spread,
                    poly_bid_vol_all: r.poly_bid_vol_all,
                    poly_ask_vol_all: r.poly_ask_vol_all,
                    poly_imbalance: r.poly_imbalance,
                    trade_side: r.trade_side.clone(),
                    trade_price: r.trade_price,
                    trade_size: r.trade_size,
                    is_informed: r.is_informed,
                    latencia_ms: r.latencia_ms,
                    macro_slope: 0.0,
                    vfi_value: 0.0,
                    macd_hist: 0.0,
                predicted_bias: String::new(),
                is_feedback_adjusted: 0,
                dynamic_rsi: 0.0,
                vfi_confidence: 0.0,
                db_accuracy_factor: 1.0,
            });
        }
    }
    }

    // Sort by timestamp
    rows.sort_by_key(|r| r.ts);

    let mut data = String::new();
    // 61-column data header
    data.push_str("ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,trade_price,trade_size,is_informed,imba_status,imba_side,imba_entry_price,imba_exit_price,imba_trade_pnl,imba_balance,liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance,trades_per_second,price_velocity,poly_liquidity_delta,absorption_ratio,price_gap_ratio,spoofing_flag,tape_speed_flag,gap_alert_flag,bollinger_sma,bollinger_upper,bollinger_lower,mean_reversion_signal,technical_confluence,trend_direction,signal_label,realized_volatility,high_volatility_event,bollinger_position,master_signal,cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor\n");

    for r in &rows {
        let _ = writeln!(
            data,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            r.ts.to_rfc3339(), "", r.event_type,
            r.latencia_ms, r.binance_price, r.binance_micro_price,
            r.binance_imbalance, r.binance_vol_100ms, r.binance_vol_24h,
            r.poly_bid, r.poly_ask, r.poly_mid, r.poly_spread,
            r.poly_bid_vol_all, r.poly_ask_vol_all, r.poly_imbalance,
            r.trade_side, r.trade_price, r.trade_size, r.is_informed,
            "N/A", "", 0.0, 0.0, 0.0, 0.0,
            "N/A", "", 0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0, 0, 0, 0,
            0.0, 0.0, 0.0, 0, 0, 0, "",
            0.0, 0, 0, 0, 0.0, 0,
            r.macro_slope, r.vfi_value, r.macd_hist, r.predicted_bias, r.is_feedback_adjusted,
            r.dynamic_rsi, r.vfi_confidence, r.db_accuracy_factor,
        );
    }

    build_csv_body(session, &data)
}

struct CsvFallbackRow {
    ts: chrono::DateTime<chrono::Utc>,
    event_type: &'static str,
    binance_price: f64,
    binance_micro_price: f64,
    binance_imbalance: f64,
    binance_vol_100ms: f64,
    binance_vol_24h: f64,
    poly_bid: f64,
    poly_ask: f64,
    poly_mid: f64,
    poly_spread: f64,
    poly_bid_vol_all: f64,
    poly_ask_vol_all: f64,
    poly_imbalance: f64,
    trade_side: String,
    trade_price: f64,
    trade_size: f64,
    is_informed: u8,
    latencia_ms: i64,
    macro_slope: f64,
    vfi_value: f64,
    macd_hist: f64,
    predicted_bias: String,
    is_feedback_adjusted: u8,
    dynamic_rsi: f64,
    vfi_confidence: f64,
    db_accuracy_factor: f64,
}

/// Try to read the per-session CSV file from disk (written by SessionManager).
/// Flushes the writer first to ensure buffered data is on disk, then returns the full file content.
/// Returns None if file doesn't exist or is empty after trim.
fn read_session_csv_disk(session_manager: &crate::modules::hft::session_manager::SessionManager, id: i32) -> Option<String> {
    // Flush BufWriter to disk before reading — data may be buffered in RAM
    let _ = session_manager.flush(id);
    let path = session_manager.session_path(id);
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim_end().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

async fn export_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Response {
    let session = match repository::get_session_by_id(&s, id).await {
        Ok(Some(session)) => session,
        Ok(None) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
        Err(e) => return Json(json!({"error": e.to_string()})).into_response(),
    };

    // Prefer the rich per-session CSV file on disk (written during recording)
    let csv = if let Some(raw_data) = read_session_csv_disk(&s.session_manager, id) {
        build_csv_body(&session, &raw_data)
    } else {
        build_fallback_csv(&session, &s, id).await
    };

    (StatusCode::OK,
     [("Content-Type", "text/csv"),
      ("Content-Disposition", &format!("attachment; filename=\"session_{}_hft.csv\"", id))],
     csv).into_response()
}

#[derive(Deserialize)]
struct BulkExportQuery {
    ids: String, // comma-separated session IDs
}

async fn export_bulk_sessions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<BulkExportQuery>,
) -> Response {
    let ids: Vec<i32> = q.ids.split(',')
        .filter_map(|p| p.trim().parse::<i32>().ok())
        .collect();

    if ids.is_empty() {
        return (StatusCode::BAD_REQUEST, "no valid session ids").into_response();
    }

    // Build ZIP in memory
    let mut zip_buf = Vec::new();
    let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for id in ids {
        let session = match repository::get_session_by_id(&s, id).await {
            Ok(Some(sess)) => sess,
            Ok(None) => continue,
            Err(_) => continue,
        };

        // Prefer the rich per-session CSV file on disk
        let csv = if let Some(raw_data) = read_session_csv_disk(&s.session_manager, id) {
            build_csv_body(&session, &raw_data)
        } else {
            build_fallback_csv(&session, &s, id).await
        };

        let filename = format!("session_{}_hft.csv", id);
        if zip_writer.start_file(&filename, options).is_err() { continue; }
        if zip_writer.write_all(csv.as_bytes()).is_err() { continue; }
    }

    let _ = zip_writer.finish();

    (StatusCode::OK,
     [("Content-Type", "application/zip"),
      ("Content-Disposition", "attachment; filename=\"sessions_bulk.zip\"")],
     zip_buf).into_response()
}
