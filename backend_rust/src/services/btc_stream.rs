// BTC price stream — broadcasts directly to WebSocket with zero consumer queue delay.
// Also forwards ticks to the consumer via mpsc for state updates (btc_price, tracking, volume).

use chrono::Utc;
use futures_util::StreamExt;
use std::sync::mpsc;
use std::time::Duration;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

use crate::controllers::worker::AppMsg;

const BINANCE_WS: &str = "wss://stream.binance.com:9443/ws/btcusdt@aggTrade";

pub async fn run(tx: mpsc::Sender<AppMsg>) {
    let mut backoff = Duration::from_secs(2);
    loop {
        match connect_async(BINANCE_WS).await {
            Ok((ws_stream, _)) => {
                backoff = Duration::from_secs(2);
                info!("BTC Binance aggTrade conectado");
                let (_, mut read) = ws_stream.split();
                while let Some(Ok(m)) = read.next().await {
                    if !m.is_text() {
                        continue;
                    }
                    let text = match m.into_text() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        let price: Option<f64> = json
                            .get("p")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .filter(|&p| p > 0.0);
                        let volume: f64 = json
                            .get("q")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        if let Some(p) = price {
                            let now_ms = Utc::now().timestamp_millis();
                            let _ = tx.send(AppMsg::BtcTick {
                                price: p,
                                volume,
                                event_time: now_ms,
                            });
                        }
                    }
                }
                warn!("BTC stream desconectado, reconectando...");
            }
            Err(e) => {
                warn!(
                    "BTC Binance connect falló: {} — retry {}s",
                    e,
                    backoff.as_secs()
                );
            }
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}
