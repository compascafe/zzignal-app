//! zzignal — BTC 15-min Polymarket Trading Engine

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::time::Duration;

mod models;
mod controllers;
mod services;
mod utils;
use std::sync::{mpsc, Arc, Mutex};
use chrono::{Timelike, Utc};
use tokio::sync::{broadcast, mpsc as tokio_mpsc, RwLock};
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use crate::controllers::worker::{AppMsg, CandleInterval, CmdMsg, ConnStatus};
use crate::models::credentials::ClobCredentials;
use crate::models::state::AppState;
use crate::models::hft::{BinanceDepth, EventType};
use crate::models::hft::PriceRingBuffer;
use crate::services::metrics::{self, TrackingState};
use crate::services::binance::BinanceTickEvent;
use crate::services::perf;
use crate::services::pipeline;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("[warn] .env no cargado: {e}");
    }
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).with_target(false).compact().finish();
    tracing::subscriber::set_global_default(subscriber)?;
    info!("zzignal v{} built @{}", env!("GIT_VERSION"), env!("BUILD_TIME"));

    let creds = match ClobCredentials::from_env() {
        Ok(c) => { info!("Wallet: {}", c.display_address()); Arc::new(c) }
        Err(e) => { error!("Credenciales no disponibles: {:#}", e); return Ok(()); }
    };

    let (tx, rx)              = mpsc::channel::<AppMsg>();
    let (cmd_tx, cmd_rx)      = tokio_mpsc::unbounded_channel::<CmdMsg>();
    let (bcast_tx, _)         = broadcast::channel::<String>(512);
    let interval_arc          = Arc::new(Mutex::new(CandleInterval::OneMinute));

    let binance_depth   = Arc::new(RwLock::new(None::<BinanceDepth>));
    let binance_ring    = Arc::new(PriceRingBuffer::new());
    let tracking_state  = Arc::new(TrackingState::new());
    let (tick_tx, mut tick_rx) = tokio_mpsc::unbounded_channel::<BinanceTickEvent>();
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = AppState::new(
        cmd_tx, bcast_tx.clone(),
        Arc::clone(&interval_arc),
        Arc::clone(&binance_depth),
        Arc::clone(&binance_ring),
        Arc::clone(&tracking_state),
        tick_tx,
        (*creds).clone(),
    );

    // Estrategias desactivadas por defecto — trading manual desde TUI
    state.odiseo_trading.set_live_mode(false);
    state.odiseo_trading.set_variant(0, false);
    state.odiseo_trading.set_variant(1, false);
    state.odiseo_trading.set_variant(2, false);

    // Auto-session manager: starts a new 15-min session at each boundary
    {
        let auto_state = Arc::clone(&state);
        let mut shutdown_ses = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut last_session_id: i32 = 0;
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Utc::now();
                        let t = now.time();
                        let secs_into_chunk = (t.minute() as i64 % 15) * 60 + t.second() as i64;
                        let secs_left = 900 - secs_into_chunk;

                        // Start new session at boundary (within first 2 seconds of chunk)
                        let recording = auto_state.recording_sessions.read().await;
                        let has_active = !recording.is_empty();
                        drop(recording);

                        if !has_active && secs_left >= 898 {
                            let chunk_start = now - chrono::Duration::seconds(secs_into_chunk);
                            let chunk_end = chunk_start + chrono::Duration::minutes(15);
                            last_session_id += 1;
                            let sid = last_session_id;
                            let name = format!("S{:04}-{}", sid, chunk_start.format("%H%M"));

                            info!("[SESSION] Auto-start #{} {} ({}→{})", sid, name,
                                chunk_start.format("%H:%M"), chunk_end.format("%H:%M"));

                            auto_state.recording_sessions.write().await.push(sid);
                            auto_state.session_manager.start_session(sid, &name).ok();
                            auto_state.tracking_state.reset_session_baselines();
                            auto_state.tick_drain.store(true, std::sync::atomic::Ordering::Release);
                        }

                        // Stop sessions that have passed their end
                        // Drain IDs first, drop lock, then do I/O outside lock
                        if has_active && secs_left <= 2 && secs_left >= 0 {
                            let to_stop: Vec<i32> = {
                                let mut rec = auto_state.recording_sessions.write().await;
                                rec.drain(..).collect()
                            };
                            for sid in to_stop {
                                info!("[SESSION] Auto-stop #{}", sid);
                                auto_state.odiseo_trading.on_session_close(sid, "tie");
                                auto_state.session_manager.flush(sid).ok();
                                auto_state.session_manager.stop_session(sid).ok();
                            }
                        }
                    }
                    _ = shutdown_ses.recv() => {
                        auto_state.session_manager.flush_all();
                        return;
                    }
                }
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

    // CSV safety flush — every 15s
    {
        let sm2 = Arc::clone(&state.session_manager);
        let mut shutdown2 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = interval.tick() => { sm2.flush_all(); }
                    _ = shutdown2.recv() => { sm2.flush_all(); return; }
                }
            }
        });
    }

    // Binance depth stream (HFT)
    {
        let depth2 = Arc::clone(&binance_depth);
        let ring2  = Arc::clone(&binance_ring);
        let tick2  = state.tick_tx.clone();
        let track2 = Arc::clone(&tracking_state);
        let shutdown2 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            crate::services::binance::run_binance_depth_stream(depth2, ring2, tick2, track2, shutdown2).await;
        });
    }

    // Session manager flush — every 30s
    {
        let sm = Arc::clone(&state.session_manager);
        let mut shutdown4 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => { sm.flush_all(); }
                    _ = shutdown4.recv() => { sm.flush_all(); return; }
                }
            }
        });
    }

    // HFT tick consumer
    {
        let depth3 = Arc::clone(&binance_depth);
        let ring3  = Arc::clone(&binance_ring);
        let sm3    = Arc::clone(&state.session_manager);
        let track3 = Arc::clone(&tracking_state);
        let drain3 = Arc::clone(&state.tick_drain);
        let tick_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(tick) = tick_rx.recv().await {
                let _guard = perf::TICK_CONSUMER.start();
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

    // Consumer de AppMsg: broadcast WS + actualiza estado + pipeline CSV
    {
        let state2 = Arc::clone(&state);
        let (bridge_tx, mut bridge_rx) = tokio_mpsc::unbounded_channel::<AppMsg>();
        std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                if bridge_tx.send(msg).is_err() { break; }
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

    // Auto-purge CSVs older than 1 hour — every 5 minutes
    {
        let mut shutdown_purge = shutdown_tx.subscribe();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
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

    // SIGINT/SIGTERM graceful shutdown
    {
        let shutdown_sig = shutdown_tx.clone();
        let sig_state = Arc::clone(&state);
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("SIGINT received — flushing CSVs...");
            sig_state.session_manager.flush_all();
            info!("Graceful shutdown complete.");
            let _ = shutdown_sig.send(());
        });
    }

    info!("============================================");
    info!(" ZZIGNAL BTC 15-min Backend");
    info!(" REST API:    http://{}/api/...", "0.0.0.0:8080");
    info!(" WebSocket:   ws://0.0.0.0:8080/ws");
    info!("============================================");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    let app = crate::controllers::api::router(Arc::clone(&state));
    axum::serve(listener, app).await?;
    Ok(())
}

static BTC_TICK_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

async fn update_state(msg: &AppMsg, state: &AppState) {
    match msg {
        AppMsg::Status(s) => {
            let label = match s {
                ConnStatus::Initializing => "Initializing".into(),
                ConnStatus::Authenticating => "Authenticating".into(),
                ConnStatus::FetchingMarkets => "FetchingMarkets".into(),
                ConnStatus::ConnectingWs => "ConnectingWs".into(),
                ConnStatus::Live => "LIVE".into(),
                ConnStatus::MarketFound(m) => format!("MarketFound: {}", m.title),
                ConnStatus::Reconnecting(n) => format!("Reconnecting ({})", n),
                ConnStatus::Error(e) => format!("Error: {}", e),
            };
            *state.status.write().await = label;
            if let ConnStatus::MarketFound(info) = s {
                *state.market.write().await = Some(info.clone());
            }
        }
        AppMsg::BookUp(b) => {
            *state.book_up.write().await = Some(b.clone());
            if let Some(bid) = b.bids.first() {
                *state.best_bid_up.write().await = bid.price;
            }
            pipeline::push_depth_frame(state, 0, &b.bids, &b.asks).await;
            pipeline::capture_combined(state, "up", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
        }
        AppMsg::BookDown(b) => {
            *state.book_down.write().await = Some(b.clone());
            if let Some(bid) = b.bids.first() {
                *state.best_bid_dn.write().await = bid.price;
            }
            pipeline::push_depth_frame(state, 1, &b.bids, &b.asks).await;
            pipeline::capture_combined(state, "down", &b.bids, &b.asks, EventType::BookUpdate, "", 0.0, 0.0).await;
        }
        AppMsg::LastTradeUp { price, size } => {
            let mut prev = state.prev_raw_up.write().await;
            let current = *price;
            *state.raw_trade_up.write().await = current;
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
        AppMsg::Balance(b) => { *state.balance.write().await = Some(*b); }
        AppMsg::BtcOpen(p) => { *state.btc_open.write().await = Some(*p); }
        AppMsg::BtcTick { price, volume, event_time } => {
            *state.btc_price.write().await = Some(*price);
            *state.btc_volume.write().await = *volume;
            state.tracking_state.push_volume(*event_time, *volume);
            state.tracking_state.track_price(*price, *event_time);
            {
                let mut window = state.btc_vol_window.write().await;
                window.push_back((*event_time, *volume));
                let cutoff = *event_time - 60_000;
                while window.front().map_or(false, |(ts, _)| *ts < cutoff) { window.pop_front(); }
                *state.btc_vol_1m.write().await = window.iter().map(|(_, v)| *v).sum();
            }
            *state.btc_vol_ses.write().await += *volume;
            let _ = BTC_TICK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        AppMsg::OpenOrders(o) => { *state.open_orders.write().await = o.clone(); }
        AppMsg::RecentFills(fills) => {
            let current = state.recent_fills.read().await;
            let is_same = current.len() == fills.len()
                && current.first().map_or(false, |f| fills.first().map_or(false, |g| f.time == g.time))
                && current.last().map_or(false, |f| fills.last().map_or(false, |g| f.time == g.time));
            drop(current);
            if !is_same {
                *state.recent_fills.write().await = fills.clone();
                pipeline::capture_fills_csv(state, &fills).await;
            }
        }
        AppMsg::Candles { interval: _, candles } => {
            *state.candles.write().await = candles.clone();
        }
        AppMsg::CandleUpdate(c) => {
            let mut candles = state.candles.write().await;
            match candles.last_mut() {
                Some(last) if last.open_time == c.open_time => *last = c.clone(),
                _ => candles.push(c.clone()),
            }
        }
        AppMsg::OrderResult(r) => { info!("Order result: {}", r); }
    }
}
