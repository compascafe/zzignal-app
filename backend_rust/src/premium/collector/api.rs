use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    routing::get,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::models::state::AppState;
use super::repository;

/// Router para endpoints del Collector.
/// Solo se incluye cuando el feature `premium-collector` está activo.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/premium/collector/candles", get(list_candles))
        .route("/api/premium/collector/stats",   get(stats))
        .with_state(state)
}

#[derive(Deserialize)]
struct CandleQuery {
    interval: Option<String>,  // default: "5m"
    side:     Option<String>,  // "up" | "down" | None (ambos)
    limit:    Option<i64>,     // default: 100
    from:     Option<DateTime<Utc>>,
    to:       Option<DateTime<Utc>>,
}

async fn list_candles(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CandleQuery>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };

    let interval = q.interval.as_deref().unwrap_or("5m");
    let limit    = q.limit.unwrap_or(100).min(2000);

    match repository::query_candles(pool, interval, q.side.as_deref(), limit, q.from, q.to).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn stats(State(s): State<Arc<AppState>>) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };

    match repository::count_by_interval(pool).await {
        Ok(counts) => Json(json!({
            "total_candles": counts.iter().map(|(_, c)| c).sum::<i64>(),
            "by_interval": counts,
        })),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}
