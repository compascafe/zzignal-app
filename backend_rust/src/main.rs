//! Polymarket BTC 15-min — Backend Service
//!
//! Pipeline HFT unificado: BOOK_UPDATE | TRADE | BINANCE_TICK → CSV 20 columnas

#![allow(dead_code)]

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

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
use crate::modules::hft::logger::CsvLogger;
use crate::modules::hft::binance_depth::BinanceTickEvent;

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
    let csv_logger      = Arc::new(CsvLogger::new("hft_snapshots.csv"));
    let (tick_tx, mut tick_rx) = tokio_mpsc::unbounded_channel::<BinanceTickEvent>();
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let state = AppState::new(
        cmd_tx, bcast_tx.clone(), shutdown_tx.clone(),
        Arc::clone(&interval_arc), db,
        Arc::clone(&btc_provider_tx),
        Arc::clone(&binance_depth),
        Arc::clone(&binance_ring),
        Arc::clone(&tracking_state),
        Arc::clone(&csv_logger),
        tick_tx,
    );

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

    // CSV flush task
    {
        let logger2   = Arc::clone(&csv_logger);
        let shutdown3 = shutdown_tx.subscribe();
        tokio::spawn(async move {
            crate::modules::hft::logger::csv_flush_loop(logger2, shutdown3).await;
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
                    _ = interval.tick() => { let _ = sm.flush(); }
                    _ = shutdown4.recv() => { let _ = sm.flush(); return; }
                }
            }
        });
    }

    // ─── HFT tick consumer: BINANCE_TICK events from depth stream ──────────
    {
        let depth3   = Arc::clone(&binance_depth);
        let ring3    = Arc::clone(&binance_ring);
        let csv3     = Arc::clone(&csv_logger);
        let sm3      = Arc::clone(&state.session_manager);
        let track3   = Arc::clone(&tracking_state);
        let drain3   = Arc::clone(&state.tick_drain);
        tokio::spawn(async move {
            while let Some(tick) = tick_rx.recv().await {
                // Drain residual ticks on session boundary
                if drain3.swap(false, std::sync::atomic::Ordering::Acquire) {
                    while tick_rx.try_recv().is_ok() {}
                    continue; // discard this tick — it's from the old session
                }
                if let Some(ref bn) = *depth3.read().await {
                    tracking_state.track_price(tick.price, tick.event_time);
                    tracking_state.record_binance_trade(tick.event_time);
                    tracking_state.record_price_sample(tick.event_time, tick.price);
                    let rec = metrics::build_binance_tick(bn, &ring3, &track3, tick.event_time, tick.price, tick.volume);
                    csv3.push(rec.clone());
                    sm3.push(&rec);
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
            state3.recording_sessions.write().await.extend(&recovered);
        }
        tokio::spawn(async move {
            crate::modules::db::scheduler::run_scheduler(state3).await;
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
                let _ = bridge_tx.send(msg);
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

    info!("============================================");
    info!(" Polymarket BTC 15-min Backend");
    info!(" REST API:    http://{}/api/...", addr);
    info!(" WebSocket:   ws://{}/ws", addr);
    info!(" HFT CSV:     hft_snapshots.csv");
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
            *state.recent_fills.write().await = fills.clone();
            for fill in fills {
                if let Err(e) = db::insert_fill(state.db.as_ref(), fill).await {
                    warn!("DB insert_fill: {e}");
                }
            }
            capture_fills_csv(state, fills).await;
            capture_fills_db(state, fills).await;
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

async fn capture_fills_db(state: &AppState, fills: &[crate::modules::core::worker::RecentFill]) {
    let session_ids = state.recording_sessions.read().await.clone();
    if session_ids.is_empty() { return; }
    for fill in fills {
        let btc_price = *state.btc_price.read().await;
        let trade_side = match fill.side {
            crate::modules::core::worker::OrderSide::Buy => "buy",
            crate::modules::core::worker::OrderSide::Sell => "sell",
        };
        for &session_id in &session_ids {
            if let Err(e) = session_repo::insert_session_trade(
                state, session_id, &fill.outcome, trade_side, fill.price, fill.size, btc_price,
            ).await { warn!("Session trade #{}: {}", session_id, e); }
        }
    }
}

/// Captura combinada: BOOK_UPDATE y TRADE usan el mismo pipeline CSV + DB (legacy).
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
    state.csv_logger.push(rec.clone());

    // Per-session CSV file (strict isolation — only writes if session is active)
    state.session_manager.push(&rec);

    // Strategy Manager: evaluate both shadow strategies (A: Imbalance, B: Liquidity)
    let result = state.strategy_manager.lock().unwrap().evaluate(&rec);
    let closed = result.imba.status == "CLOSED" || result.liqb.status == "CLOSED";

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
    // If a trade just closed, flush the CSV immediately
    if closed {
        let _ = state.session_manager.flush();
    }

    // In-memory buffer — tag with session_id for filtering
    let session_ids = state.recording_sessions.read().await.clone();
    let active_sid = state.session_manager.active_id()
        .or_else(|| session_ids.first().copied())
        .unwrap_or(0);
    rec.session_id = active_sid;
    state.mem_hft.write().await.push(rec.clone());

    // DB insert (only if PostgreSQL is available) — write to all recording sessions
    if let Some(pool) = state.db.as_ref() {
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
            .execute(pool)
            .await;
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
