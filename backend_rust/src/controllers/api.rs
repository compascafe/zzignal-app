use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Path, State, WebSocketUpgrade},
    response::Response,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Timelike, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sysinfo::System;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::controllers::worker::{CmdMsg, OrderSide, Outcome};
use crate::models::state::AppState;
use crate::services::perf;

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // TUI polls
        .route("/api/health", get(get_health))
        .route("/api/btc", get(get_btc))
        .route("/api/hft/latest", get(get_hft_latest))
        .route("/api/orders", get(get_orders))
        .route("/api/sessions", get(get_sessions))
        // Perf monitoring
        .route("/api/perf", get(get_perf))
        // TUI commands — trading
        .route("/api/orders/limit", post(post_limit_order))
        .route("/api/orders/market", post(post_market_order))
        .route("/api/orders", delete(cancel_all_orders))
        .route("/api/orders/{id}", delete(cancel_order))
        .route("/api/panic", post(post_panic))
        // TUI commands — session
        .route("/api/sessions/start", post(post_session_start))
        // WebSocket
        .route("/ws", get(ws_handler))
        .with_state(Arc::clone(&state))
        .layer(CorsLayer::permissive())
}

// ─── Health & System ──────────────────────────────────────────────────────────

async fn get_health(State(s): State<Arc<AppState>>) -> Json<Value> {
    let app_version = env!("CARGO_PKG_VERSION");
    let build_time = env!("BUILD_TIME");
    let git_sha = env!("GIT_VERSION");

    Json(json!({
        "app": {
            "version":    app_version,
            "build_time": build_time,
            "git_sha":    git_sha,
        },
        "system": system_metrics(),
        "status": *s.status.read().await,
    }))
}

fn system_metrics() -> Value {
    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu = sys.cpus().first().map(|c| c.cpu_usage()).unwrap_or(0.0);
    let ram_used = sys.used_memory() / 1024 / 1024;
    let ram_total = sys.total_memory() / 1024 / 1024;
    json!({
        "cpu_percent":   (cpu as f64 * 10.0).round() / 10.0,
        "ram_mb_used":   ram_used,
        "ram_mb_total":  ram_total,
        "ram_pct":       if ram_total > 0 { (ram_used as f64 / ram_total as f64 * 100.0 * 10.0).round() / 10.0 } else { 0.0 },
    })
}

// ─── BTC & Sessions ───────────────────────────────────────────────────────────

async fn get_btc(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "price": s.btc_price.read().await.unwrap_or(0.0),
        "open":  s.btc_open.read().await.unwrap_or(0.0),
    }))
}

async fn get_sessions(State(s): State<Arc<AppState>>) -> Json<Value> {
    let ids = s.recording_sessions.read().await.clone();
    let sessions: Vec<Value> = ids
        .iter()
        .map(|&sid| {
            let path = s.session_manager.session_path(sid);
            let name = std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            json!({
                "id": sid,
                "name": name,
                "status": "recording",
                "scheduled_start": "",
                "scheduled_end": "",
                "duration_min": 15,
                "tick_count": s.session_manager.tick_count(sid),
                "trade_count": s.session_manager.trade_count(sid),
            })
        })
        .collect();
    Json(json!(sessions))
}

async fn post_session_start(State(s): State<Arc<AppState>>) -> Json<Value> {
    let now = Utc::now();
    let t = now.time();
    let secs_into = (t.minute() as i64 % 15) * 60 + t.second() as i64;
    let start = now - chrono::Duration::seconds(secs_into);
    let end = start + chrono::Duration::minutes(15);
    // Generate unique session ID
    let sid = now.timestamp() as i32;
    let name = format!("S{:04}-{}", sid, start.format("%H%M"));

    // Stop any existing recording first
    let mut rec = s.recording_sessions.write().await;
    for old in rec.drain(..) {
        s.session_manager.flush(old).ok();
        s.session_manager.stop_session(old).ok();
    }

    rec.push(sid);
    drop(rec);
    s.session_manager.start_session(sid, &name).ok();
    s.tick_drain
        .store(true, std::sync::atomic::Ordering::Release);
    info!(
        "[SESSION] Manual start #{} {} ({}→{})",
        sid,
        name,
        start.format("%H:%M"),
        end.format("%H:%M")
    );
    Json(json!({"ok": true, "id": sid, "name": name}))
}

// ─── Órdenes ──────────────────────────────────────────────────────────────────

async fn get_orders(State(s): State<Arc<AppState>>) -> Json<Value> {
    let orders = s.open_orders.read().await;
    let arr: Vec<Value> = orders
        .iter()
        .map(|o| {
            json!({
                "id":           o.id,
                "outcome":      o.outcome,
                "side":         side_str(o.side),
                "price":        o.price,
                "size_orig":    o.size_orig,
                "size_matched": o.size_matched,
            })
        })
        .collect();
    Json(json!(arr))
}

#[derive(Deserialize)]
struct LimitOrderReq {
    side: String,
    outcome: String,
    price: f64,
    size: f64,
}

async fn post_limit_order(
    State(s): State<Arc<AppState>>,
    Json(body): Json<LimitOrderReq>,
) -> Json<Value> {
    let (side, outcome) = match parse_args(&body.side, &body.outcome) {
        Ok(v) => v,
        Err(e) => return Json(json!({"ok": false, "error": e})),
    };
    let _ = s.cmd_tx.send(CmdMsg::PlaceLimitOrder {
        side,
        outcome,
        price: body.price,
        size: body.size,
    });
    Json(json!({"ok": true}))
}

#[derive(Deserialize)]
struct MarketOrderReq {
    side: String,
    outcome: String,
    amount_usdc: f64,
}

async fn post_market_order(
    State(s): State<Arc<AppState>>,
    Json(body): Json<MarketOrderReq>,
) -> Json<Value> {
    let (side, outcome) = match parse_args(&body.side, &body.outcome) {
        Ok(v) => v,
        Err(e) => return Json(json!({"ok": false, "error": e})),
    };
    let _ = s.cmd_tx.send(CmdMsg::PlaceMarketOrder {
        side,
        outcome,
        amount_usdc: body.amount_usdc,
    });
    Json(json!({"ok": true}))
}

async fn cancel_order(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let _ = s.cmd_tx.send(CmdMsg::CancelOrder { order_id: id });
    Json(json!({"ok": true}))
}

async fn cancel_all_orders(State(s): State<Arc<AppState>>) -> Json<Value> {
    let _ = s.cmd_tx.send(CmdMsg::CancelMarket);
    Json(json!({"ok": true}))
}

// ─── Panic — cancelar todo + market sell ──────────────────────────────────────

#[derive(Deserialize)]
struct PanicBody {
    outcome: Option<String>, // "up", "down", o ausente = ambos
    amount_up: Option<f64>,
    amount_down: Option<f64>,
}

async fn post_panic(State(s): State<Arc<AppState>>, Json(body): Json<PanicBody>) -> Json<Value> {
    info!("🚨 PANIC — cancel all + immediate market sell");

    // 1. Cancelar todas las órdenes abiertas (limit buys/sells pendientes)
    let _ = s.cmd_tx.send(CmdMsg::CancelMarket);

    // 2. Market sell inmediato para los outcomes especificados (o ambos)
    let outcomes: Vec<&str> = match body.outcome.as_deref() {
        Some("up") => vec!["up"],
        Some("down") => vec!["down"],
        _ => vec!["up", "down"],
    };

    for outcome in &outcomes {
        let outcome_enum = match *outcome {
            "up" => Outcome::Up,
            "down" => Outcome::Down,
            _ => continue,
        };
        let amount = match *outcome {
            "up" => body.amount_up.unwrap_or(0.0),
            "down" => body.amount_down.unwrap_or(0.0),
            _ => 0.0,
        };
        let shares = if amount > 0.0 {
            amount
        } else {
            // Fallback: sell a generous amount to close any remaining position
            // The monitor now sends individual market sells before PANIC,
            // so this fallback only triggers for direct /p usage.
            500.0
        };
        info!("  Market SELL {outcome} × {shares:.0} shares");
        let _ = s.cmd_tx.send(CmdMsg::PlaceMarketOrder {
            side: OrderSide::Sell,
            outcome: outcome_enum,
            amount_usdc: shares,
        });
    }

    Json(
        json!({"ok": true, "message": format!("PANIC: cancelled all + market sell {:?}", outcomes)}),
    )
}

// ─── HFT State ────────────────────────────────────────────────────────────────

async fn get_hft_latest(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let hft = state.latest_hft.read().await.clone();
    Json(serde_json::to_value(hft).unwrap_or_default())
}

// ─── WebSocket ────────────────────────────────────────────────────────────────

async fn ws_handler(ws: WebSocketUpgrade, State(s): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, s))
}

async fn handle_ws_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.broadcast_tx.subscribe();

    // Snapshot inicial al conectar
    let snap = build_snapshot(&state).await;
    if socket.send(Message::Text(snap.into())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            // Reenviar broadcasts al cliente
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // Recibir comandos del cliente
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => { handle_ws_cmd(&text, &state).await; }
                    Some(Ok(Message::Ping(d)))    => { let _ = socket.send(Message::Pong(d)).await; }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

async fn build_snapshot(state: &AppState) -> String {
    let status = state.status.read().await.clone();
    let btc = *state.btc_price.read().await;
    let balance = *state.balance.read().await;
    json!({
        "type":    "snapshot",
        "status":  status,
        "btc":     btc,
        "balance": balance,
    })
    .to_string()
}

async fn handle_ws_cmd(text: &str, state: &AppState) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let cmd_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match cmd_type {
        "limit" => {
            let side = v["side"].as_str().and_then(|s| parse_side(s).ok());
            let outcome = v["outcome"].as_str().and_then(|o| parse_outcome(o).ok());
            if let (Some(side), Some(outcome), Some(price), Some(size)) =
                (side, outcome, v["price"].as_f64(), v["size"].as_f64())
            {
                let _ = state.cmd_tx.send(CmdMsg::PlaceLimitOrder {
                    side,
                    outcome,
                    price,
                    size,
                });
            }
        }
        "market" => {
            let side = v["side"].as_str().and_then(|s| parse_side(s).ok());
            let outcome = v["outcome"].as_str().and_then(|o| parse_outcome(o).ok());
            if let (Some(side), Some(outcome), Some(amount_usdc)) =
                (side, outcome, v["amount_usdc"].as_f64())
            {
                let _ = state.cmd_tx.send(CmdMsg::PlaceMarketOrder {
                    side,
                    outcome,
                    amount_usdc,
                });
            }
        }
        "cancel" => {
            if let Some(id) = v["order_id"].as_str() {
                let _ = state.cmd_tx.send(CmdMsg::CancelOrder {
                    order_id: id.to_string(),
                });
            }
        }
        "cancel_all" => {
            let _ = state.cmd_tx.send(CmdMsg::CancelMarket);
        }
        _ => {}
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn parse_side(s: &str) -> Result<OrderSide, String> {
    match s.to_lowercase().as_str() {
        "buy" => Ok(OrderSide::Buy),
        "sell" => Ok(OrderSide::Sell),
        other => Err(format!("side inválido: {other}")),
    }
}

fn parse_outcome(s: &str) -> Result<Outcome, String> {
    match s.to_lowercase().as_str() {
        "up" => Ok(Outcome::Up),
        "down" => Ok(Outcome::Down),
        other => Err(format!("outcome inválido: {other}")),
    }
}

fn parse_args(side: &str, outcome: &str) -> Result<(OrderSide, Outcome), String> {
    Ok((parse_side(side)?, parse_outcome(outcome)?))
}

fn side_str(s: OrderSide) -> &'static str {
    match s {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
    }
}

async fn get_perf() -> Json<Value> {
    let slots = serde_json::from_str::<Value>(&perf::dump_json()).unwrap_or_default();
    Json(json!({
        "slots": slots,
        "sessions": std::fs::read_dir("sessions").ok().map(|d| d.filter_map(|e| e.ok()).count()).unwrap_or(0),
    }))
}
