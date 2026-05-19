use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State},
    routing::{get, put},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::models::state::AppState;
use super::models::DetectorConfig;
use super::repository;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/premium/patterns/signals", get(list_signals))
        .route("/api/premium/patterns/config",  get(get_config))
        .route("/api/premium/patterns/config",  put(update_config))
        .with_state(state)
}

#[derive(Deserialize)]
struct SignalQuery {
    side:  Option<String>,
    limit: Option<i64>,
}

async fn list_signals(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SignalQuery>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::query_signals(pool, q.side.as_deref(), q.limit.unwrap_or(100)).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn get_config(State(s): State<Arc<AppState>>) -> Json<Value> {
    let config = s.patterns_config.read().await.clone();
    Json(json!(config))
}

async fn update_config(
    State(s): State<Arc<AppState>>,
    Json(config): Json<DetectorConfig>,
) -> Json<Value> {
    *s.patterns_config.write().await = config;
    Json(json!({"ok": true}))
}
