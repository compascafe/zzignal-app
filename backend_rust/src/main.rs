//! Polymarket BTC 15-min — Backend Service
//!
//! Pipeline HFT unificado: BOOK_UPDATE | TRADE | BINANCE_TICK → CSV

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::time::Duration;

mod modules;

use std::sync::{mpsc, Arc, Mutex};

use chrono::Utc;
use tokio::sync::{broadcast, mpsc as tokio_mpsc, RwLock};
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use crate::modules::core::worker::{AppMsg, BtcPriceProvider, CandleInterval, CmdMsg, ConnStatus};
use crate::modules::core::credentials::ClobCredentials;
use crate::modules::core::state::AppState;
use crate::modules::core::persistence as db;
use crate::modules::hft::types::{BinanceDepth, CsvRecord, EventType};
use crate::modules::hft::ring_buffer::PriceRingBuffer;
use crate::modules::hft::metrics::{self, TrackingState};
use crate::modules::hft::binance_depth::BinanceTickEvent;
use crate::modules::hft::adaptive_risk_engine::warmup_fetch_and_compute;
use crate::modules::hft::perf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] .env no cargado: {e}");
    }

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let creds = match ClobCredentials::from_env() {
        Ok(c) => { info!("Wallet: {}", c.display_address()); Arc::new(c) }
        Err(e) => { error!("Credenciales no disponibles: {:#}", e); return Ok(()); }
    };

    let db = match std::env::var("DATABASE_URL") {
        Ok(url) => match sqlx::PgPool::connect(&url).await {
            Ok(pool) => {
                if let Err(e) = db::run_migrations(&pool).await {
                    tracing::warn!("Migraciones DB fallaron: {e}");
                } else { info!("PostgreSQL OK — migraciones aplicadas"); }
                Some(pool)
            }
            Err(e) => { tracing::warn!("PostgreSQL no disponible ({e})"); None }
        },
        Err(_) => { info!("DATABASE_URL no configurada — sin persistencia"); None }
    };

    // Channels
    let (tx, rx)              = mpsc::channel::<AppMsg>();
    let (cmd_tx, cmd_rx)      = tokio_mpsc::unbounded_channel::<CmdMsg>();
    let (bcast_tx, _)         = broadcast::channel::<String>(512);
    let interval_arc          = Arc::new(Mutex::new(CandleInterval::OneMinute));
    let (btc_provider_tx, btc_provider_rx) = tokio::sync::watch::channel(BtcPriceProvider::Binance);
    let btc_provider_tx       = Arc::new(btc_provider_tx);

    // ─── HFT Module ─────────────────────────────────────────────────────────
    let binance_depth   = Arc::new(RwLock::new(None::<BinanceDepth>));
    let binance_ring    = Arc::new(PriceRingBuffer::new());
    let tracking_state  = Arc::new(TrackingState::new());
    let (tick_tx, mut tick_rx) = tokio_mpsc::unbounded_channel::<BinanceTickEvent>();
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = AppState::new(
        cmd_tx, bcast_tx.clone(),
        Arc::clone(&interval_arc), db,
        Arc::clone(&btc_provider_tx),
        Arc::clone(&binance_depth),
        Arc::clone(&binance_ring),
        Arc::clone(&tracking_state),
        tick_tx,
    );

    // ─── Import Wisdom from file (CLI: --import-wisdom <path>) ──────────────
    if let Some(pos) = std::env::args().position(|a| a == "--import-wisdom") {
        let path = std::env::args().nth(pos + 1);
        if let Some(ref p) = path {
            match std::fs::read_to_string(p) {
                Ok(json_str) => {
                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                        Ok(wisdom) => {
                            let mut eng = state.adaptive_engine.lock().await;
                            if let Err(e) = eng.import_wisdom(&wisdom) {
                                warn!("Wisdom import from {}: {}", p, e);
                            } else {
                                info!("Wisdom imported from {}", p);
                            }
                        }
                        Err(e) => warn!("Wisdom JSON parse error in {}: {}", p, e),
                    }
                }
                Err(e) => warn!("Cannot read {}: {}", p, e),
            }
        }
    }

    // ─── Macro 24h Warm‑up (Adaptive Risk Engine) ────────────────────────────
    {
        let warm_engine = Arc::clone(&state.adaptive_engine);
        tokio::spawn(async move {
            // Fetch outside lock, then briefly lock to apply
            match warmup_fetch_and_compute().await {
                Ok(result) => {
                    warm_engine.lock().await.apply_warmup_result(result);
                    info!("AdaptiveRiskEngine warm‑up OK");
                }
                Err(e) => warn!("AdaptiveRiskEngine warm‑up failed: {e} — continuing without macro bias"),
            }
        });
    }

    // Worker (hilo OS)
    {
        let tx2 = tx.clone();
        let creds2 = Arc::clone(&creds);
        let interval_arc2 = Arc::clone(&interval_arc);
        let bcast_tx2 = bcast_tx.clone();
        std::thread::Builder::new()
            .name("polymarket-worker".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all().build().expect("tokio runtime worker")
                    .block_on(crate::modules::core::worker::run(tx2, creds2, cmd_rx, interval_arc2, bcast_tx2, btc_provider_rx));
            })
            .expect("spawn worker");
    }

    // CSV safety flush task — ensures BufWriter data reaches disk every 15s
    {
        let sm2       = Arc::clone(&state.session_manager);
        let mut shutdown2 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = interval.tick() => { sm2.flush_all(); }
                    _ = shutdown2.recv() => {
                        sm2.flush_all();
                        info!("CSV flush: final flush on shutdown");
                        return;
                    }
                }
            }
        });
    }

    // Binance depth stream (HFT)
    {
        let depth2    = Arc::clone(&binance_depth);
        let ring2     = Arc::clone(&binance_ring);
        let tick2     = state.tick_tx.clone();
        let track2    = Arc::clone(&tracking_state);
        let shutdown2 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            crate::modules::hft::binance_depth::run_binance_depth_stream(depth2, ring2, tick2, track2, shutdown2).await;
        });
    }

    // Session manager flush loop — every 30s while recording
    {
        let sm = Arc::clone(&state.session_manager);
        let mut shutdown4 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => { sm.flush_all(); }
                    _ = shutdown4.recv() => { sm.flush_all(); return; }
                }
            }
        });
    }

    // ─── HFT tick consumer: BINANCE_TICK events from depth stream ──────────
    {
        let depth3   = Arc::clone(&binance_depth);
        let ring3    = Arc::clone(&binance_ring);
        let sm3      = Arc::clone(&state.session_manager);
        let track3   = Arc::clone(&tracking_state);
        let drain3   = Arc::clone(&state.tick_drain);
        let tick_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(tick) = tick_rx.recv().await {
                let _guard = perf::TICK_CONSUMER.start();
                // Drain residual ticks on session boundary
                if drain3.swap(false, std::sync::atomic::Ordering::Acquire) {
                    while tick_rx.try_recv().is_ok() {}
                    continue;
                }
                if let Some(ref bn) = *depth3.read().await {
                    { let _g = perf::TRACK_PRICE.start(); tracking_state.track_price(tick.price, tick.event_time); }
                    { let _g = perf::RECORD_TRADE.start(); tracking_state.record_binance_trade(tick.event_time); }
                    { let _g = perf::RECORD_SAMPLE.start(); tracking_state.record_price_sample(tick.event_time, tick.price); }
                    { let _g = perf::PUSH_BOLLINGER.start(); tracking_state.push_bollinger_price(tick.price); }
                    let mut rec = { let _g = perf::BUILD_TICK.start();
                        metrics::build_binance_tick(bn, &ring3, &track3, tick.event_time, tick.price, tick.volume)
                    };
                    rec.session_id = tick_state.recording_sessions.read().await.first().copied().unwrap_or(0);
                    // ─── Adaptive Risk Engine: macro fields + master signal ───────
                    {
                        let _g = perf::ENGINE_LOCK.start();
                        let mut eng = tick_state.adaptive_engine.lock().await;
                        rec.macro_slope = eng.macro_slope();
                        rec.vfi_value = eng.vfi_value();
                        rec.macd_hist = eng.macd_hist();
                        rec.predicted_bias = eng.predicted_bias().to_string();
                        rec.is_feedback_adjusted = eng.feedback_adjusted();
                        let is_fb = rec.is_feedback_adjusted > 0;
                        let (master, cp_range, cp_valid) = { let _g2 = perf::EVAL_MASTER.start();
                            eng.evaluate_master_signal(
                                rec.binance_price, rec.poly_mid, rec.poly_spread,
                                rec.bollinger_sma, rec.bollinger_upper, rec.bollinger_lower,
                                rec.poly_imbalance, rec.price_velocity,
                                is_fb,
                                tick.volume, tick.event_time,
                                rec.tape_speed_flag, rec.absorption_ratio, rec.spoofing_flag,
                            )
                        };
                        rec.master_signal = master;
                        rec.cp_uncertainty_range = cp_range;
                        rec.cp_valid_signal = cp_valid;
                        // ─── Dynamic macro context (from shared state) ────────
                        { let _g3 = perf::MACRO_CTX_WRITE.start();
                            let mut ctx = tick_state.macro_ctx.write().await;
                            eng.sync_to_context(&mut ctx);
                            { let _g4 = perf::UPDATE_RSI.start(); eng.update_dynamic_rsi(&mut ctx, tick.price); }
                            { let _g5 = perf::CHECK_MOMENTUM.start(); eng.check_momentum_trigger(&mut ctx); }
                            rec.dynamic_rsi = ctx.dynamic_rsi;
                            rec.vfi_confidence = ctx.vfi_confidence;
                            rec.db_accuracy_factor = ctx.db_accuracy_factor;
                        }
                    }
                    { let _g = perf::SM_PUSH.start(); sm3.push(&rec); }
                }
            }
        });
    }

    // DB Scheduler
    {
        let state3 = Arc::clone(&state);
        // Recover recording sessions from DB after restart
        let recovered = session_repo::list_recording_session_ids(state3.db.as_ref()).await;
        if !recovered.is_empty() {
            info!("Recovered {} recording sessions from DB", recovered.len());
            for &sid in &recovered {
                if let Err(e) = state3.session_manager.recover_session(sid) {
                    warn!("Failed to recover session #{}: {}", sid, e);
                }
            }
            state3.recording_sessions.write().await.extend(&recovered);
        }
        // Auto-start: if nothing is recording, create indefinite 15-min session now
        if state3.recording_sessions.read().await.is_empty() && state3.db.is_some() {
            info!("No active recording sessions — auto-starting 15-min indefinite...");
            let now = chrono::Utc::now();
            let name = format!("BTC15-Auto-{}", now.format("%Y%m%dT%H%M"));
            let parent_start = now;
            let parent_end = parent_start + chrono::Duration::days(365);
            let chunk_min = 15i32;
            match session_repo::create_session(&state3, &name, parent_start, parent_end, chunk_min, 50, None).await {
                Ok(parent_id) => {
                    let btc_price = *state3.btc_price.read().await;
                    // Parent is a container — do NOT start recording, just keep as 'scheduled'
                    // The scheduler will auto-generate children and only children record data
                    // Create first child aligned to next boundary
                    let child_start = crate::modules::db::scheduler::snap_to_next_chunk(parent_start, chunk_min);
                    let child_end = child_start + chrono::Duration::minutes(chunk_min as i64);
                    let child_name = crate::modules::db::api::child_session_name(child_start, chunk_min);
                    if let Ok(child_id) = session_repo::create_session(&state3, &child_name, child_start, child_end, chunk_min, 50, Some(parent_id)).await {
                        state3.recording_sessions.write().await.push(child_id);
                        state3.session_manager.start_session(child_id).ok();
                        state3.adaptive_engine.lock().await.reset_session_warmup();
                        session_repo::start_session_recording(&state3, child_id, btc_price).await.ok();
                        info!("Auto-started indefinite session: parent #{}, child #{} ({})", parent_id, child_id, child_name);
                    }
                }
                Err(e) => warn!("Auto-start failed: {}", e),
            }
        }
        tokio::spawn(async move {
            info!("[SCHEDULER] Task started");
            crate::modules::db::scheduler::run_scheduler(state3).await;
            warn!("[SCHEDULER] Task exited unexpectedly");
        });
    }

    #[cfg(feature = "premium-collector")]
    {
        let state4 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::collector::scheduler::run_collector(state4).await;
        });
    }
    #[cfg(feature = "premium-patterns")]
    {
        let state5 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::patterns::scheduler::run_detector(state5).await;
        });
    }
    #[cfg(feature = "premium-executor")]
    {
        let state6 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::executor::scheduler::run_executor(state6).await;
        });
    }

    // Consumer de AppMsg: broadcast WS + actualiza estado + pipeline CSV + DB
    {
        let state2 = Arc::clone(&state);
        let (bridge_tx, mut bridge_rx) = tokio_mpsc::unbounded_channel::<AppMsg>();
        std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                if bridge_tx.send(msg).is_err() {
                    warn!("[CHANNEL] bridge_tx closed — exiting bridge thread");
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Some(msg) = bridge_rx.recv().await {
                if let Some(json) = msg.to_json() {
                    let _ = state2.broadcast_tx.send(json);
                }
                update_state(&msg, &state2).await;
            }
        });
    }

    let addr = "0.0.0.0:8080";
    let app  = crate::modules::core::api::router(Arc::clone(&state));

    // ─── Wisdom Checkpoint — save wisdom_state.json every hour ───────────────
    {
        let wis_state = Arc::clone(&state);
        let mut shutdown_wis = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let eng = wis_state.adaptive_engine.lock().await;
                        let wisdom = eng.export_wisdom();
                        let json = serde_json::to_string_pretty(&wisdom).unwrap_or_default();
                        if let Err(e) = std::fs::write("wisdom_state.json", &json) {
                            warn!("Wisdom checkpoint save failed: {}", e);
                        } else {
                            info!("Wisdom checkpoint saved → wisdom_state.json");
                        }
                    }
                    _ = shutdown_wis.recv() => {
                        // Emergency wisdom save on shutdown
                        let eng = wis_state.adaptive_engine.lock().await;
                        let wisdom = eng.export_wisdom();
                        let json = serde_json::to_string_pretty(&wisdom).unwrap_or_default();
                        let _ = std::fs::write("wisdom_state.json", &json);
                        info!("Emergency wisdom saved on shutdown");
                        return;
                    }
                }
            }
        });
    }

    // ─── Auto-purge CSVs older than 1 hour — every 5 minutes ────────────────
    {
        let _purge_state = Arc::clone(&state);
        let mut shutdown_purge = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(60);
                        if let Ok(entries) = std::fs::read_dir("sessions") {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().map_or(false, |e| e == "csv") {
                                    if let Ok(meta) = entry.metadata() {
                                        if let Ok(modified) = meta.modified() {
                                            let mod_time: chrono::DateTime<chrono::Utc> = modified.into();
                                            if mod_time < cutoff {
                                                if let Err(e) = std::fs::remove_file(&path) {
                                                    warn!("Auto-purge failed {}: {}", path.display(), e);
                                                } else {
                                                    info!("Auto-purged: {}", path.display());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_purge.recv() => { return; }
                }
            }
        });
    }

    // ─── SIGINT / SIGTERM graceful shutdown ──────────────────────────────────
    {
        let shutdown_sig = shutdown_tx.clone();
        let sig_state = Arc::clone(&state);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("SIGINT/SIGTERM received — emergency wisdom save...");
            let eng = sig_state.adaptive_engine.lock().await;
            let wisdom = eng.export_wisdom();
            let json = serde_json::to_string_pretty(&wisdom).unwrap_or_default();
            let _ = std::fs::write("wisdom_state.json", &json);
            // ─── Wisdom v2 + v3: T-5 + T-3 Strategies ──────────────────────
            let wisdom2 = sig_state.t5_manager.export_wisdom2();
            let _ = std::fs::write("wisdom2_state.json", &wisdom2);
            let wisdom3 = sig_state.t3_manager.export_wisdom3();
            let _ = std::fs::write("wisdom3_state.json", &wisdom3);
            let wisdom4 = sig_state.pnr_manager.export_json();
            let _ = std::fs::write("hydra_noreturn_state.json", &wisdom4);
            sig_state.session_manager.flush_all();
            info!("Graceful shutdown complete.");
            let _ = shutdown_sig.send(());
        });
    }

    info!("============================================");
    info!(" Polymarket BTC 15-min Backend");
    info!(" REST API:    http://{}/api/...", addr);
    info!(" WebSocket:   ws://{}/ws", addr);
    info!(" Black Box HFT — Wisdom condensation engine");
    info!("============================================");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ─── Consumer de AppMsg ───────────────────────────────────────────────────────

static BTC_TICK_COUNTER: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

async fn update_state(msg: &AppMsg, state: &AppState) {
    match msg {
        AppMsg::Status(s) => {
            let label = match s {
                ConnStatus::Initializing      => "Initializing".into(),
                ConnStatus::Authenticating    => "Authenticating".into(),
                ConnStatus::FetchingMarkets   => "FetchingMarkets".into(),
                ConnStatus::ConnectingWs      => "ConnectingWs".into(),
                ConnStatus::Live              => "LIVE".into(),
                ConnStatus::MarketFound(m)    => format!("MarketFound: {}", m.title),
                ConnStatus::Reconnecting(n)   => format!("Reconnecting ({})", n),
                ConnStatus::Error(e)          => format!("Error: {}", e),
            };
            *state.status.write().await = label;
            if let ConnStatus::MarketFound(info) = s {
                *state.market.write().await = Some(info.clone());
            }
        }

        AppMsg::BookUp(b) => {
            *state.book_up.write().await = Some(b.clone());
            capture_combined(state, "up", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
            capture_book_db(state, "up", &b.bids, &b.asks).await;
            // ─── T-5 + T-3 Strategies: track poly price ────────────────────
            let poly_mid = if let (Some(bid), Some(ask)) = (b.bids.first(), b.asks.first()) {
                (bid.price + ask.price) / 2.0
            } else { 0.0 };
            if poly_mid > 0.0 {
                let sid = state.recording_sessions.read().await.first().copied().unwrap_or(0);
                state.t5_manager.on_tick(Utc::now(), sid, poly_mid,
                    b.bids.first().map(|l| l.price).unwrap_or(0.0),
                    b.asks.first().map(|l| l.price).unwrap_or(0.0));
                state.t5_manager.track_volatility(sid, poly_mid);
                state.t3_manager.on_tick(Utc::now(), sid, poly_mid,
                    b.bids.first().map(|l| l.price).unwrap_or(0.0),
                    b.asks.first().map(|l| l.price).unwrap_or(0.0));
                state.t3_manager.track_volatility(sid, poly_mid);
            }
        }
        AppMsg::BookDown(b) => {
            *state.book_down.write().await = Some(b.clone());
            capture_combined(state, "down", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
            capture_book_db(state, "down", &b.bids, &b.asks).await;
        }

        AppMsg::LastTradeUp(p)   => { *state.last_trade_up.write().await   = Some(*p); }
        AppMsg::LastTradeDown(p) => { *state.last_trade_down.write().await  = Some(*p); }
        AppMsg::Balance(b)       => { *state.balance.write().await          = Some(*b); }
        AppMsg::BtcOpen(p)       => { *state.btc_open.write().await         = Some(*p); }

        AppMsg::BtcTick { price, volume, event_time } => {
            *state.btc_price.write().await = Some(*price);
            // Tracking: volumen deslizante + detección de big move
            state.tracking_state.push_volume(*event_time, *volume);
            state.tracking_state.track_price(*price, *event_time);

            let n = BTC_TICK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n % 10 == 0 {
                if let Err(e) = db::insert_btc_tick(state.db.as_ref(), *price).await {
                    warn!("DB btc_tick: {e}");
                }
            }
        }

        AppMsg::OpenOrders(o) => { *state.open_orders.write().await = o.clone(); }

        AppMsg::RecentFills(fills) => {
            // Dedup: skip if fills haven't changed since last poll (CLOB returns history, not deltas)
            let current = state.recent_fills.read().await;
            let is_same = current.len() == fills.len()
                && current.first().map_or(false, |f| fills.first().map_or(false, |g| f.time == g.time))
                && current.last().map_or(false, |f| fills.last().map_or(false, |g| f.time == g.time));
            drop(current);
            if !is_same {
                *state.recent_fills.write().await = fills.clone();
                for fill in fills.iter() {
                    if let Err(e) = db::insert_fill(state.db.as_ref(), fill).await {
                        warn!("DB insert_fill: {e}");
                    }
                }
                capture_fills_csv(state, &fills).await;
            }
        }

        AppMsg::Candles { interval, candles } => {
            *state.candles.write().await = candles.clone();
            for c in candles {
                if let Err(e) = db::upsert_candle(state.db.as_ref(), interval, c).await {
                    warn!("DB upsert_candle: {e}");
                }
            }
        }
        AppMsg::CandleUpdate(c) => {
            let interval = state.interval_arc.lock()
                .map(|g| g.binance_str().to_string())
                .unwrap_or_else(|_| "1m".into());
            let mut candles = state.candles.write().await;
            match candles.last_mut() {
                Some(last) if last.open_time == c.open_time => *last = c.clone(),
                _ => candles.push(c.clone()),
            }
            drop(candles);
            if let Err(e) = db::upsert_candle(state.db.as_ref(), &interval, c).await {
                warn!("DB upsert_candle update: {e}");
            }
        }
        AppMsg::OrderResult(r) => { info!("Order result: {}", r); }
    }
}

// ─── CSV Pipeline Unificado (20 columnas) ─────────────────────────────────────

use crate::modules::core::worker::PriceLevel;
use crate::modules::db::repository as session_repo;
use serde_json::json;

async fn capture_book_db(state: &AppState, side: &str, bids: &[PriceLevel], asks: &[PriceLevel]) {
    let session_ids = state.recording_sessions.read().await.clone();
    if session_ids.is_empty() { return; }

    let best_bid = bids.first().map(|l| l.price);
    let best_bid_sz = bids.first().map(|l| l.size);
    let best_ask = asks.first().map(|l| l.price);
    let best_ask_sz = asks.first().map(|l| l.size);
    let spread = best_bid.and_then(|bb| best_ask.map(|ba| ba - bb));
    let mid_price = best_bid.and_then(|bb| best_ask.map(|ba| (bb + ba) / 2.0));
    let bid_vol_5: f64 = bids.iter().take(5).map(|l| l.size).sum();
    let ask_vol_5: f64 = asks.iter().take(5).map(|l| l.size).sum();
    let bid_vol_10: f64 = bids.iter().take(10).map(|l| l.size).sum();
    let ask_vol_10: f64 = asks.iter().take(10).map(|l| l.size).sum();
    let bid_vol: f64 = bids.iter().map(|l| l.size).sum();
    let ask_vol: f64 = asks.iter().map(|l| l.size).sum();
    let imb = if ask_vol > 0.0 { Some(bid_vol / ask_vol) } else { None };
    let dbids = json!(bids.iter().map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
    let dasks = json!(asks.iter().map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
    let btc_price = *state.btc_price.read().await;

    for &session_id in &session_ids {
        if let Err(e) = session_repo::insert_session_snapshot(
            state, session_id, side,
            best_bid, best_bid_sz, best_ask, best_ask_sz, spread, mid_price,
            Some(bid_vol_5), Some(ask_vol_5), Some(bid_vol_10), Some(ask_vol_10),
            Some(bid_vol), Some(ask_vol), imb, mid_price, mid_price.map(|p| 1.0 - p),
            Some(dbids.clone()), Some(dasks.clone()), btc_price,
            None, None, None,
        ).await { warn!("Session snapshot #{}: {}", session_id, e); }
    }
}

/// Captura combinada: BOOK_UPDATE y TRADE usan el mismo pipeline CSV + DB.
async fn capture_combined(
    state: &AppState, _side: &str,
    poly_bids: &[PriceLevel], poly_asks: &[PriceLevel],
    evt_type: EventType, trade_side: &str, trade_price: f64, trade_size: f64,
) {
    let poly_ts = Utc::now().timestamp_millis();
    let binance_opt = state.binance_depth.read().await.clone();

    // Always build record — use defaults if Binance is unavailable
    let mut rec = if let Some(ref binance) = binance_opt {
        match evt_type {
            EventType::BookUpdate => metrics::build_book_update(
                binance, &state.binance_ring, poly_bids, poly_asks, &state.tracking_state, poly_ts,
            ),
            EventType::Trade => metrics::build_trade_record(
                binance, &state.binance_ring, poly_bids, poly_asks, &state.tracking_state,
                poly_ts, trade_side, trade_price, trade_size,
            ),
            _ => unreachable!(),
        }
    } else {
        // Binance not connected — use Poly-only defaults
        let pb_bid = poly_bids.first().map(|l| l.price).unwrap_or(0.0);
        let pb_ask = poly_asks.first().map(|l| l.price).unwrap_or(0.0);
        let pb_mid = if pb_bid > 0.0 && pb_ask > 0.0 { (pb_bid + pb_ask) / 2.0 } else { 0.0 };
        let pb_vol_bid: f64 = poly_bids.iter().map(|l| l.size).sum();
        let pb_vol_ask: f64 = poly_asks.iter().map(|l| l.size).sum();
        let pb_imb = if pb_vol_ask > 0.0 { pb_vol_bid / pb_vol_ask } else { 0.0 };

        let mut rec = CsvRecord {
            event_type: evt_type,
            ts_local: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            poly_bid: pb_bid,
            poly_ask: pb_ask,
            poly_mid: pb_mid,
            poly_spread: if pb_bid > 0.0 && pb_ask > 0.0 { pb_ask - pb_bid } else { 0.0 },
            poly_bid_vol_all: pb_vol_bid,
            poly_ask_vol_all: pb_vol_ask,
            poly_imbalance: if pb_imb.is_finite() { pb_imb } else { 0.0 },
            ..Default::default()
        };
        if evt_type == EventType::Trade {
            rec.trade_side = trade_side.to_string();
            rec.trade_price = trade_price;
            rec.trade_size = trade_size;
        }
        rec
    };
    // Tag with session_id BEFORE pushing to per-session CSV + in-memory buffer
    let session_ids = state.recording_sessions.read().await.clone();
    let active_sid = state.session_manager.active_ids().first().copied()
        .or_else(|| session_ids.first().copied())
        .unwrap_or(0);
    rec.session_id = active_sid;

    // ─── Adaptive Risk Engine: macro fields + master signal ──────────────────
    {
        let mut eng = state.adaptive_engine.lock().await;
        rec.macro_slope = eng.macro_slope();
        rec.vfi_value = eng.vfi_value();
        rec.macd_hist = eng.macd_hist();
        rec.predicted_bias = eng.predicted_bias().to_string();
        rec.is_feedback_adjusted = eng.feedback_adjusted();
        let is_fb = rec.is_feedback_adjusted > 0;
        let ts_now = chrono::Utc::now().timestamp_millis();
        let (master, cp_range, cp_valid) = eng.evaluate_master_signal(
            rec.binance_price, rec.poly_mid, rec.poly_spread,
            rec.bollinger_sma, rec.bollinger_upper, rec.bollinger_lower,
            rec.poly_imbalance, rec.price_velocity,
            is_fb,
            rec.binance_vol_100ms, ts_now,
            rec.tape_speed_flag, rec.absorption_ratio, rec.spoofing_flag,
        );
        rec.master_signal = master;
        rec.cp_uncertainty_range = cp_range;
        rec.cp_valid_signal = cp_valid;
        // ─── Dynamic macro context (from shared state) ────────────────────
        let ctx = state.macro_ctx.read().await;
        rec.dynamic_rsi = ctx.dynamic_rsi;
        rec.vfi_confidence = ctx.vfi_confidence;
        rec.db_accuracy_factor = ctx.db_accuracy_factor;
        // ─── T-5 Certainty Strategy (Wisdom v2) ────────────────────────────
        let (t5_pred, t5_entry, _t5_correct) = state.t5_manager.get_prediction(active_sid);
        rec.t5_prediction = t5_pred;
        rec.t5_entry_price = t5_entry;
        // ─── T-3 Aggressive Strategy (Wisdom v3) ────────────────────────────
        let (t3_pred, t3_entry, t3_active) = state.t3_manager.get_prediction(active_sid);
        rec.t3_prediction = t3_pred;
        rec.t3_entry_price = t3_entry;
        rec.t3_active = if t3_active { 1 } else { 0 };
        // ─── PNR: Point of No Return indicators (last 5 min analysis) ────────
        let secs_left = state.t5_manager.seconds_left(active_sid);
        if secs_left >= 0 && secs_left <= 300 {
            rec.pnr_active = 1;
            rec.pnr_seconds_left = secs_left as i32;
            rec.pnr_price = rec.poly_mid;
            rec.pnr_return_up = if rec.poly_ask > 0.0 { 1.0 - rec.poly_ask } else { 0.0 };
            rec.pnr_return_down = if rec.poly_bid > 0.0 { rec.poly_bid } else { 0.0 };
            rec.pnr_volatility_1m = rec.poly_liquidity_delta.abs();
            rec.pnr_confidence = ((rec.poly_mid - 0.5).abs() * 2.0).min(1.0);
            rec.pnr_trend = if rec.predicted_bias.contains("UP") { 1 }
                else if rec.predicted_bias.contains("DOWN") { -1 } else { 0 };
            rec.pnr_spread_pct = if rec.poly_mid > 0.0 { rec.poly_spread / rec.poly_mid } else { 0.0 };
            // Feed to Hydra No Return accumulator
            state.pnr_manager.accumulate_tick(active_sid, secs_left as i32,
                rec.pnr_price, rec.pnr_return_up, rec.pnr_return_down);
        }
        // ─── Insight Strategies: Cerbero + Fenix ─────────────────────────────
        let insights = state.insight_manager.on_tick(active_sid, rec.poly_mid);
        for (code, active, dir) in insights {
            match code.as_str() {
                "cerbero70" => { rec.cerbero70_active = active; rec.cerbero70_price = if active>0 {rec.poly_mid} else {0.0}; rec.cerbero70_dir = dir as i8; }
                "cerbero80" => { rec.cerbero80_active = active; rec.cerbero80_price = if active>0 {rec.poly_mid} else {0.0}; rec.cerbero80_dir = dir as i8; }
                "cerbero90" => { rec.cerbero90_active = active; rec.cerbero90_price = if active>0 {rec.poly_mid} else {0.0}; rec.cerbero90_dir = dir as i8; }
                "fenix35"   => { rec.fenix35_active = active; rec.fenix35_price = if active>0 {rec.poly_mid} else {0.0}; rec.fenix35_dir = dir as i8; }
                "fenix30"   => { rec.fenix30_active = active; rec.fenix30_price = if active>0 {rec.poly_mid} else {0.0}; rec.fenix30_dir = dir as i8; }
                "fenix45"   => { rec.fenix45_active = active; rec.fenix45_price = if active>0 {rec.poly_mid} else {0.0}; rec.fenix45_dir = dir as i8; }
                _ => {}
            }
        }
        // ─── Fenix Trading: paper-trading simulation ──────────────────────────
        let fenix_trades = state.fenix_trading.on_tick(active_sid, rec.poly_mid, rec.poly_bid, rec.poly_ask);
        for (code, active, _entry) in fenix_trades {
            match code.as_str() {
                "fenix35"   => { rec.fenix35_trade = active; }
                "fenix30"   => { rec.fenix30_trade = active; }
                "fenix45"   => { rec.fenix45_trade = active; }
                "fenix40"   => { rec.fenix40_trade = active; }
                "fenix4550" => { rec.fenix4550_trade = active; }
                _ => {}
            }
            // entry price could go to CSV if needed
        }
    }

    // Per-session CSV file (multi-writer: each session gets its own file)
    state.session_manager.push(&rec);

    // Strategy Manager: evaluate both shadow strategies (A: Imbalance, B: Liquidity)
    let result = state.strategy_manager.lock().unwrap().evaluate(&rec);

    // Update the in-memory record with strategy fields
    {
        let mut mem = state.mem_hft.write().await;
        if let Some(last) = mem.last_mut() {
            last.imba_status = result.imba.status;
            last.imba_side = result.imba.side;
            last.imba_entry_price = result.imba.entry_price;
            last.imba_exit_price = result.imba.exit_price;
            last.imba_trade_pnl = result.imba.trade_pnl;
            last.imba_balance = result.imba.balance;
            last.liqb_status = result.liqb.status;
            last.liqb_side = result.liqb.side;
            last.liqb_entry_price = result.liqb.entry_price;
            last.liqb_exit_price = result.liqb.exit_price;
            last.liqb_trade_pnl = result.liqb.trade_pnl;
            last.liqb_balance = result.liqb.balance;
        }
    }
    // NO mid-session flush — all data stays in BufWriter + OS page cache
    // until session end (stop_session) or graceful shutdown (SIGINT).

    // In-memory buffer
    state.mem_hft.write().await.push(rec.clone());

    // DB insert (only if PostgreSQL is available) — write to all recording sessions
    if let Some(_pool) = state.db.as_ref() {
        for &sid in &session_ids {
            let _ = sqlx::query(
                "INSERT INTO hft_snapshots (btc_price_binance, btc_bid_vol_5, btc_ask_vol_5, poly_mid_price, poly_imbalance, latency_delta, session_id, binance_lag_ms, binance_micro_price_at_t) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"
            )
            .bind(rec.binance_price)
            .bind(rec.binance_vol_100ms)
            .bind(0.0f64)
            .bind(rec.poly_mid)
            .bind(rec.poly_imbalance)
            .bind(rec.latencia_ms as f64)
            .bind(sid)
            .bind(rec.latencia_ms)
            .bind(rec.binance_micro_price)
            .execute(_pool)
            .await;
        }
        // For Trade events: also insert into session_trades table (previously capture_fills_db)
        if evt_type == EventType::Trade && !session_ids.is_empty() {
            let btc_price = *state.btc_price.read().await;
            let db_trade_side = match trade_side {
                "BUY" => "buy", "SELL" => "sell", _ => trade_side,
            };
            for &sid in &session_ids {
                if let Err(e) = session_repo::insert_session_trade(
                    state, sid, _side, db_trade_side, trade_price, trade_size, btc_price,
                ).await { warn!("Session trade #{}: {}", sid, e); }
            }
        }
    }
}

async fn capture_fills_csv(state: &AppState, fills: &[crate::modules::core::worker::RecentFill]) {
    let up_book   = state.book_up.read().await.clone();
    let down_book = state.book_down.read().await.clone();

    for fill in fills {
        let (bids, asks) = match fill.outcome.as_str() {
            "Up" | "up" => (
                up_book.as_ref().map(|b| &b.bids[..]).unwrap_or(&[]),
                up_book.as_ref().map(|b| &b.asks[..]).unwrap_or(&[]),
            ),
            _ => (
                down_book.as_ref().map(|b| &b.bids[..]).unwrap_or(&[]),
                down_book.as_ref().map(|b| &b.asks[..]).unwrap_or(&[]),
            ),
        };

        let trade_side = match fill.side {
            crate::modules::core::worker::OrderSide::Buy => "BUY",
            crate::modules::core::worker::OrderSide::Sell => "SELL",
        };

        capture_combined(
            state, &fill.outcome, bids, asks,
            EventType::Trade, trade_side, fill.price, fill.size,
        ).await;
    }
}
