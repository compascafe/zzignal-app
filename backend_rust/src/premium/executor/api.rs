use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::models::state::AppState;
use super::models::NewStrategy;
use super::repository;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/premium/strategies",         get(list_strategies))
        .route("/api/premium/strategies",         post(create_strategy))
        .route("/api/premium/strategies/{id}/toggle", post(toggle_strategy))
        .route("/api/premium/strategies/{id}",     delete(delete_strategy))
        .route("/api/premium/executions",          get(list_executions))
        .with_state(state)
}

async fn list_strategies(State(s): State<Arc<AppState>>) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::list_strategies(pool).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn create_strategy(
    State(s): State<Arc<AppState>>,
    Json(body): Json<NewStrategy>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::insert_strategy(pool, &body).await {
        Ok(id) => Json(json!({"ok": true, "id": id})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn toggle_strategy(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<ToggleBody>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::toggle_strategy(pool, id, body.enabled).await {
        Ok(_) => Json(json!({"ok": true})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn delete_strategy(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::delete_strategy(pool, id).await {
        Ok(_) => Json(json!({"ok": true})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct ToggleBody { enabled: bool }

#[derive(Deserialize)]
struct LogQuery { limit: Option<i64> }

async fn list_executions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<LogQuery>,
) -> Json<Value> {
    let Some(pool) = s.db.as_ref() else {
        return Json(json!({"error": "DB no disponible"}));
    };
    match repository::query_execution_logs(pool, q.limit.unwrap_or(50)).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}
