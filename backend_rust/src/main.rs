//! Polymarket BTC 15-min — Backend Service
//!
//! Expone:
//!   REST API  → http://0.0.0.0:8080/api/...
//!   WebSocket → ws://0.0.0.0:8080/ws
//!
//! Variables de entorno requeridas (.env):
//!   POLYMARKET_PRIVATE_KEY, CLOB_API_KEY, CLOB_API_SECRET, CLOB_API_PASSPHRASE
//!   DATABASE_URL=postgres://user:pass@host/db
//!
//! Uso: cargo run [--release]

#![allow(dead_code)]

mod modules;

use std::sync::{mpsc, Arc, Mutex};

use tokio::sync::{broadcast, mpsc as tokio_mpsc};
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;
use crate::modules::core::worker::{AppMsg, BtcPriceProvider, CandleInterval, CmdMsg, ConnStatus};
use crate::modules::core::credentials::ClobCredentials;
use crate::modules::core::state::AppState;
use crate::modules::core::persistence as db;

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

    // Credenciales Polymarket
    let creds = match ClobCredentials::from_env() {
        Ok(c) => { info!("Wallet: {}", c.display_address()); Arc::new(c) }
        Err(e) => { error!("Credenciales no disponibles: {:#}", e); return Ok(()); }
    };

    // PostgreSQL (opcional — si DATABASE_URL no está, el backend corre sin DB)
    let db = match std::env::var("DATABASE_URL") {
        Ok(url) => {
            match sqlx::PgPool::connect(&url).await {
                Ok(pool) => {
                    if let Err(e) = db::run_migrations(&pool).await {
                        tracing::warn!("Migraciones DB fallaron: {e}");
                    } else {
                        info!("PostgreSQL OK — migraciones aplicadas");
                    }
                    Some(pool)
                }
                Err(e) => {
                    tracing::warn!("PostgreSQL no disponible ({e}) — arrancando sin DB");
                    None
                }
            }
        }
        Err(_) => {
            info!("DATABASE_URL no configurada — arrancando sin persistencia");
            None
        }
    };

    // Channels
    let (tx, rx)          = mpsc::channel::<AppMsg>();
    let (cmd_tx, cmd_rx)  = tokio_mpsc::unbounded_channel::<CmdMsg>();
    let (bcast_tx, _)     = broadcast::channel::<String>(512);
    let interval_arc      = Arc::new(Mutex::new(CandleInterval::OneMinute));
    let (btc_provider_tx, btc_provider_rx) = tokio::sync::watch::channel(BtcPriceProvider::Binance);
    let btc_provider_tx   = Arc::new(btc_provider_tx);

    // AppState compartido
    let state = AppState::new(
        cmd_tx,
        bcast_tx.clone(),
        Arc::clone(&interval_arc),
        db,
        Arc::clone(&btc_provider_tx),
    );

    // Worker (hilo OS con su propio runtime tokio)
    {
        let tx2           = tx.clone();
        let creds2        = Arc::clone(&creds);
        let interval_arc2 = Arc::clone(&interval_arc);
        let bcast_tx2     = bcast_tx.clone();
        std::thread::Builder::new()
            .name("polymarket-worker".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime worker")
                    .block_on(crate::modules::core::worker::run(tx2, creds2, cmd_rx, interval_arc2, bcast_tx2, btc_provider_rx));
            })
            .expect("spawn worker");
    }

    // Scheduler del módulo DB: snapshots cada 10s + ejecuciones programadas cada 5s
    {
        let state3 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::db::scheduler::run_scheduler(state3).await;
        });
    }

    // Scheduler del módulo Premium Collector (solo si el feature está activo)
    #[cfg(feature = "premium-collector")]
    {
        let state4 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::collector::scheduler::run_collector(state4).await;
        });
    }

    // Scheduler del módulo Pattern Detector (solo si el feature está activo)
    #[cfg(feature = "premium-patterns")]
    {
        let state5 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::patterns::scheduler::run_detector(state5).await;
        });
    }

    // Scheduler del módulo Executor (solo si el feature está activo)
    #[cfg(feature = "premium-executor")]
    {
        let state6 = Arc::clone(&state);
        tokio::spawn(async move {
            crate::modules::premium::executor::scheduler::run_executor(state6).await;
        });
    }

    // Consumer de AppMsg: actualiza estado + persiste en DB + hace broadcast WS
    {
        let state2 = Arc::clone(&state);
        // Bridge std::mpsc → tokio mpsc para poder usarlo en async
        let (bridge_tx, mut bridge_rx) = tokio_mpsc::unbounded_channel::<AppMsg>();
        std::thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                let _ = bridge_tx.send(msg);
            }
        });
        tokio::spawn(async move {
            while let Some(msg) = bridge_rx.recv().await {
                // Broadcast JSON a clientes WS
                if let Some(json) = msg.to_json() {
                    let _ = state2.broadcast_tx.send(json);
                }
                // Actualizar estado en memoria y persistir en DB
                update_state(&msg, &state2).await;
            }
        });
    }

    // Servidor axum
    let addr = "0.0.0.0:8080";
    let app  = crate::modules::core::api::router(Arc::clone(&state));

    info!("============================================");
    info!(" Polymarket BTC 15-min Backend");
    info!(" REST API:  http://{}/api/...", addr);
    info!(" WebSocket: ws://{}/ws", addr);
    info!("============================================");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ─── Consumer de AppMsg ───────────────────────────────────────────────────────

/// Tick counter para muestrear BTC ticks (1 de cada 10 actualizaciones → ~1/s)
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

        AppMsg::BookUp(b)        => {
            *state.book_up.write().await = Some(b.clone());
            capture_book(state, "up", &b.bids, &b.asks).await;
        }
        AppMsg::BookDown(b)      => {
            *state.book_down.write().await = Some(b.clone());
            capture_book(state, "down", &b.bids, &b.asks).await;
        }
        AppMsg::LastTradeUp(p)   => { *state.last_trade_up.write().await   = Some(*p); }
        AppMsg::LastTradeDown(p) => { *state.last_trade_down.write().await  = Some(*p); }
        AppMsg::Balance(b)       => { *state.balance.write().await          = Some(*b); }
        AppMsg::BtcOpen(p)       => { *state.btc_open.write().await         = Some(*p); }

        AppMsg::BtcPrice(p) => {
            *state.btc_price.write().await = Some(*p);
            let n = BTC_TICK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n % 10 == 0 {
                if let Err(e) = db::insert_btc_tick(state.db.as_ref(), *p).await {
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
            capture_fills(state, fills).await;
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
            let interval = state.interval_arc
                .lock()
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

// ─── Session Recorder — Captura por tick ──────────────────────────────────────

use crate::modules::core::worker::PriceLevel;
use crate::modules::db::repository as session_repo;
use serde_json::json;

async fn capture_book(state: &AppState, side: &str, bids: &[PriceLevel], asks: &[PriceLevel]) {
    let session_id = match *state.recording_session.read().await {
        Some(id) => id,
        None => return,
    };

    let best_bid = bids.first().map(|l| l.price);
    let best_bid_sz = bids.first().map(|l| l.size);
    let best_ask = asks.first().map(|l| l.price);
    let best_ask_sz = asks.first().map(|l| l.size);
    let spread = best_bid.and_then(|bb| best_ask.map(|ba| ba - bb));
    let mid_price = best_bid.and_then(|bb| best_ask.map(|ba| (bb + ba) / 2.0));

    // Volume breakdown by depth
    let bid_volume_5: f64 = bids.iter().take(5).map(|l| l.size).sum();
    let ask_volume_5: f64 = asks.iter().take(5).map(|l| l.size).sum();
    let bid_volume_10: f64 = bids.iter().take(10).map(|l| l.size).sum();
    let ask_volume_10: f64 = asks.iter().take(10).map(|l| l.size).sum();
    let bid_volume: f64 = bids.iter().map(|l| l.size).sum();    // ALL levels
    let ask_volume: f64 = asks.iter().map(|l| l.size).sum();    // ALL levels

    // Imbalance ratio: >1 = bid-heavy, <1 = ask-heavy
    let imbalance_ratio = if ask_volume > 0.0 { Some(bid_volume / ask_volume) } else { None };

    // UP/DOWN probability: Polymarket tokens trade 0-1, mid_price ≈ probability
    let up_probability = mid_price;
    let down_probability = mid_price.map(|p| 1.0 - p);

    // Capture FULL order book (all levels) in JSONB
    let depth_bids = json!(bids.iter().map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());
    let depth_asks = json!(asks.iter().map(|l| json!({"p":l.price,"s":l.size})).collect::<Vec<_>>());

    let btc_price = *state.btc_price.read().await;

    if let Err(e) = session_repo::insert_session_snapshot(
        state, session_id, side,
        best_bid, best_bid_sz, best_ask, best_ask_sz, spread, mid_price,
        Some(bid_volume_5), Some(ask_volume_5),
        Some(bid_volume_10), Some(ask_volume_10),
        Some(bid_volume), Some(ask_volume),
        imbalance_ratio, up_probability, down_probability,
        Some(depth_bids), Some(depth_asks),
        btc_price,
    ).await {
        warn!("Session snapshot capture: {}", e);
    }
}

async fn capture_fills(state: &AppState, fills: &[crate::modules::core::worker::RecentFill]) {
    let session_id = match *state.recording_session.read().await {
        Some(id) => id,
        None => return,
    };

    for fill in fills {
        // Read BTC price per-fill to avoid stale batch prices (BUG FIX)
        let btc_price = *state.btc_price.read().await;
        let trade_side = match fill.side {
            crate::modules::core::worker::OrderSide::Buy => "buy",
            crate::modules::core::worker::OrderSide::Sell => "sell",
        };

        if let Err(e) = session_repo::insert_session_trade(
            state, session_id, &fill.outcome, trade_side, fill.price, fill.size, btc_price,
        ).await {
            warn!("Session trade capture: {}", e);
        }
    }
}
