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
use sysinfo::System;
use tracing::info;

use crate::modules::core::persistence as db;
use crate::modules::core::state::AppState;
use crate::modules::core::worker::{self, BtcPriceProvider, CandleInterval, CmdMsg, OrderSide, Outcome};
use crate::modules::db::api as db_api;
use crate::modules::hft::perf;

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
        // USDC approve — activa el saldo en CLOB (requiere MATIC en wallet)
        .route("/api/approve",         post(post_approve))
        // Wrap USDC.e → pUSD vía CollateralOnramp (CLOB V2)
        .route("/api/wrap",            post(post_wrap))
        // Macro indicators (Adaptive Risk Engine)
        .route("/api/macro",           get(get_macro))
        // Wisdom & RL state
        .route("/api/wisdom",          get(get_wisdom))
        .route("/api/wisdom/save",     post(snapshot_wisdom))
        .route("/api/wisdom/list",     get(list_wisdom_snapshots))
        .route("/api/wisdom/export",   get(export_wisdom))
        .route("/api/wisdom/export-bulk",get(export_wisdom_bulk))
        .route("/api/wisdom/import",   post(import_wisdom))

        // ─── Wisdom v2: T-5 Certainty Strategy ─────────────────────────────
        .route("/api/wisdom2",         get(get_wisdom2))

        // ─── Wisdom v3: T-3 Aggressive Strategy ────────────────────────────
        .route("/api/wisdom3",         get(get_wisdom3))

        // ─── Wisdom v4: Hydra No Return ────────────────────────────────────
        .route("/api/wisdom4",         get(get_wisdom4))

        // ─── Insight Strategies: Cerbero + Fenix ───────────────────────────
        .route("/api/insights",        get(get_insights))

        // ─── Fenix Trading ─────────────────────────────────────────────────
        .route("/api/fenix",           get(get_fenix))
        // ─── Odiseo Trading ────────────────────────────────────────────────
        .route("/api/odiseo",          get(get_odiseo))
        .route("/api/odiseo/live",     post(post_odiseo_live))
        .route("/api/odiseo/variant",  post(post_odiseo_variant))
        .route("/api/odiseo/budget",   post(post_odiseo_budget))
        .route("/api/odiseo/reinvest", post(post_odiseo_reinvest))
        .route("/api/odiseo/max-sessions", post(post_odiseo_max_sessions))
        .route("/api/odiseo/filters", post(post_odiseo_filters))
        .route("/api/odiseo/trade-config", post(post_odiseo_trade_config))
        .route("/api/odiseo/status",   get(get_odiseo_status))
        // Order book
        .route("/api/book/up",         get(get_book_up))
        .route("/api/book/down",       get(get_book_down))
        // Depth history — orderbook completo en memoria (todos los niveles)
        .route("/api/depth/latest",    get(get_depth_latest))
        .route("/api/depth/history",   get(get_depth_history))
        .route("/api/depth/session",   get(get_depth_session))
        // Diagnostic mode toggle
        .route("/api/mode",            get(get_mode))
        .route("/api/mode/diagnostic", post(post_diagnostic_mode))
        // Live HFT triggers for TUI
        .route("/api/hft/latest",      get(get_hft_latest))
        // Candles (live desde estado en memoria)
        .route("/api/candles",         get(get_candles))
        .route("/api/candles/interval",post(set_interval))
        // Órdenes abiertas
        .route("/api/orders",          get(get_orders))
        .route("/api/orders",          delete(cancel_all_orders))
        .route("/api/orders/{id}",     delete(cancel_order))
        // Panic — cancel all + market sell
        .route("/api/panic",           post(post_panic))
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
        // Performance counters
        .route("/api/perf",            get(get_perf))
        .route("/api/perf/reset",      post(reset_perf))
        // Live CSV export (in-memory buffer, no DB required)
        .route("/api/csv/live",        get(export_live_csv))
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
    // Use simple df command for disk (cross-platform approach)
    let disk_free = std::process::Command::new("df")
        .args(["-k", "."])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.lines().nth(1).map(|l| l.to_string()))
        .and_then(|l| l.split_whitespace().nth(3).map(|s| s.to_string()))
        .and_then(|s| s.parse::<f64>().ok())
        .map(|kb| kb / 1_048_576.0)
        .unwrap_or(0.0);
    json!({
        "cpu_percent":   (cpu as f64 * 10.0).round() / 10.0,
        "ram_mb_used":   ram_used,
        "ram_mb_total":  ram_total,
        "disk_gb_free":  (disk_free * 10.0).round() / 10.0,
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

// ─── Adaptive Risk Engine: Macro Indicators ────────────────────────────────────

async fn get_macro(State(s): State<Arc<AppState>>) -> Json<Value> {
    let ctx = s.macro_ctx.read().await;
    Json(json!({
        "predicted_bias":   ctx.predicted_bias,
        "sma50":            ctx.sma200 * 0.99,
        "sma200":           ctx.sma200,
        "macro_slope":      ctx.macro_slope,
        "macd_line":        ctx.macd_hist,
        "macd_signal":      ctx.macd_hist * 0.8,
        "macd_hist":        ctx.macd_hist,
        "vfi":              ctx.vfi,
        "rsi14":            ctx.rsi14,
        "cp_quantile":      ctx.cp_quantile,
        "cp_alpha":         ctx.cp_alpha,
        "auto_widened":     ctx.auto_widened,
        "feedback_count":   ctx.feedback_count,
        "hunting_mode":     ctx.hunting_mode,
        "hunting_z_score":  ctx.hunting_z_score,
        "volatility_1h":    ctx.volatility_1h,
        "signal_priority":  ctx.signal_priority,
    }))
}

async fn get_wisdom(State(s): State<Arc<AppState>>) -> Json<Value> {
    let ctx = s.macro_ctx.read().await;
    Json(json!({
        "mode":               ctx.mode,
        "accuracy_24h":       ctx.accuracy_24h,
        "cp_confidence":      ctx.cp_confidence,
        "cp_quantile":        ctx.cp_quantile,
        "cp_alpha":           ctx.cp_alpha,
        "weight_sma":         ctx.weight_sma,
        "weight_vfi":         ctx.weight_vfi,
        "weight_macd":        ctx.weight_macd,
        "weight_rsi":         ctx.weight_rsi,
        "weight_bb":          ctx.weight_bb,
        "feedback_count":     ctx.feedback_count,
        "auto_widened":       ctx.auto_widened,
        "dynamic_rsi":        ctx.dynamic_rsi,
        "vfi_confidence":     ctx.vfi_confidence,
        "db_accuracy_factor": ctx.db_accuracy_factor,
        "hunting_mode":       ctx.hunting_mode,
        "hunting_z_score":    ctx.hunting_z_score,
        "volatility_1h":      ctx.volatility_1h,
        "signal_priority":    ctx.signal_priority,
    }))
}

async fn get_wisdom2(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.t5_manager.export_wisdom2();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_wisdom3(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.t3_manager.export_wisdom3();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_wisdom4(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.pnr_manager.export_json();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_insights(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.insight_manager.export_json();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_fenix(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.fenix_trading.export_json();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_odiseo(State(s): State<Arc<AppState>>) -> Json<Value> {
    let json_str = s.odiseo_trading.export_json();
    let value: Value = serde_json::from_str(&json_str).unwrap_or(json!({"error": "parse failed"}));
    Json(value)
}

async fn get_odiseo_status(State(s): State<Arc<AppState>>) -> Json<Value> {
    let live = s.odiseo_trading.live_mode.load(std::sync::atomic::Ordering::Relaxed);
    let stats = s.odiseo_trading.export_json();
    let mut stats_arr: Vec<Value> = serde_json::from_str(&stats).unwrap_or_default();
    // Inject per-variant enabled state
    for (i, v) in stats_arr.iter_mut().enumerate() {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("enabled".into(), json!(s.odiseo_trading.is_enabled(i)));
            obj.insert("budget".into(), json!(s.odiseo_trading.get_budget(i)));
            obj.insert("max_sessions".into(), json!(s.odiseo_trading.get_max_sessions(i)));
            obj.insert("sessions_done".into(), json!(s.odiseo_trading.get_sessions_done(i)));
        }
    }
    Json(json!({
        "live_mode": live,
        "reinvest": s.odiseo_trading.reinvest.load(std::sync::atomic::Ordering::Relaxed),
        "status": if live { "LIVE" } else { "PAPER" },
        "variants": stats_arr,
        "session_history": s.odiseo_trading.session_summaries(),
    }))
}

#[derive(Deserialize)]
struct OdiseoVariantBody { index: usize, enable: bool }

async fn post_odiseo_variant(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoVariantBody>) -> Json<Value> {
    s.odiseo_trading.set_variant(body.index, body.enable);
    Json(json!({"ok": true, "index": body.index, "enabled": s.odiseo_trading.is_enabled(body.index)}))
}

#[derive(Deserialize)]
struct OdiseoBudgetBody { index: usize, amount: f64 }

async fn post_odiseo_budget(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoBudgetBody>) -> Json<Value> {
    s.odiseo_trading.set_budget(body.index, body.amount);
    Json(json!({"ok": true, "index": body.index, "budget": s.odiseo_trading.get_budget(body.index)}))
}

#[derive(Deserialize)]
struct OdiseoReinvestBody { enable: bool }

async fn post_odiseo_reinvest(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoReinvestBody>) -> Json<Value> {
    s.odiseo_trading.set_reinvest(body.enable);
    Json(json!({"ok": true, "reinvest": body.enable}))
}

#[derive(Deserialize)]
struct OdiseoMaxSessionsBody { index: usize, max_sessions: u32 }

async fn post_odiseo_max_sessions(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoMaxSessionsBody>) -> Json<Value> {
    s.odiseo_trading.set_max_sessions(body.index, body.max_sessions);
    Json(json!({"ok": true, "index": body.index, "max_sessions": s.odiseo_trading.get_max_sessions(body.index)}))
}

#[derive(Deserialize)]
struct OdiseoLiveBody { enable: bool }

async fn post_odiseo_live(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoLiveBody>) -> Json<Value> {
    s.odiseo_trading.set_live_mode(body.enable);
    Json(json!({"ok": true, "live_mode": s.odiseo_trading.live_mode.load(std::sync::atomic::Ordering::Relaxed)}))
}

#[derive(Deserialize)]
struct OdiseoFiltersBody { all: Option<bool>, name: Option<String>, enable: Option<bool> }

async fn post_odiseo_filters(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoFiltersBody>) -> Json<Value> {
    if let Some(name) = &body.name {
        if body.enable.unwrap_or(true) {
            s.odiseo_trading.filter_chain.enable(name);
        } else {
            s.odiseo_trading.filter_chain.disable(name);
        }
    } else if body.all == Some(false) {
        s.odiseo_trading.filter_chain.disable_all();
    } else if body.all == Some(true) {
        s.odiseo_trading.filter_chain.enable_all();
    }
    Json(json!({
        "ok": true,
        "enabled_mask": s.odiseo_trading.filter_chain.enabled_mask(),
        "filters": s.odiseo_trading.filter_chain.list_filters(),
    }))
}

#[derive(Deserialize)]
struct OdiseoTradeConfigBody { min_vol: Option<f64>, window: Option<usize> }

async fn post_odiseo_trade_config(State(s): State<Arc<AppState>>, Json(body): Json<OdiseoTradeConfigBody>) -> Json<Value> {
    if let Some(v) = body.min_vol {
        *s.trade_min_vol.write().await = v.max(0.0);
    }
    if let Some(n) = body.window {
        *s.trade_window_n.write().await = n.max(1).min(50);
    }
    Json(json!({
        "ok": true,
        "trade_min_vol": *s.trade_min_vol.read().await,
        "trade_window_n": *s.trade_window_n.read().await,
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

// ─── Live CSV Export (from in-memory buffer — no DB required) ──────────────────

async fn export_live_csv(
    State(s): State<Arc<AppState>>,
    Query(params): Query<LiveCsvParams>,
) -> Response {
    use std::fmt::Write;
    use crate::modules::hft::types::{CsvRecord, EventType};

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
        let fields = r.to_csv_fields();
        let _ = writeln!(csv, "{}", fields.join(","));
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

// ─── Panic — cancelar todo + market sell ──────────────────────────────────────

#[derive(Deserialize)]
struct PanicBody {
    outcome: Option<String>, // "up", "down", o ausente = ambos
}

async fn post_panic(State(s): State<Arc<AppState>>, Json(body): Json<PanicBody>) -> Json<Value> {
    info!("🚨 PANIC — cancel all + market sell");

    // 1. Cancelar todas las órdenes
    let _ = s.cmd_tx.send(CmdMsg::CancelMarket);

    // 2. Market sell para el outcome especificado (o ambos)
    let outcomes: Vec<&str> = match body.outcome.as_deref() {
        Some("up")   => vec!["up"],
        Some("down") => vec!["down"],
        _            => vec!["up", "down"],
    };

    let bal = s.balance.read().await;
    let usdc = bal.unwrap_or(0.0);
    // For SELL: amount = number of shares/contracts (not USDC).
    // Estimate: assume average entry ~0.50, so contracts = USDC / 0.5, with buffer.
    let shares = if usdc > 0.0 { (usdc * 2.5).max(10.0) } else { 10.0 };

    for outcome in &outcomes {
        let outcome_enum = match *outcome {
            "up"   => Outcome::Up,
            "down" => Outcome::Down,
            _      => continue,
        };
        info!("  Market SELL {outcome} × {shares:.0} shares");
        let _ = s.cmd_tx.send(CmdMsg::PlaceMarketOrder {
            side: OrderSide::Sell,
            outcome: outcome_enum,
            amount_usdc: shares,
        });
    }

    Json(json!({"ok": true, "message": format!("PANIC executed: cancelled all + market sell {:?}", outcomes)}))
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

// ─── Performance Counters ────────────────────────────────────────────────────

async fn get_perf() -> Response {
    let body = perf::dump_json();
    (axum::http::StatusCode::OK,
     [("Content-Type", "application/json")],
     body).into_response()
}

async fn reset_perf() -> Json<Value> {
    perf::reset_all();
    Json(json!({"ok": true, "message": "Performance counters reset"}))
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

fn parse_args(side: &str, outcome: &str) -> Result<(OrderSide, Outcome), String> {
    Ok((parse_side(side)?, parse_outcome(outcome)?))
}

fn side_str(s: OrderSide) -> &'static str {
    match s { OrderSide::Buy => "BUY", OrderSide::Sell => "SELL" }
}
