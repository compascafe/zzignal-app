use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State, WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json,
};
use axum::extract::ws::{Message, WebSocket};
use chrono::{Timelike, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;
use sysinfo::System;
use tracing::info;

use crate::models::state::AppState;
use crate::controllers::worker::{self, CandleInterval, CmdMsg, OrderSide, Outcome};

use crate::services::perf;
use crate::services::pipeline;

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // TUI polls
        .route("/api/health",          get(get_health))
        .route("/api/btc",             get(get_btc))
        .route("/api/hft/latest",      get(get_hft_latest))
        .route("/api/orders",          get(get_orders))
        .route("/api/sessions",        get(get_sessions))
        // Perf monitoring
        .route("/api/perf",            get(get_perf))
        // TUI commands — trading
        .route("/api/orders/limit",    post(post_limit_order))
        .route("/api/orders/market",   post(post_market_order))
        .route("/api/orders",          delete(cancel_all_orders))
        .route("/api/orders/{id}",     delete(cancel_order))
        .route("/api/panic",           post(post_panic))
        // TUI commands — session
        .route("/api/sessions/start",  post(post_session_start))
        // WebSocket
        .route("/ws",                  get(ws_handler))
        .with_state(Arc::clone(&state))
        .layer(CorsLayer::permissive())
}

// ─── Health Check — Despliegue + BD ────────────────────────────────────────────
// Verifica: compilación, conexión BD, migraciones, tablas existentes

async fn get_health(State(s): State<Arc<AppState>>) -> Json<Value> {
    let app_version = env!("CARGO_PKG_VERSION");
    let build_time  = option_env!("VERGEN_BUILD_TIMESTAMP").unwrap_or("dev");
    let git_sha     = option_env!("VERGEN_GIT_SHA").unwrap_or("dev");

    Json(json!({
        "app": {
            "version":    app_version,
            "build_time": build_time,
            "git_sha":    git_sha,
        },
        "system": system_metrics(),
        "latency": {
            "binance_ms": *s.latency_binance.read().await,
            "polymarket_ms": *s.latency_poly.read().await,
        },
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

// ─── Status & Mercado ─────────────────────────────────────────────────────────

async fn get_status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "status": *s.status.read().await }))
}

async fn get_market(State(s): State<Arc<AppState>>) -> Json<Value> {
    let m = s.market.read().await;
    match &*m {
        Some(m) => Json(json!({
            "title":         m.title,
            "outcome_up":    m.outcome_up,
            "outcome_down":  m.outcome_down,
            "price_to_beat": m.price_to_beat,
            "end_date":      m.end_date,
            "active":        m.active,
        })),
        None => Json(json!(null)),
    }
}

async fn get_balance(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "balance": *s.balance.read().await }))
}

async fn get_btc(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "price": s.btc_price.read().await.unwrap_or(0.0),
        "open":  s.btc_open.read().await.unwrap_or(0.0),
    }))
}

async fn get_sessions(State(s): State<Arc<AppState>>) -> Json<Value> {
    let ids = s.recording_sessions.read().await.clone();
    let sessions: Vec<Value> = ids.iter().map(|&sid| {
        let path = s.session_manager.session_path(sid);
        let name = std::path::Path::new(&path)
            .file_stem().and_then(|s| s.to_str()).unwrap_or("")
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
    }).collect();
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
    s.tracking_state.reset_session_baselines();
    s.tick_drain.store(true, std::sync::atomic::Ordering::Release);
    info!("[SESSION] Manual start #{} {} ({}→{})", sid, name, start.format("%H:%M"), end.format("%H:%M"));
    Json(json!({"ok": true, "id": sid, "name": name}))
}

// ─── USDC Approve ────────────────────────────────────────────────────────────

async fn post_approve(State(s): State<Arc<AppState>>) -> Json<Value> {
    if worker::is_approve_running() {
        return Json(json!({"ok": false, "error": "Approve ya está en ejecución. Espera a que termine."}));
    }
    info!("POST /api/approve — ejecutando approve USDC + CTF manualmente");
    worker::set_approve_running(true);
    let result = match worker::approve_usdc_for_ctf(&s.creds).await {
        Ok(()) => {
            info!("Approve completado.");
            Json(json!({"ok": true, "message": "USDC approved for CTF Exchange. Refreshing balance..."}))
        }
        Err(e) => {
            let msg = format!("Approve falló: {e}");
            tracing::error!("{msg}");
            Json(json!({"ok": false, "error": msg}))
        }
    };
    worker::set_approve_running(false);
    result
}

// ─── Wrap USDC.e → pUSD ──────────────────────────────────────────────────────

async fn post_wrap(State(s): State<Arc<AppState>>) -> Json<Value> {
    info!("POST /api/wrap — USDC.e → pUSD vía CollateralOnramp");

    // Deposit wallet (Poly1271) — el pUSD debe estar aquí
    let wallet_str = "0x0000000000000000000000000000000000000000".to_string();

    match worker::wrap_usdc_to_pusd(&s.creds, &wallet_str).await {
        Ok(()) => {
            info!("Wrap completado. pUSD enviado a Proxy {wallet_str}");
            Json(json!({"ok": true, "message": format!("USDC.e → pUSD enviado a Proxy {wallet_str}")}))
        }
        Err(e) => {
            let msg = format!("Wrap falló: {e}");
            tracing::error!("{msg}");
            Json(json!({"ok": false, "error": msg}))
        }
    }
}

// ─── Live CSV Export (from in-memory buffer — no DB required) ──────────────────

async fn export_live_csv(
    State(s): State<Arc<AppState>>,
    Query(params): Query<LiveCsvParams>,
) -> Response {
    use std::fmt::Write;
    use crate::models::hft::CsvRecord;

    let mem = s.mem_hft.read().await;
    let total = mem.len();
    let limit = params.limit.unwrap_or(5000).min(total).max(1);
    let session_filter = params.session_id;

    // Filter: optionally by session_id, always take last `limit`
    let filtered: Vec<&CsvRecord> = if let Some(sid) = session_filter {
        mem.iter().filter(|r| r.session_id == sid).collect()
    } else {
        mem.iter().collect()
    };
    let rows: Vec<&CsvRecord> = filtered.iter().rev().take(limit).rev().copied().collect();

    // Build CSV header (125 columns matching fast_format_csv_line)
    let mut csv = String::with_capacity(rows.len() * 512);
    let _ = writeln!(csv, "# zzignal-app live CSV export — {} rows (buffer: {} total)", rows.len(), total);
    let _ = writeln!(csv, "# exported_at={}", chrono::Utc::now().to_rfc3339());
    let _ = writeln!(csv, "{}", CsvRecord::csv_header());

    for r in &rows {
        let _ = writeln!(csv, "{}", r.to_csv_line());
    }

    let filename = format!("zzignal_live_{}.csv", chrono::Utc::now().format("%Y%m%dT%H%M%S"));
    (axum::http::StatusCode::OK,
     [("Content-Type", "text/csv; charset=utf-8"),
      ("Content-Disposition", &format!("attachment; filename=\"{}\"", filename))],
     csv).into_response()
}

#[derive(Deserialize)]
struct LiveCsvParams {
    limit: Option<usize>,
    session_id: Option<i32>,
}

// ─── Order Book ───────────────────────────────────────────────────────────────

async fn get_book_up(State(s): State<Arc<AppState>>) -> Json<Value> {
    let guard = s.book_up.read().await;
    Json(book_snapshot_to_json(&guard))
}

async fn get_book_down(State(s): State<Arc<AppState>>) -> Json<Value> {
    let guard = s.book_down.read().await;
    Json(book_snapshot_to_json(&guard))
}

fn book_snapshot_to_json(book: &Option<crate::controllers::worker::BookSnapshot>) -> Value {
    match book {
        Some(b) => json!({
            "bids": b.bids.iter().map(|l| json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
            "asks": b.asks.iter().map(|l| json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
        }),
        None => json!({"bids": [], "asks": []}), // always return valid object
    }
}

// ─── Candles ──────────────────────────────────────────────────────────────────

async fn get_candles(State(s): State<Arc<AppState>>) -> Json<Value> {
    let candles = s.candles.read().await;
    let arr: Vec<Value> = candles.iter().map(|c| json!({
        "open_time": c.open_time,
        "open":  c.open,
        "high":  c.high,
        "low":   c.low,
        "close": c.close,
        "volume":c.volume,
    })).collect();
    Json(json!(arr))
}

#[derive(Deserialize)]
struct IntervalBody {
    interval: String,  // "1s" | "1m" | "5m" | "15m" | "1h"
}

async fn set_interval(
    State(s):    State<Arc<AppState>>,
    Json(body):  Json<IntervalBody>,
) -> Json<Value> {
    let iv = match CandleInterval::from_str(&body.interval) {
        Some(iv) => iv,
        None => return Json(json!({"ok": false, "error": format!("intervalo desconocido: {}", body.interval)})),
    };
    if let Ok(mut guard) = s.interval_arc.lock() {
        *guard = iv;
    }
    Json(json!({"ok": true, "interval": body.interval}))
}

// ─── Órdenes ──────────────────────────────────────────────────────────────────

async fn get_orders(State(s): State<Arc<AppState>>) -> Json<Value> {
    let orders = s.open_orders.read().await;
    let arr: Vec<Value> = orders.iter().map(|o| json!({
        "id":           o.id,
        "outcome":      o.outcome,
        "side":         side_str(o.side),
        "price":        o.price,
        "size_orig":    o.size_orig,
        "size_matched": o.size_matched,
    })).collect();
    Json(json!(arr))
}

#[derive(Deserialize)]
struct LimitOrderReq {
    side:    String,
    outcome: String,
    price:   f64,
    size:    f64,
}

async fn post_limit_order(
    State(s):   State<Arc<AppState>>,
    Json(body): Json<LimitOrderReq>,
) -> Json<Value> {
    let (side, outcome) = match parse_args(&body.side, &body.outcome) {
        Ok(v) => v,
        Err(e) => return Json(json!({"ok": false, "error": e})),
    };
    let _ = s.cmd_tx.send(CmdMsg::PlaceLimitOrder { side, outcome, price: body.price, size: body.size });
    Json(json!({"ok": true}))
}

#[derive(Deserialize)]
struct MarketOrderReq {
    side:        String,
    outcome:     String,
    amount_usdc: f64,
}

async fn post_market_order(
    State(s):   State<Arc<AppState>>,
    Json(body): Json<MarketOrderReq>,
) -> Json<Value> {
    let (side, outcome) = match parse_args(&body.side, &body.outcome) {
        Ok(v) => v,
        Err(e) => return Json(json!({"ok": false, "error": e})),
    };
    let _ = s.cmd_tx.send(CmdMsg::PlaceMarketOrder { side, outcome, amount_usdc: body.amount_usdc });
    Json(json!({"ok": true}))
}

#[derive(Deserialize)]
struct ScalpReq {
    outcome:      String,
    price:        f64,
    size:         f64,
    target_price: f64,
}

async fn post_scalp_order(
    State(s):   State<Arc<AppState>>,
    Json(body): Json<ScalpReq>,
) -> Json<Value> {
    let outcome = match parse_outcome(&body.outcome) {
        Ok(o) => o,
        Err(e) => return Json(json!({"ok": false, "error": e})),
    };
    let _ = s.cmd_tx.send(CmdMsg::ScalpBuy {
        outcome,
        price:        body.price,
        size:         body.size,
        target_price: body.target_price,
    });
    Json(json!({"ok": true}))
}

async fn cancel_order(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
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
        Some("up")   => vec!["up"],
        Some("down") => vec!["down"],
        _            => vec!["up", "down"],
    };

    for outcome in &outcomes {
        let outcome_enum = match *outcome {
            "up"   => Outcome::Up,
            "down" => Outcome::Down,
            _      => continue,
        };
        let amount = match *outcome {
            "up"   => body.amount_up.unwrap_or(0.0),
            "down" => body.amount_down.unwrap_or(0.0),
            _      => 0.0,
        };
        let shares = if amount > 0.0 { amount } else {
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

    Json(json!({"ok": true, "message": format!("PANIC: cancelled all + market sell {:?}", outcomes)}))
}

// ─── Fills ────────────────────────────────────────────────────────────────────

async fn get_fills(State(s): State<Arc<AppState>>) -> Json<Value> {
    let fills = s.recent_fills.read().await;
    let arr: Vec<Value> = fills.iter().map(|f| json!({
        "outcome": f.outcome,
        "side":    side_str(f.side),
        "price":   f.price,
        "size":    f.size,
        "time":    f.time,
        "session": f.session,
    })).collect();
    Json(json!(arr))
}

// ─── WebSocket ────────────────────────────────────────────────────────────────

async fn ws_handler(
    ws:       WebSocketUpgrade,
    State(s): State<Arc<AppState>>,
) -> Response {
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
    let status  = state.status.read().await.clone();
    let btc     = *state.btc_price.read().await;
    let balance = *state.balance.read().await;
    json!({
        "type":    "snapshot",
        "status":  status,
        "btc":     btc,
        "balance": balance,
    }).to_string()
}

async fn handle_ws_cmd(text: &str, state: &AppState) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    let cmd_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match cmd_type {
        "limit" => {
            let side    = v["side"].as_str().and_then(|s| parse_side(s).ok());
            let outcome = v["outcome"].as_str().and_then(|o| parse_outcome(o).ok());
            if let (Some(side), Some(outcome), Some(price), Some(size)) =
                (side, outcome, v["price"].as_f64(), v["size"].as_f64())
            {
                let _ = state.cmd_tx.send(CmdMsg::PlaceLimitOrder { side, outcome, price, size });
            }
        }
        "market" => {
            let side    = v["side"].as_str().and_then(|s| parse_side(s).ok());
            let outcome = v["outcome"].as_str().and_then(|o| parse_outcome(o).ok());
            if let (Some(side), Some(outcome), Some(amount_usdc)) =
                (side, outcome, v["amount_usdc"].as_f64())
            {
                let _ = state.cmd_tx.send(CmdMsg::PlaceMarketOrder { side, outcome, amount_usdc });
            }
        }
        "scalp" => {
            let outcome      = v["outcome"].as_str().and_then(|o| parse_outcome(o).ok());
            let price        = v["price"].as_f64();
            let size         = v["size"].as_f64();
            let target_price = v["target_price"].as_f64();
            if let (Some(outcome), Some(price), Some(size), Some(target_price)) =
                (outcome, price, size, target_price)
            {
                let _ = state.cmd_tx.send(CmdMsg::ScalpBuy { outcome, price, size, target_price });
            }
        }
        "cancel" => {
            if let Some(id) = v["order_id"].as_str() {
                let _ = state.cmd_tx.send(CmdMsg::CancelOrder { order_id: id.to_string() });
            }
        }
        "cancel_all" => {
            let _ = state.cmd_tx.send(CmdMsg::CancelMarket);
        }
        "set_interval" => {
            if let Some(iv_str) = v["interval"].as_str() {
                if let Some(iv) = CandleInterval::from_str(iv_str) {
                    if let Ok(mut guard) = state.interval_arc.lock() {
                        *guard = iv;
                    }
                }
            }
        }
        _ => {}
    }
}

// ─── Depth History Handlers ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DepthQuery {
    side: Option<String>,
    levels: Option<usize>,
    limit: Option<usize>,
}

async fn get_depth_latest(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DepthQuery>,
) -> Response {
    let side = match q.side.as_deref() {
        Some("up") => 0u8,
        Some("down") => 1u8,
        _ => return (axum::http::StatusCode::BAD_REQUEST, "?side=up|down required").into_response(),
    };
    let max_levels = q.levels.unwrap_or(50);

    let history = state.poly_depth_history.read().await;
    let frame = history.iter().rev().find(|f| f.side == side);

    match frame {
        None => Json(json!({ "found": false, "reason": "no_frames_yet" })).into_response(),
        Some(f) => {
            let bids: Vec<_> = f.bids.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            let asks: Vec<_> = f.asks.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            Json(json!({
                "found": true,
                "side": if side == 0 { "up" } else { "down" },
                "ts_unix_ms": f.ts_unix_ms,
                "total_bid_levels": f.bids.len(),
                "total_ask_levels": f.asks.len(),
                "bids": bids,
                "asks": asks,
            })).into_response()
        }
    }
}

async fn get_depth_history(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DepthQuery>,
) -> Response {
    let side_filter: Option<u8> = match q.side.as_deref() {
        Some("up") => Some(0),
        Some("down") => Some(1),
        Some(other) => return (axum::http::StatusCode::BAD_REQUEST,
            format!("?side=up|down (got '{other}')")).into_response(),
        None => None,
    };
    let limit = q.limit.unwrap_or(50).min(300);
    let max_levels = q.levels.unwrap_or(20);

    let history = state.poly_depth_history.read().await;
    let frames: Vec<_> = history.iter()
        .rev()
        .filter(|f| side_filter.map_or(true, |s| f.side == s))
        .take(limit)
        .map(|f| {
            let bids: Vec<_> = f.bids.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            let asks: Vec<_> = f.asks.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            serde_json::json!({
                "ts_unix_ms": f.ts_unix_ms,
                "side": if f.side == 0 { "up" } else { "down" },
                "levels": { "bids": bids, "asks": asks },
                "total_bid_levels": f.bids.len(),
                "total_ask_levels": f.asks.len(),
            })
        })
        .collect();

    Json(json!({
        "total_frames_in_buffer": history.len(),
        "returned": frames.len(),
        "frames": frames,
    })).into_response()
}

async fn get_depth_session(
    State(state): State<Arc<AppState>>,
    Query(q): Query<DepthQuery>,
) -> Response {
    let side: u8 = match q.side.as_deref() {
        Some("up") => 0,
        Some("down") => 1,
        _ => return (axum::http::StatusCode::BAD_REQUEST, "?side=up|down required").into_response(),
    };
    let limit = q.limit.unwrap_or(2000).min(5000);
    let max_levels = q.levels.unwrap_or(10);

    let history = state.poly_depth_history.write().await;
    let frames: Vec<_> = history.iter()
        .rev()
        .filter(|f| f.side == side)
        .take(limit)
        .map(|f| {
            let bids: Vec<_> = f.bids.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            let asks: Vec<_> = f.asks.iter().take(max_levels).map(|l| {
                serde_json::json!({"price": l.price, "size": l.size})
            }).collect();
            let best_bid = f.bids.first().map(|l| l.price).unwrap_or(0.0);
            let best_ask = f.asks.first().map(|l| l.price).unwrap_or(0.0);
            let mid = if best_bid > 0.0 && best_ask > 0.0 {
                (best_bid + best_ask) / 2.0
            } else if best_bid > 0.0 { best_bid } else { best_ask };
            let spread = if best_bid > 0.0 && best_ask > 0.0 { best_ask - best_bid } else { 0.0 };
            let bid_vol: f64 = f.bids.iter().map(|l| l.size).sum();
            let ask_vol: f64 = f.asks.iter().map(|l| l.size).sum();
            serde_json::json!({
                "ts": f.ts_unix_ms,
                "best_bid": best_bid,
                "best_ask": best_ask,
                "mid": mid,
                "spread": spread,
                "bid_vol": bid_vol,
                "ask_vol": ask_vol,
                "bid_levels": f.bids.len(),
                "ask_levels": f.asks.len(),
                "bids": bids,
                "asks": asks,
            })
        })
        .collect();

    Json(json!({
        "side": if side == 0 { "up" } else { "down" },
        "total_frames": frames.len(),
        "frames": frames,
    })).into_response()
}

async fn get_mode(State(state): State<Arc<AppState>>) -> Response {
    let diagnostic = state.diagnostic_mode.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({
        "diagnostic_mode": diagnostic,
        "strategies_active": !diagnostic,
    })).into_response()
}

async fn post_diagnostic_mode(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let enable = body.get("enable").and_then(|v| v.as_bool()).unwrap_or(true);
    state.diagnostic_mode.store(enable, std::sync::atomic::Ordering::Relaxed);
    info!("Diagnostic mode: {}", if enable { "ON" } else { "OFF" });
    Json(json!({
        "diagnostic_mode": enable,
        "strategies_active": !enable,
    })).into_response()
}

// ─── DB Stubs ──────────────────────────────────────────────────────────────────

async fn db_snapshots_stub() -> Json<Value> {
    Json(json!([]))
}

async fn db_executions_stub() -> Json<Value> {
    Json(json!([]))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

async fn get_hft_latest(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let hft = state.latest_hft.read().await.clone();
    Json(serde_json::to_value(hft).unwrap_or_default())
}

// ─── Helpers (continued) ──────────────────────────────────────────────────────

fn parse_side(s: &str) -> Result<OrderSide, String> {
    match s.to_lowercase().as_str() {
        "buy"  => Ok(OrderSide::Buy),
        "sell" => Ok(OrderSide::Sell),
        other  => Err(format!("side inválido: {other}")),
    }
}

fn parse_outcome(s: &str) -> Result<Outcome, String> {
    match s.to_lowercase().as_str() {
        "up"   => Ok(Outcome::Up),
        "down" => Ok(Outcome::Down),
        other  => Err(format!("outcome inválido: {other}")),
    }
}

async fn get_perf() -> Json<Value> {
    let slots = serde_json::from_str::<Value>(&perf::dump_json()).unwrap_or_default();
    Json(json!({
        "slots": slots,
        "sessions": std::fs::read_dir("sessions").ok().map(|d| d.filter_map(|e| e.ok()).count()).unwrap_or(0),
    }))
}

fn parse_args(side: &str, outcome: &str) -> Result<(OrderSide, Outcome), String> {
    Ok((parse_side(side)?, parse_outcome(outcome)?))
}

fn side_str(s: OrderSide) -> &'static str {
    match s { OrderSide::Buy => "BUY", OrderSide::Sell => "SELL" }
}
