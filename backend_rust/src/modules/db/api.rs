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
        .route("/api/sessions/stop-all",    post(stop_all_sessions))
        .route("/api/sessions/cleanup",     post(cleanup_sessions))
        .route("/api/sessions/{id}",         delete(delete_session))
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
    let indefinite = body.indefinite.unwrap_or(false);

    let (scheduled_start, scheduled_end, is_indefinite) = if indefinite {
        let start = body.scheduled_start.unwrap_or(now);
        let end = start + Duration::days(365);
        (start, end, true)
    } else if let (Some(start), Some(end)) = (body.scheduled_start, body.scheduled_end) {
        (start, end, false)
    } else if let Some(start) = body.scheduled_start {
        let end = start + Duration::minutes(body.duration_min.max(1) as i64);
        (start, end, false)
    } else {
        let minute = now.minute();
        let next_min = ((minute / 15) + 1) * 15;
        let start = if next_min >= 60 {
            now.with_minute(0).unwrap() + Duration::hours(1)
        } else {
            now.with_minute(next_min).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        };
        let end = start + Duration::minutes(body.duration_min.max(15) as i64);
        (start, end, false)
    };

    let name = if body.name.is_empty() {
        format!("BTC-{}", scheduled_start.format("%H%M"))
    } else {
        body.name
    };
    let depth = body.depth_levels.max(5).min(50);
    let total_duration = ((scheduled_end - scheduled_start).num_seconds() / 60).max(1) as i32;

    // Auto-split sessions longer than 15 min into 15-min children
    if total_duration > 15 && !is_indefinite {
        match repository::create_session_batch(&s, &name, scheduled_start, total_duration, depth, 15).await {
            Ok((parent_id, child_ids)) => {
                // Fetch children for response
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
        // Single session (≤15 min or indefinite)
        let duration = total_duration.max(1);
        match repository::create_session(&s, &name, scheduled_start, scheduled_end, duration, depth, None).await {
            Ok(id) => Json(json!({
                "ok": true,
                "id": id,
                "status": "scheduled",
                "scheduled_start": scheduled_start.to_rfc3339(),
                "scheduled_end": scheduled_end.to_rfc3339(),
                "indefinite": is_indefinite,
                "child_ids": [],
                "children": [],
                "message": if is_indefinite {
                    "Sesión INDEFINIDA iniciada. Detener manualmente.".into()
                } else {
                    format!("Sesión programada para {} → {}", scheduled_start.format("%H:%M"), scheduled_end.format("%H:%M"))
                }
            })),
            Err(e) => Json(json!({"ok": false, "error": e.to_string() })),
        }
    }
}

async fn stop_session(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Json<Value> {
    let final_price = *s.btc_price.read().await;
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
    Query(q): Query<ExportQuery>,
) -> Response {
    let format = q.format.as_deref().unwrap_or("json");

    // Fetch snapshots: from session directly + all children if parent
    let mut snapshots = match repository::list_session_snapshots(&s, id).await {
        Ok(rows) => rows,
        Err(e)   => return Json(json!({"error": e.to_string()})).into_response(),
    };

    // Aggregate children snapshots
    if let Ok(children) = repository::list_session_children(&s, id).await {
        for child in &children {
            if let Ok(child_snaps) = repository::list_session_snapshots(&s, child.id).await {
                snapshots.extend(child_snaps);
            }
        }
        // Sort by ts for chronological export
        snapshots.sort_by(|a, b| a.ts.cmp(&b.ts));
    }

    if format == "csv" || format == "csv-depth" {
        let is_depth = format == "csv-depth";
        let mut csv = if is_depth {
            String::from("ts,side,level,price,size,btc_price\n")
        } else {
            String::from("ts,side,session_id,best_bid,best_bid_sz,best_ask,best_ask_sz,spread,mid_price,bid_vol_5,ask_vol_5,bid_vol_10,ask_vol_10,bid_vol_all,ask_vol_all,imbalance_ratio,up_prob,down_prob,btc_price\n")
        };

        for snap in &snapshots {
            if is_depth {
                // Expand depth: one row per price level
                let btc = snap.btc_price.unwrap_or(0.0);
                let ts = snap.ts.to_rfc3339();
                // Parse depth_bids
                if let Some(ref bids) = snap.depth_bids {
                    if let Some(arr) = bids.as_array() {
                        for (i, level) in arr.iter().enumerate() {
                            let p = level["p"].as_f64().unwrap_or(0.0);
                            let s = level["s"].as_f64().unwrap_or(0.0);
                            csv.push_str(&format!("{},{},bid_{},{},{},{}\n", ts, snap.side, i, p, s, btc));
                        }
                    }
                }
                // Parse depth_asks
                if let Some(ref asks) = snap.depth_asks {
                    if let Some(arr) = asks.as_array() {
                        for (i, level) in arr.iter().enumerate() {
                            let p = level["p"].as_f64().unwrap_or(0.0);
                            let s = level["s"].as_f64().unwrap_or(0.0);
                            csv.push_str(&format!("{},{},ask_{},{},{},{}\n", ts, snap.side, i, p, s, btc));
                        }
                    }
                }
            } else {
                csv.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                    snap.ts.to_rfc3339(),
                    snap.side,
                    snap.session_id,
                    snap.best_bid.unwrap_or(0.0),
                    snap.best_bid_sz.unwrap_or(0.0),
                    snap.best_ask.unwrap_or(0.0),
                    snap.best_ask_sz.unwrap_or(0.0),
                    snap.spread.unwrap_or(0.0),
                    snap.mid_price.unwrap_or(0.0),
                    snap.bid_volume_5.unwrap_or(0.0),
                    snap.ask_volume_5.unwrap_or(0.0),
                    snap.bid_volume_10.unwrap_or(0.0),
                    snap.ask_volume_10.unwrap_or(0.0),
                    snap.bid_volume.unwrap_or(0.0),
                    snap.ask_volume.unwrap_or(0.0),
                    snap.imbalance_ratio.unwrap_or(0.0),
                    snap.up_probability.unwrap_or(0.0),
                    snap.down_probability.unwrap_or(0.0),
                    snap.btc_price.unwrap_or(0.0),
                ));
            }
        }
        let fname = if is_depth { format!("session_{}_depth.csv", id) } else { format!("session_{}.csv", id) };
        return (
            StatusCode::OK,
            [("Content-Type", "text/csv"), ("Content-Disposition", &format!("attachment; filename=\"{}\"", fname))],
            csv,
        ).into_response();
    }

    if format == "parquet" {
        match write_parquet(&snapshots) {
            Ok(bytes) => {
                return (
                    StatusCode::OK,
                    [
                        ("Content-Type", "application/octet-stream"),
                        ("Content-Disposition", &format!("attachment; filename=\"session_{}.parquet\"", id)),
                    ],
                    bytes,
                ).into_response();
            }
            Err(e) => return Json(json!({"error": e.to_string()})).into_response(),
        }
    }

    // JSON default
    Json(json!({ "snapshots": snapshots })).into_response()
}

/// Writes session snapshots as a Parquet file
fn write_parquet(snapshots: &[crate::modules::db::models::SessionSnapshot]) -> Result<Vec<u8>, anyhow::Error> {
    use std::sync::Arc;
    use arrow::array::{Float64Builder, StringBuilder, TimestampMicrosecondBuilder, Int32Builder};
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use arrow::record_batch::RecordBatch;
    use parquet::arrow::ArrowWriter;
    use parquet::basic::Compression;
    use parquet::file::properties::WriterProperties;

    let schema = Arc::new(Schema::new(vec![
        Field::new("session_id", DataType::Int32, false),
        Field::new("ts", DataType::Timestamp(TimeUnit::Microsecond, None), false),
        Field::new("side", DataType::Utf8, false),
        Field::new("best_bid", DataType::Float64, true),
        Field::new("best_bid_sz", DataType::Float64, true),
        Field::new("best_ask", DataType::Float64, true),
        Field::new("best_ask_sz", DataType::Float64, true),
        Field::new("spread", DataType::Float64, true),
        Field::new("mid_price", DataType::Float64, true),
        Field::new("bid_vol_5", DataType::Float64, true),
        Field::new("ask_vol_5", DataType::Float64, true),
        Field::new("bid_vol_10", DataType::Float64, true),
        Field::new("ask_vol_10", DataType::Float64, true),
        Field::new("bid_vol_all", DataType::Float64, true),
        Field::new("ask_vol_all", DataType::Float64, true),
        Field::new("imbalance_ratio", DataType::Float64, true),
        Field::new("up_prob", DataType::Float64, true),
        Field::new("down_prob", DataType::Float64, true),
        Field::new("btc_price", DataType::Float64, true),
    ]));

    let n = snapshots.len();
    let mut session_id_b = Int32Builder::with_capacity(n);
    let mut ts_b = TimestampMicrosecondBuilder::with_capacity(n);
    let mut side_b = StringBuilder::with_capacity(n, n * 4);
    let mut bb_b = Float64Builder::with_capacity(n);
    let mut bbs_b = Float64Builder::with_capacity(n);
    let mut ba_b = Float64Builder::with_capacity(n);
    let mut bas_b = Float64Builder::with_capacity(n);
    let mut sp_b = Float64Builder::with_capacity(n);
    let mut mp_b = Float64Builder::with_capacity(n);
    let mut bv5_b = Float64Builder::with_capacity(n);
    let mut av5_b = Float64Builder::with_capacity(n);
    let mut bv10_b = Float64Builder::with_capacity(n);
    let mut av10_b = Float64Builder::with_capacity(n);
    let mut bv_b = Float64Builder::with_capacity(n);
    let mut av_b = Float64Builder::with_capacity(n);
    let mut imb_b = Float64Builder::with_capacity(n);
    let mut up_b = Float64Builder::with_capacity(n);
    let mut dn_b = Float64Builder::with_capacity(n);
    let mut btc_b = Float64Builder::with_capacity(n);

    for snap in snapshots {
        session_id_b.append_value(snap.session_id);
        ts_b.append_value(snap.ts.timestamp_micros());
        side_b.append_value(&snap.side);
        bb_b.append_option(snap.best_bid);
        bbs_b.append_option(snap.best_bid_sz);
        ba_b.append_option(snap.best_ask);
        bas_b.append_option(snap.best_ask_sz);
        sp_b.append_option(snap.spread);
        mp_b.append_option(snap.mid_price);
        bv5_b.append_option(snap.bid_volume_5);
        av5_b.append_option(snap.ask_volume_5);
        bv10_b.append_option(snap.bid_volume_10);
        av10_b.append_option(snap.ask_volume_10);
        bv_b.append_option(snap.bid_volume);
        av_b.append_option(snap.ask_volume);
        imb_b.append_option(snap.imbalance_ratio);
        up_b.append_option(snap.up_probability);
        dn_b.append_option(snap.down_probability);
        btc_b.append_option(snap.btc_price);
    }

    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(session_id_b.finish()),
            Arc::new(ts_b.finish()),
            Arc::new(side_b.finish()),
            Arc::new(bb_b.finish()),
            Arc::new(bbs_b.finish()),
            Arc::new(ba_b.finish()),
            Arc::new(bas_b.finish()),
            Arc::new(sp_b.finish()),
            Arc::new(mp_b.finish()),
            Arc::new(bv5_b.finish()),
            Arc::new(av5_b.finish()),
            Arc::new(bv10_b.finish()),
            Arc::new(av10_b.finish()),
            Arc::new(bv_b.finish()),
            Arc::new(av_b.finish()),
            Arc::new(imb_b.finish()),
            Arc::new(up_b.finish()),
            Arc::new(dn_b.finish()),
            Arc::new(btc_b.finish()),
        ],
    )?;

    let mut buf = Vec::new();
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(&mut buf, Arc::clone(&schema), Some(props))?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(buf)
}

#[derive(Deserialize)]
struct ExportQuery {
    format: Option<String>,  // "csv" | "json"
}
