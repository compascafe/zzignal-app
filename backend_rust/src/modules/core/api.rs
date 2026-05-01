use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State, WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json,
};
use axum::extract::ws::{Message, WebSocket};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::cors::CorsLayer;

use crate::modules::core::persistence as db;
use crate::modules::core::state::AppState;
use crate::modules::core::worker::{BtcPriceProvider, CandleInterval, CmdMsg, OrderSide, Outcome};
use crate::modules::db::api as db_api;

// ─── Router ───────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router {
    let core = Router::new()
        // Health check — despliegue + BD
        .route("/api/health",          get(get_health))
        // Status / mercado
        .route("/api/status",          get(get_status))
        .route("/api/market",          get(get_market))
        .route("/api/balance",         get(get_balance))
        .route("/api/btc",             get(get_btc))
        .route("/api/btc/provider",    get(get_btc_provider))
        .route("/api/btc/provider",    post(set_btc_provider))
        // Macro indicators (Adaptive Risk Engine)
        .route("/api/macro",           get(get_macro))
        // Wisdom & RL state
        .route("/api/wisdom",          get(get_wisdom))
        .route("/api/wisdom/save",     post(snapshot_wisdom))
        .route("/api/wisdom/list",     get(list_wisdom_snapshots))
        .route("/api/wisdom/export",   get(export_wisdom))
        .route("/api/wisdom/export-bulk",get(export_wisdom_bulk))
        .route("/api/wisdom/import",   post(import_wisdom))
        // Order book
        .route("/api/book/up",         get(get_book_up))
        .route("/api/book/down",       get(get_book_down))
        // Candles (live desde estado en memoria)
        .route("/api/candles",         get(get_candles))
        .route("/api/candles/interval",post(set_interval))
        // Órdenes abiertas
        .route("/api/orders",          get(get_orders))
        .route("/api/orders",          delete(cancel_all_orders))
        .route("/api/orders/{id}",     delete(cancel_order))
        // Colocar órdenes
        .route("/api/orders/limit",    post(post_limit_order))
        .route("/api/orders/market",   post(post_market_order))
        .route("/api/orders/scalp",    post(post_scalp_order))
        // Fills
        .route("/api/fills",           get(get_fills))
        // Análisis histórico (PostgreSQL)
        .route("/api/analysis/candles",get(analysis_candles))
        .route("/api/analysis/pnl",    get(analysis_pnl))
        .route("/api/analysis/fills",  get(analysis_fills))
        // WebSocket
        .route("/ws",                  get(ws_handler))
        .with_state(Arc::clone(&state));

    let db_r = db_api::router(Arc::clone(&state));
    let session_r = db_api::session_router(Arc::clone(&state)); // clone para no consumir state

    #[allow(unused_mut)]
    let mut app = core.merge(db_r).merge(session_r);

    // ─── Premium modules (conditional compilation) ───
    #[cfg(feature = "premium-collector")]
    {
        use crate::modules::premium::collector::api as collector_api;
        app = app.merge(collector_api::router(Arc::clone(&state)));
    }

    #[cfg(feature = "premium-patterns")]
    {
        use crate::modules::premium::patterns::api as patterns_api;
        app = app.merge(patterns_api::router(Arc::clone(&state)));
    }

    #[cfg(feature = "premium-executor")]
    {
        use crate::modules::premium::executor::api as executor_api;
        app = app.merge(executor_api::router(Arc::clone(&state)));
    }

    app.layer(CorsLayer::permissive())
}

// ─── Health Check — Despliegue + BD ────────────────────────────────────────────
// Verifica: compilación, conexión BD, migraciones, tablas existentes

async fn get_health(State(s): State<Arc<AppState>>) -> Json<Value> {
    let app_version   = env!("CARGO_PKG_VERSION");
    let build_time    = option_env!("VERGEN_BUILD_TIMESTAMP").unwrap_or("dev");
    let git_sha       = option_env!("VERGEN_GIT_SHA").unwrap_or("dev");
    let target        = option_env!("VERGEN_CARGO_TARGET_TRIPLE").unwrap_or("unknown");

    // Minimum required tables for full functionality
    let expected_tables: &[&str] = &[
        "btc_ticks",
        "candles",
        "fills",
        "hft_snapshots",
        "order_book_snapshots",
        "recording_sessions",
        "scheduled_executions",
        "session_snapshots",
        "session_trades",
    ];

    let (db_ok, db_error, mut tables, mut missing_tables) = match s.db.as_ref() {
        Some(pool) => {
            match sqlx::query("SELECT 1 AS ping").fetch_one(pool).await {
                Ok(_) => {
                    match sqlx::query_as::<_, (String,)>(r#"
                        SELECT table_name FROM information_schema.tables
                        WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
                        ORDER BY table_name
                    "#).fetch_all(pool).await {
                        Ok(rows) => {
                            let existing: Vec<String> = rows.into_iter().map(|(t,)| t).collect();
                            let missing: Vec<String> = expected_tables.iter()
                                .filter(|t| !existing.iter().any(|e| e == **t))
                                .map(|t| t.to_string())
                                .collect();
                            (true, None, existing, missing)
                        }
                        Err(e) => {
                            let msg = format!("error leyendo tablas: {}", e);
                            let missing: Vec<String> = expected_tables.iter().map(|t| t.to_string()).collect();
                            (false, Some(msg), vec![], missing)
                        }
                    }
                }
                Err(e) => {
                    let msg = format!("ping falló: {e}");
                    let missing: Vec<String> = expected_tables.iter().map(|t| t.to_string()).collect();
                    (false, Some(msg), vec![], missing)
                }
            }
        }
        None => {
            let missing: Vec<String> = expected_tables.iter().map(|t| t.to_string()).collect();
            (false, Some("DATABASE_URL no configurada".into()), vec![], missing)
        }
    };

    // Ensure tables/missing_tables are bound even if unreachable
    if tables.is_empty() && db_ok {
        tables = vec!["(empty)".into()];
    }
    if missing_tables.is_empty() && !db_ok {
        missing_tables = expected_tables.iter().map(|t| t.to_string()).collect();
    }

    Json(json!({
        "app": {
            "version":    app_version,
            "build_time": build_time,
            "git_sha":    git_sha,
            "target":     target,
        },
        "database": {
            "connected":      db_ok,
            "error":          db_error,
            "tables":         tables,
            "expected":       expected_tables,
            "missing":        missing_tables,
            "all_present":    missing_tables.is_empty(),
        },
        "status": *s.status.read().await,
    }))
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
        "price": *s.btc_price.read().await,
        "open":  *s.btc_open.read().await,
    }))
}

async fn get_btc_provider(State(s): State<Arc<AppState>>) -> Json<Value> {
    let provider = *s.btc_provider.read().await;
    Json(json!({ "provider": provider.as_str() }))
}

#[derive(Deserialize)]
struct ProviderBody {
    provider: String,
}

async fn set_btc_provider(
    State(s):   State<Arc<AppState>>,
    Json(body): Json<ProviderBody>,
) -> Json<Value> {
    let provider = match BtcPriceProvider::from_str(&body.provider) {
        Some(p) => p,
        None => return Json(json!({"ok": false, "error": format!("proveedor inválido: {}", body.provider)})),
    };

    // Actualizar estado
    *s.btc_provider.write().await = provider;

    // Notificar al worker vía watch channel
    let _ = s.btc_provider_tx.send(provider);

    // Broadcast a clientes WS
    let _ = s.broadcast_tx.send(
        json!({"type":"btc_provider","provider":provider.as_str()}).to_string()
    );

    Json(json!({"ok": true, "provider": provider.as_str()}))
}

// ─── Adaptive Risk Engine: Macro Indicators ────────────────────────────────────

async fn get_macro(State(s): State<Arc<AppState>>) -> Json<Value> {
    let eng = s.adaptive_engine.lock().await;
    let snap = &eng.macro_snap;
    Json(json!({
        "sma50":            snap.sma50,
        "sma200":           snap.sma200,
        "macro_slope":      snap.macro_slope,
        "macd_line":        snap.macd_line,
        "macd_signal":      snap.macd_signal,
        "macd_hist":        snap.macd_hist,
        "vfi":              snap.vfi,
        "rsi14":            snap.rsi14,
        "predicted_bias":   snap.predicted_bias,
        "cp_quantile":      eng.cp.quantile_price,
        "cp_alpha":         eng.cp.alpha,
        "accuracy_count":   eng.cp.accuracy_window.len(),
        "auto_widened":     eng.cp.auto_widened,
        "feedback_count":   eng.cp.feedback_count,
    }))
}

// ─── Wisdom & Reinforcement Learning State ─────────────────────────────────────

async fn get_wisdom(State(s): State<Arc<AppState>>) -> Json<Value> {
    let eng = s.adaptive_engine.lock().await;
    let ctx = s.macro_ctx.read().await;
    let accuracy_24h = if eng.cp.accuracy_window.is_empty() { 0.5 }
        else { eng.cp.accuracy_window.iter().filter(|&&b| b).count() as f64 / eng.cp.accuracy_window.len() as f64 };

    let mode = if accuracy_24h < 0.4 { "OBSERVATION" }
        else if eng.cp.auto_widened { "CALIBRATING" }
        else { "ACTIVE" };

    Json(json!({
        "mode":               mode,
        "accuracy_24h":       accuracy_24h,
        "cp_confidence":      eng.cp.confidence_level,
        "cp_quantile":        eng.cp.quantile_price,
        "cp_alpha":           eng.cp.alpha,
        "weight_sma":         eng.cp.weight_sma,
        "weight_vfi":         eng.cp.weight_vfi,
        "weight_macd":        eng.cp.weight_macd,
        "weight_rsi":         eng.cp.weight_rsi,
        "weight_bb":          eng.cp.weight_bb,
        "feedback_count":     eng.cp.feedback_count,
        "auto_widened":       eng.cp.auto_widened,
        "dynamic_rsi":        ctx.dynamic_rsi,
        "vfi_confidence":     ctx.vfi_confidence,
        "db_accuracy_factor": ctx.db_accuracy_factor,
    }))
}

async fn export_wisdom(State(s): State<Arc<AppState>>) -> Response {
    let eng = s.adaptive_engine.lock().await;
    let ctx = s.macro_ctx.read().await;
    let wisdom = json!({
        "version":           "1.0",
        "exported_at":       chrono::Utc::now().to_rfc3339(),
        "cp": {
            "confidence_level":  eng.cp.confidence_level,
            "quantile_macd":     eng.cp.quantile_macd,
            "quantile_rsi":      eng.cp.quantile_rsi,
            "quantile_price":    eng.cp.quantile_price,
            "alpha":             eng.cp.alpha,
            "feedback_count":    eng.cp.feedback_count,
        },
        "weights": {
            "sma":  eng.cp.weight_sma,
            "vfi":  eng.cp.weight_vfi,
            "macd": eng.cp.weight_macd,
            "rsi":  eng.cp.weight_rsi,
        },
        "context": {
            "dynamic_rsi":        ctx.dynamic_rsi,
            "vfi_confidence":     ctx.vfi_confidence,
            "db_accuracy_factor": ctx.db_accuracy_factor,
        },
    });
    let body = serde_json::to_string_pretty(&wisdom).unwrap_or_default();
    (axum::http::StatusCode::OK,
     [("Content-Type", "application/json"),
      ("Content-Disposition", "attachment; filename=\"wisdom_state.json\"")],
     body).into_response()
}

async fn snapshot_wisdom(State(s): State<Arc<AppState>>) -> Json<Value> {
    let wisdom = build_wisdom_json(&s).await;
    let dir = "wisdom";
    std::fs::create_dir_all(dir).ok();
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let path = format!("{}/wisdom_state_{}.json", dir, ts);
    let body = serde_json::to_string_pretty(&wisdom).unwrap_or_default();
    match std::fs::write(&path, &body) {
        Ok(_) => Json(json!({"ok": true, "path": path, "timestamp": ts})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn list_wisdom_snapshots() -> Json<Value> {
    let dir = "wisdom";
    let mut files: Vec<Value> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                let meta = entry.metadata().ok();
                files.push(json!({
                    "name": name,
                    "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    "modified": meta.and_then(|m| m.modified().ok())
                        .map(|t| {
                            let dt: chrono::DateTime<chrono::Utc> = t.into();
                            dt.to_rfc3339()
                        }).unwrap_or_default(),
                }));
            }
        }
    }
    files.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    Json(json!(files))
}

async fn export_wisdom_bulk() -> Response {
    use std::io::Write;
    let dir = "wisdom";
    let mut zip_buf = Vec::new();
    let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") { continue; }
            if let Ok(data) = std::fs::read(entry.path()) {
                if zip_writer.start_file(&name, options).is_err() { continue; }
                if zip_writer.write_all(&data).is_err() { continue; }
            }
        }
    }
    let _ = zip_writer.finish();
    (axum::http::StatusCode::OK,
     [("Content-Type", "application/zip"),
      ("Content-Disposition", "attachment; filename=\"wisdom_bulk.zip\"")],
     zip_buf).into_response()
}

async fn import_wisdom(State(s): State<Arc<AppState>>, Json(body): Json<Value>) -> Json<Value> {
    let mut eng = s.adaptive_engine.lock().await;
    match eng.import_wisdom(&body) {
        Ok(()) => Json(json!({"ok": true, "message": "Wisdom imported successfully"})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn build_wisdom_json(s: &AppState) -> Value {
    let eng = s.adaptive_engine.lock().await;
    let ctx = s.macro_ctx.read().await;
    json!({
        "version":           "1.0",
        "exported_at":       chrono::Utc::now().to_rfc3339(),
        "cp": {
            "confidence_level":  eng.cp.confidence_level,
            "quantile_macd":     eng.cp.quantile_macd,
            "quantile_rsi":      eng.cp.quantile_rsi,
            "quantile_price":    eng.cp.quantile_price,
            "alpha":             eng.cp.alpha,
            "feedback_count":    eng.cp.feedback_count,
        },
        "weights": {
            "sma":  eng.cp.weight_sma,
            "vfi":  eng.cp.weight_vfi,
            "macd": eng.cp.weight_macd,
            "rsi":  eng.cp.weight_rsi,
            "bb":   eng.cp.weight_bb,
        },
        "context": {
            "dynamic_rsi":        ctx.dynamic_rsi,
            "vfi_confidence":     ctx.vfi_confidence,
            "db_accuracy_factor": ctx.db_accuracy_factor,
        },
    })
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

fn book_snapshot_to_json(book: &Option<crate::modules::core::worker::BookSnapshot>) -> Value {
    match book {
        Some(b) => json!({
            "bids": b.bids.iter().map(|l| json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
            "asks": b.asks.iter().map(|l| json!({"price": l.price, "size": l.size})).collect::<Vec<_>>(),
        }),
        None => json!(null),
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

// ─── Análisis (PostgreSQL) ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CandleQuery {
    interval: Option<String>,
    limit:    Option<i64>,
    from:     Option<DateTime<Utc>>,
    to:       Option<DateTime<Utc>>,
}

async fn analysis_candles(
    State(s): State<Arc<AppState>>,
    Query(q): Query<CandleQuery>,
) -> Json<Value> {
    let interval = q.interval.as_deref().unwrap_or("1m");
    let limit    = q.limit.unwrap_or(500).min(5000);
    match db::query_candles(s.db.as_ref(), interval, limit, q.from, q.to).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

async fn analysis_pnl(State(s): State<Arc<AppState>>) -> Json<Value> {
    match db::query_pnl(s.db.as_ref()).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct FillQuery {
    limit: Option<i64>,
}

async fn analysis_fills(
    State(s): State<Arc<AppState>>,
    Query(q): Query<FillQuery>,
) -> Json<Value> {
    let limit = q.limit.unwrap_or(100).min(1000);
    match db::query_fills(s.db.as_ref(), limit).await {
        Ok(rows) => Json(json!(rows)),
        Err(e)   => Json(json!({"error": e.to_string()})),
    }
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
        "set_btc_provider" => {
            if let Some(p_str) = v["provider"].as_str() {
                if let Some(provider) = BtcPriceProvider::from_str(p_str) {
                    *state.btc_provider.write().await = provider;
                    let _ = state.btc_provider_tx.send(provider);
                    let _ = state.broadcast_tx.send(
                        json!({"type":"btc_provider","provider":provider.as_str()}).to_string()
                    );
                }
            }
        }
        _ => {}
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

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

fn parse_args(side: &str, outcome: &str) -> Result<(OrderSide, Outcome), String> {
    Ok((parse_side(side)?, parse_outcome(outcome)?))
}

fn side_str(s: OrderSide) -> &'static str {
    match s { OrderSide::Buy => "BUY", OrderSide::Sell => "SELL" }
}
