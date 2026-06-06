//! zzignal-core — BTC 15-min Polymarket Trading Engine
//!
//! Bootstrap: wires all modules together and starts the Axum + Tokio runtime.
//! The MVC architecture is declared in lib.rs.

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::time::Duration;

// Module declarations shared with lib.rs (binary crate root)
mod models;
mod controllers;
mod services;
mod db;
mod utils;
#[cfg(any(
    feature = "premium-collector",
    feature = "premium-patterns",
    feature = "premium-executor"
))]
mod premium;

use std::sync::{mpsc, Arc, Mutex};

use chrono::Utc;
use tokio::sync::{broadcast, mpsc as tokio_mpsc, RwLock};
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use crate::controllers::worker::{AppMsg, CandleInterval, CmdMsg, ConnStatus};
use crate::models::credentials::ClobCredentials;
use crate::models::state::AppState;
use crate::utils::persistence;
use crate::db::repository as session_repo;
use crate::models::hft::{BinanceDepth, EventType};
use crate::utils::ring_buffer::PriceRingBuffer;
use crate::services::metrics::{self, TrackingState};
use crate::services::binance::BinanceTickEvent;
use crate::services::risk::warmup_fetch_and_compute;
use crate::services::perf;
use crate::services::pipeline;

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

    info!("zzignal-app v{} built @{}", env!("GIT_VERSION"), env!("BUILD_TIME"));

    let creds = match ClobCredentials::from_env() {
        Ok(c) => { info!("Wallet: {}", c.display_address()); Arc::new(c) }
        Err(e) => { error!("Credenciales no disponibles: {:#}", e); return Ok(()); }
    };

    let db = match std::env::var("DATABASE_URL") {
        Ok(url) => match sqlx::PgPool::connect(&url).await {
            Ok(pool) => {
                if let Err(e) = persistence::run_migrations(&pool).await {
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

    // ─── HFT Module ─────────────────────────────────────────────────────────
    let binance_depth   = Arc::new(RwLock::new(None::<BinanceDepth>));
    let binance_ring    = Arc::new(PriceRingBuffer::new());
    let tracking_state  = Arc::new(TrackingState::new());
    let (tick_tx, mut tick_rx) = tokio_mpsc::unbounded_channel::<BinanceTickEvent>();
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = AppState::new(
        cmd_tx, bcast_tx.clone(),
        Arc::clone(&interval_arc), db,
        Arc::clone(&binance_depth),
        Arc::clone(&binance_ring),
        Arc::clone(&tracking_state),
        tick_tx,
        (*creds).clone(),
    );

    // ─── ESTRATEGIAS AUTOMATICAS DESACTIVADAS — trading manual ───
    // Los indicadores (momentum, BTC vel, CLOB delta) siguen activos
    // Las ordenes se ejecutan MANUALMENTE desde el TUI via comandos /b /s /l
    state.odiseo_trading.set_live_mode(false);
    state.odiseo_trading.set_variant(0, false); // Odiseo 83 OFF
    state.odiseo_trading.set_variant(1, false); // Houdini 65 OFF
    state.odiseo_trading.set_variant(2, false); // Senna OFF

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
                    .block_on(crate::controllers::worker::run(tx2, creds2, cmd_rx, interval_arc2, bcast_tx2));
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
            crate::services::binance::run_binance_depth_stream(depth2, ring2, tick2, track2, shutdown2).await;
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
                // Drain residual ticks on session boundary (capped at 50)
                if drain3.swap(false, std::sync::atomic::Ordering::Acquire) {
                    let mut drained = 0;
                    while drained < 50 && tick_rx.try_recv().is_ok() { drained += 1; }
                    continue;
                }
                if let Some(ref bn) = *depth3.read().await {
                    { let _g = perf::TRACK_PRICE.start(); tracking_state.track_price(tick.price, tick.event_time); }
                    { let _g = perf::RECORD_TRADE.start(); tracking_state.record_binance_trade(tick.event_time); }
                    { let _g = perf::RECORD_SAMPLE.start(); tracking_state.record_price_sample(tick.event_time, tick.price); }
                    let mut rec = { let _g = perf::BUILD_TICK.start();
                        metrics::build_binance_tick(bn, &ring3, &track3, tick.event_time, tick.price, tick.volume)
                    };
                    rec.session_id = tick_state.recording_sessions.read().await.last().copied().unwrap_or(0);
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
        let mut valid_ids: Vec<i32> = Vec::new();
        if !recovered.is_empty() {
            info!("Recovered {} recording sessions from DB — validating...", recovered.len());
            for &sid in &recovered {
                match session_repo::get_session_by_id(&state3, sid).await {
                    Ok(Some(sess)) => {
                        if sess.duration_min != 15 {
                            // Stop non-15-min stale sessions immediately
                            warn!("[RECOVERY] Stopping stale session #{} (duration={}min, not BTC 15-min)", sid, sess.duration_min);
                            let btc_price = *state3.btc_price.read().await;
                            if let Err(e) = session_repo::stop_session(&state3, sid, btc_price, btc_price).await {
                                warn!("Failed to stop stale session #{}: {}", sid, e);
                            }
                        } else {
                            valid_ids.push(sid);
                            if let Err(e) = state3.session_manager.recover_session(sid) {
                                warn!("Failed to recover session #{}: {}", sid, e);
                            }
                            // Use actual scheduled_end if still in future, capped at next 15-min boundary
                            let now = chrono::Utc::now();
                            let cap = crate::db::scheduler::snap_to_next_chunk(now, 15);
                            let t5_end = if sess.scheduled_end > now {
                                sess.scheduled_end.min(cap)
                            } else {
                                cap
                            };
                            state3.t5_manager.on_session_start(sid, t5_end);
                            state3.t3_manager.on_session_start(sid, t5_end);
                            info!("[RECOVERY] Session #{} ({}→{}) t5/t3 restored (countdown end: {})", sid,
                                sess.scheduled_start.format("%H:%M"), sess.scheduled_end.format("%H:%M"),
                                t5_end.format("%H:%M:%S"));
                        }
                    }
                    Ok(None) => warn!("[RECOVERY] Session #{} not found in DB — skipping", sid),
                    Err(e) => warn!("[RECOVERY] Session #{} query failed: {}", sid, e),
                }
            }
            if !valid_ids.is_empty() {
                info!("[RECOVERY] {} valid 15-min sessions restored", valid_ids.len());
                state3.recording_sessions.write().await.extend(&valid_ids);
            }
        }
        // Cleanup: cancel ALL non-15-min scheduled sessions (stale DB entries)
        if state3.db.is_some() {
            let cancelled = session_repo::cancel_non_15min_scheduled(state3.db.as_ref()).await;
            if cancelled > 0 {
                info!("[CLEANUP] Cancelled {} stale non-15-min scheduled sessions", cancelled);
            }
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
                    let child_start = crate::db::scheduler::snap_to_next_chunk(parent_start, chunk_min);
                    let child_end = child_start + chrono::Duration::minutes(chunk_min as i64);
                    let child_name = crate::db::api::child_session_name(child_start, chunk_min);
                    if let Ok(child_id) = session_repo::create_session(&state3, &child_name, child_start, child_end, chunk_min, 50, Some(parent_id)).await {
                        state3.recording_sessions.write().await.push(child_id);
                        state3.session_manager.start_session(child_id, &child_name).ok();
                        // Session starts NOW — countdown uses next 15-min boundary (aligned to Polymarket round)
                        let t5_end = child_end.min(crate::db::scheduler::snap_to_next_chunk(now, 15));
                        state3.t5_manager.on_session_start(child_id, t5_end);
                        state3.t3_manager.on_session_start(child_id, t5_end);
                        // Reset trade prices to avoid stale triggers from previous session
                        state3.trade_window_up.write().await.clear();
                        state3.trade_window_dn.write().await.clear();
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
            crate::db::scheduler::run_scheduler(state3).await;
            warn!("[SCHEDULER] Task exited unexpectedly");
        });
    }

    #[cfg(feature = "premium-collector")]
    {
        let state4 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::premium::collector::scheduler::run_collector(state4).await;
        });
    }
    #[cfg(feature = "premium-patterns")]
    {
        let state5 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::premium::patterns::scheduler::run_detector(state5).await;
        });
    }
    #[cfg(feature = "premium-executor")]
    {
        let state6 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::premium::executor::scheduler::run_executor(state6).await;
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
    let app  = crate::controllers::api::router(Arc::clone(&state));

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
            // Store best bid UP for exit price tracking
            if let Some(bid) = b.bids.first() {
                *state.best_bid_up.write().await = bid.price;
            }
            // ─── Capturar snapshot completo del orderbook en memoria ─────
            pipeline::push_depth_frame(state, 0, &b.bids, &b.asks).await;
            pipeline::capture_combined(state, "up", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
            pipeline::capture_book_db(state, "up", &b.bids, &b.asks).await;
            // ─── T-5 + T-3 Strategies: track poly price ────────────────────
            let poly_mid = if let (Some(bid), Some(ask)) = (b.bids.first(), b.asks.first()) {
                (bid.price + ask.price) / 2.0
            } else { 0.0 };
            if poly_mid > 0.0 {
                let sid = state.recording_sessions.read().await.last().copied().unwrap_or(0);
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
            // Store best bid DOWN for exit price tracking
            if let Some(bid) = b.bids.first() {
                *state.best_bid_dn.write().await = bid.price;
            }
            // ─── Capturar snapshot completo del orderbook en memoria ─────
            pipeline::push_depth_frame(state, 1, &b.bids, &b.asks).await;
            pipeline::capture_combined(state, "down", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
            pipeline::capture_book_db(state, "down", &b.bids, &b.asks).await;
        }

        AppMsg::LastTradeUp { price, size }   => {
            let mut prev = state.prev_raw_up.write().await;
            let current = *price;
            *state.raw_trade_up.write().await = current;
            // Update prev for next tick's momentum (done below after window update)
            let min_vol = *state.trade_min_vol.read().await;
            if *size >= min_vol {
                let mut window = state.trade_window_up.write().await;
                window.push_back((*price, *size));
                let max_n = *state.trade_window_n.read().await;
                while window.len() > max_n { window.pop_front(); }
            }
            *prev = current;
        }
        AppMsg::LastTradeDown { price, size } => {
            let mut prev = state.prev_raw_dn.write().await;
            let current = *price;
            *state.raw_trade_dn.write().await = current;
            let min_vol = *state.trade_min_vol.read().await;
            if *size >= min_vol {
                let mut window = state.trade_window_dn.write().await;
                window.push_back((*price, *size));
                let max_n = *state.trade_window_n.read().await;
                while window.len() > max_n { window.pop_front(); }
            }
            *prev = current;
        }
        AppMsg::Balance(b)       => { *state.balance.write().await          = Some(*b); }
        AppMsg::BtcOpen(p)       => { *state.btc_open.write().await         = Some(*p); }

        AppMsg::BtcTick { price, volume, event_time } => {
            *state.btc_price.write().await = Some(*price);
            *state.btc_volume.write().await = *volume;
            // Tracking: volumen deslizante + detección de big move
            state.tracking_state.push_volume(*event_time, *volume);
            state.tracking_state.track_price(*price, *event_time);

            // Rolling 60s real BTC volume (aggTrade per-tick sum)
            {
                let mut window = state.btc_vol_window.write().await;
                window.push_back((*event_time, *volume));
                let cutoff = *event_time - 60_000;
                while window.front().map_or(false, |(ts, _)| *ts < cutoff) {
                    window.pop_front();
                }
                *state.btc_vol_1m.write().await = window.iter().map(|(_, v)| *v).sum();
            }
            *state.btc_vol_ses.write().await += *volume;

            let n = BTC_TICK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n % 10 == 0 {
                if let Err(e) = persistence::insert_btc_tick(state.db.as_ref(), *price).await {
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
                    if let Err(e) = persistence::insert_fill(state.db.as_ref(), fill).await {
                        warn!("DB insert_fill: {e}");
                    }
                }
                pipeline::capture_fills_csv(state, &fills).await;
            }
        }

        AppMsg::Candles { interval, candles } => {
            *state.candles.write().await = candles.clone();
            for c in candles {
                if let Err(e) = persistence::upsert_candle(state.db.as_ref(), interval, c).await {
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
            if let Err(e) = persistence::upsert_candle(state.db.as_ref(), &interval, c).await {
                warn!("DB upsert_candle update: {e}");
            }
        }
        AppMsg::OrderResult(r) => { info!("Order result: {}", r); }
    }
}
