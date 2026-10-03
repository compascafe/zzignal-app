use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

use crate::controllers::worker::PriceLevel;
use crate::models::hft::{BinanceDepth, PriceRingBuffer};

const BINANCE_COMBINED_WS: &str =
    "wss://stream.binance.com:9443/stream?streams=btcusdt@depth20@100ms/btcusdt@ticker";

/// If Binance delivers no message for this duration, force disconnect + reconnect.
const BINANCE_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Evento de tick de Binance (precio + timestamp) para el pipeline unificado.
#[derive(Debug, Clone)]
pub struct BinanceTickEvent {
    pub price: f64,
    pub event_time: i64,
}

/// Tarea de fondo: conecta al WebSocket combinado de Binance.
/// - Escribe snapshot completo en `depth`
/// - Pushea estado compacto en `ring` (lock-free)
/// - Emite BINANCE_TICK a `tick_tx` cuando el precio cambia > $0.15
pub async fn run_binance_depth_stream(
    depth: Arc<RwLock<Option<BinanceDepth>>>,
    ring: Arc<PriceRingBuffer>,
    tick_tx: mpsc::UnboundedSender<BinanceTickEvent>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut backoff = Duration::from_secs(2);

    loop {
        info!("Conectando Binance depth stream...");

        match connect_async(BINANCE_COMBINED_WS).await {
            Ok((ws_stream, _)) => {
                backoff = Duration::from_secs(2);
                info!("Binance depth stream conectado");

                let (_, mut read) = ws_stream.split();
                let mut last_depth: Option<BinanceDepth> = None;
                let mut last_tick_price: f64 = 0.0;

                loop {
                    let msg_result = tokio::select! {
                        msg = async {
                            match tokio::time::timeout(BINANCE_READ_TIMEOUT, read.next()).await {
                                Ok(inner) => inner,
                                Err(_elapsed) => {
                                    warn!("Binance depth stream: read timeout {}s — forcing reconnect",
                                        BINANCE_READ_TIMEOUT.as_secs());
                                    None // triggers break below
                                }
                            }
                        } => msg,
                        _ = shutdown.recv() => {
                            info!("Binance depth stream: shutdown signal");
                            return;
                        }
                    };

                    match msg_result {
                        Some(Ok(msg)) => {
                            let text = match msg.into_text() {
                                Ok(t) => t,
                                Err(_) => continue,
                            };

                            let now_ms = Utc::now().timestamp_millis();

                            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                                continue;
                            };

                            let stream = json.get("stream").and_then(|s| s.as_str()).unwrap_or("");

                            match stream {
                                "btcusdt@ticker" => {
                                    let data = match json.get("data") {
                                        Some(d) => d,
                                        None => continue,
                                    };
                                    let event_time =
                                        data.get("E").and_then(|v| v.as_i64()).unwrap_or(0);
                                    let btc_price = data
                                        .get("c")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(0.0);
                                    let btc_volume_24h = data
                                        .get("v")
                                        .and_then(|v| v.as_str())
                                        .and_then(|s| s.parse::<f64>().ok())
                                        .unwrap_or(0.0);

                                    if let Some(ref mut d) = last_depth {
                                        d.event_time = event_time;
                                        d.btc_price = btc_price;
                                        d.btc_volume_24h = btc_volume_24h;
                                        d.local_time = now_ms;
                                    }

                                    // Emitir BINANCE_TICK si delta > $0.15
                                    if (btc_price - last_tick_price).abs() > 0.15
                                        && last_depth.is_some()
                                    {
                                        let tick = BinanceTickEvent {
                                            price: btc_price,
                                            event_time,
                                        };
                                        last_tick_price = btc_price;
                                        let _ = tick_tx.send(tick);
                                    } else if last_tick_price == 0.0 {
                                        last_tick_price = btc_price;
                                    }
                                }

                                "btcusdt@depth20" | "btcusdt@depth20@100ms" => {
                                    let data = match json.get("data") {
                                        Some(d) => d,
                                        None => continue,
                                    };

                                    let parse_levels =
                                        |arr: &serde_json::Value| -> Vec<PriceLevel> {
                                            arr.as_array()
                                                .iter()
                                                .flat_map(|a| a.iter())
                                                .filter_map(|entry| {
                                                    let pair = entry.as_array()?;
                                                    let price = pair
                                                        .first()?
                                                        .as_str()?
                                                        .parse::<f64>()
                                                        .ok()?;
                                                    let size = pair
                                                        .get(1)?
                                                        .as_str()?
                                                        .parse::<f64>()
                                                        .ok()?;
                                                    Some(PriceLevel { price, size })
                                                })
                                                .take(20)
                                                .collect()
                                        };

                                    let bids = data.get("bids").map_or(vec![], &parse_levels);
                                    let asks = data.get("asks").map_or(vec![], parse_levels);

                                    let current = BinanceDepth {
                                        bids: bids.clone(),
                                        asks: asks.clone(),
                                        event_time: last_depth.as_ref().map_or(0, |d| d.event_time),
                                        local_time: now_ms,
                                        btc_price: last_depth.as_ref().map_or(0.0, |d| d.btc_price),
                                        btc_volume_24h: last_depth
                                            .as_ref()
                                            .map_or(0.0, |d| d.btc_volume_24h),
                                    };

                                    *depth.write().await = Some(current.clone());

                                    // Push compacto al ring buffer (lock-free)
                                    let bb_bid = bids.first().map(|l| l.price).unwrap_or(0.0);
                                    let bb_ask = asks.first().map(|l| l.price).unwrap_or(0.0);
                                    let mid_p = if bb_bid > 0.0 && bb_ask > 0.0 {
                                        (bb_bid + bb_ask) / 2.0
                                    } else {
                                        0.0
                                    };
                                    // Local time for accurate look-back
                                    ring.push(now_ms as u64, mid_p);

                                    last_depth = Some(current);
                                }

                                _ => {}
                            }
                        }
                        Some(Err(e)) => {
                            warn!("Binance depth WS error: {}", e);
                            break;
                        }
                        None => break,
                    }
                }

                warn!("Binance depth stream desconectado, reconectando...");
            }
            Err(e) => {
                warn!(
                    "Binance depth connect falló: {} — reintento en {}s",
                    e,
                    backoff.as_secs()
                );
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}
