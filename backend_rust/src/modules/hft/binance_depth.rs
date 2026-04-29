use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::StreamExt;
use tokio::sync::{RwLock, broadcast};
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

use crate::modules::core::worker::PriceLevel;
use crate::modules::hft::types::BinanceDepth;

/// Binance combined stream: depth20@100ms + ticker en una sola conexión WebSocket.
///
/// Formato de mensajes recibidos:
/// ```json
/// {"stream":"btcusdt@depth20@100ms","data":{"lastUpdateId":...,"bids":[...],"asks":[...]}}
/// {"stream":"btcusdt@ticker","data":{"e":"24hrTicker","E":...,"c":"...","v":"..."}}
/// ```
const BINANCE_COMBINED_WS: &str =
    "wss://stream.binance.com:9443/stream?streams=btcusdt@depth20@100ms/btcusdt@ticker";

/// Tarea de fondo: conecta al WebSocket combinado de Binance y mantiene
/// actualizado `state.binance_depth` con el último snapshot de profundidad.
pub async fn run_binance_depth_stream(
    depth:  Arc<RwLock<Option<BinanceDepth>>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    let mut backoff = Duration::from_secs(2);

    loop {
        info!("Conectando Binance depth stream ({})...", BINANCE_COMBINED_WS);

        match connect_async(BINANCE_COMBINED_WS).await {
            Ok((ws_stream, _)) => {
                backoff = Duration::from_secs(2);
                info!("Binance depth stream conectado");

                let (_, mut read) = ws_stream.split();
                let mut last_depth: Option<BinanceDepth> = None;

                loop {
                    let msg_result = tokio::select! {
                        msg = read.next() => msg,
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

                            let stream = json
                                .get("stream")
                                .and_then(|s| s.as_str())
                                .unwrap_or("");

                            match stream {
                                "btcusdt@ticker" => {
                                    let data = match json.get("data") {
                                        Some(d) => d,
                                        None => continue,
                                    };
                                    let event_time = data
                                        .get("E")
                                        .and_then(|v| v.as_i64())
                                        .unwrap_or(0);
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
                                }

                                "btcusdt@depth20" | "btcusdt@depth20@100ms" => {
                                    let data = match json.get("data") {
                                        Some(d) => d,
                                        None => continue,
                                    };

                                    let last_update_id = data
                                        .get("lastUpdateId")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);

                                    let parse_levels = |arr: &serde_json::Value| -> Vec<PriceLevel> {
                                        arr.as_array()
                                            .iter()
                                            .flat_map(|a| a.iter())
                                            .filter_map(|entry| {
                                                let pair = entry.as_array()?;
                                                let price = pair.get(0)?.as_str()?.parse::<f64>().ok()?;
                                                let size  = pair.get(1)?.as_str()?.parse::<f64>().ok()?;
                                                Some(PriceLevel { price, size })
                                            })
                                            .take(20)
                                            .collect()
                                    };

                                    let bids = data.get("bids").map_or(vec![], |b| parse_levels(b));
                                    let asks = data.get("asks").map_or(vec![], |a| parse_levels(a));

                                    let current = BinanceDepth {
                                        last_update_id,
                                        bids,
                                        asks,
                                        event_time:     last_depth.as_ref().map_or(0, |d| d.event_time),
                                        local_time:     now_ms,
                                        btc_price:      last_depth.as_ref().map_or(0.0, |d| d.btc_price),
                                        btc_volume_24h: last_depth.as_ref().map_or(0.0, |d| d.btc_volume_24h),
                                    };

                                    *depth.write().await = Some(current.clone());
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
                warn!("Binance depth connect falló: {} — reintento en {}s", e, backoff.as_secs());
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}
