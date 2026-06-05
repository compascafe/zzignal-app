use chrono::Local;
use serde::Deserialize;

pub const WS_URL: &str = "ws://localhost:8080/ws";
pub const API_URL: &str = "http://localhost:8080";

#[derive(Debug, Clone, Deserialize)]
pub struct OrderPlaced {
    #[serde(alias = "order_id", alias = "orderID", default)]
    pub id: String,
    #[serde(default)]
    pub price: f64,
    #[serde(default)]
    pub size: f64,
} impl OrderPlaced {
    pub fn ok_id(&self) -> Option<&str> {
        if self.id.is_empty() { None } else { Some(&self.id) }
    }
}

// ─── Orderbook Depth ──────────────────────────────────────────────

#[derive(Debug, Deserialize, Default, Clone)]
pub struct BookLevel {
    pub price: f64,
    pub size: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct BookDepth {
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

// ─── WebSocket Messages ─────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct WsMsg {
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub balance: Option<f64>,
    pub status: Option<String>,
    pub btc: Option<f64>,
    pub price: Option<f64>,
    pub success: Option<bool>,
    pub message: Option<String>,
    /// Real-time HFT state (broadcast in-band, ~every tick)
    #[serde(default)]
    pub data: Option<HftState>,
    /// Orderbook snapshot (broadcast from CLOB WS)
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub book: Option<BookDepth>,
    #[serde(default)]
    pub provider: Option<String>,
}

// ─── REST Response Types ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OdiseoStatus {
    pub live_mode: bool,
    pub reinvest: Option<bool>,
    pub variants: Vec<OdiseoVariant>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct OdiseoVariant {
    pub enabled: Option<bool>,
    pub name: Option<String>,
    pub code: Option<String>,
    #[serde(default)] pub budget: f64,
    #[serde(default)] pub total_pnl: f64,
    #[serde(default)] pub balance: f64,
    #[serde(default)] pub trades_up: i64, #[serde(default)] pub trades_dn: i64,
    #[serde(default)] pub wins_up: i64, #[serde(default)] pub wins_dn: i64,
    #[serde(default)] pub tp_up: i64, #[serde(default)] pub tp_dn: i64,
    #[serde(default)] pub sl_up: i64, #[serde(default)] pub sl_dn: i64,
    #[serde(default)] pub sessions: i64,
    #[serde(default)] pub accuracy: f64, #[serde(default)] pub avg_pnl: f64,
    #[serde(default)] pub best: f64, #[serde(default)] pub worst: f64,
}

#[derive(Debug, Deserialize, Default)]
pub struct BtcInfo {
    pub price: f64,
    pub open: f64,
}

#[derive(Debug, Deserialize)]
pub struct BtcProviderInfo {
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct HftState {
    pub time: String,
    pub event: String,
    pub btc_price: f64,
    pub mid: f64,
    pub spread: f64,
    pub imbalance: f64,
    pub bid_vol: f64,
    pub ask_vol: f64,
    pub clob_trade_up: f64,
    pub clob_trade_dn: f64,
    pub clob_trade_up_vol: f64,
    pub clob_trade_dn_vol: f64,
    pub btc_vel: f64,
    pub btc_acel: f64,
    pub btc_volatility: f64,
    pub btc_volume_24h: f64,
    pub btc_vol_1m: f64,       // real-time BTC volume in last 60s (aggTrade per-tick sum)
    pub btc_vol_ses: f64,      // cumulative real BTC volume since session start
    pub btc_vol: f64,           // latest per-tick BTC volume from aggTrade
    pub spoof: u8,
    pub dump_score: u8,
    pub ask_wall: u8,
    pub tick_gap_ms: i64,
    pub secs_left: i32,
    pub od83_up: u8,
    pub od83_dn: u8,
    pub hd65_up: u8,
    pub hd65_dn: u8,
    pub od83_up_bal: f64,
    pub od83_dn_bal: f64,
    pub hd65_up_bal: f64,
    pub hd65_dn_bal: f64,
    pub od83_filters: u16,
    pub od83_event: String,
    pub hd65_event: String,
    pub sen_up: u8,
    pub sen_dn: u8,
    pub sen_up_bal: f64,
    pub sen_dn_bal: f64,
    pub sen_event: String,
    pub sen_clob_delta: f64,
    pub sen_btc_vel: f64,
    pub depth_up_bids: Vec<(f64,f64)>,
    pub depth_up_asks: Vec<(f64,f64)>,
    pub depth_dn_bids: Vec<(f64,f64)>,
    pub depth_dn_asks: Vec<(f64,f64)>,
    #[serde(default)]
    pub ofi_up: f64,
    #[serde(default)]
    pub ofi_dn: f64,
    #[serde(default)]
    pub micro_price_up: f64,
    #[serde(default)]
    pub micro_price_dn: f64,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct SessionInfo {
    pub id: i32,
    pub name: String,
    pub status: String,
    pub scheduled_start: String,
    pub scheduled_end: String,
    pub duration_min: i32,
    pub tick_count: i32,
    pub trade_count: i32,
}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct HealthInfo {
    pub status: String,
    pub btc: f64,
    pub balance: f64,
}

// ─── Log Entry ──────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LogEntry {
    pub ts: String,
    pub text: String,
    pub color: ratatui::style::Color,
}

impl LogEntry {
    pub fn new(text: String, color: ratatui::style::Color) -> Self {
        Self { ts: Local::now().format("%H:%M:%S").to_string(), text, color }
    }
}

// ─── Trade Entry (for TAP / Time & Sales) ───────────────────────────

#[derive(Clone)]
pub struct TradeEntry {
    pub ts: String,
    pub side: String,   // "UP" or "DOWN"
    pub price: f64,
    pub size: f64,
}

// ─── Order Tracking ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct OrderInfo {
    #[allow(dead_code)]
    pub id: String,
    pub outcome: String,
    pub side: String,
    pub price: f64,
    pub size_orig: f64,
    pub size_matched: f64,
}

impl OrderInfo {
    pub fn is_filled(&self) -> bool { self.size_matched >= self.size_orig }
    pub fn is_partial(&self) -> bool { self.size_matched > 0.0 && self.size_matched < self.size_orig }
}

// ─── HTTP Helpers ───────────────────────────────────────────────────

use std::time::Duration;

fn client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .connect_timeout(Duration::from_secs(1))
            .pool_max_idle_per_host(0)
            .build()
            .unwrap()
    })
}

/// Wraps an async future with a tokio timeout, returning None on timeout.
/// Safety net: prevents any HTTP call from blocking the render loop indefinitely.
async fn with_timeout<T, F>(future: F, label: &str) -> Option<T>
where
    F: std::future::Future<Output = T>,
{
    match tokio::time::timeout(Duration::from_secs(2), future).await {
        Ok(v) => Some(v),
        Err(_) => {
            // Silent — timeout prevents UI freeze
            let _ = label; // could log if needed
            None
        }
    }
}

pub async fn http_get<T: for<'de> Deserialize<'de>>(path: &str) -> Option<T> {
    let url = format!("{API_URL}{path}");
    with_timeout(
        async {
            client().get(&url).send().await.ok()?.json::<T>().await.ok()
        },
        path,
    ).await.flatten()
}

pub async fn http_post(path: &str, body: &str) -> Result<(), String> {
    let url = format!("{API_URL}{path}");
    let fut = async {
        let resp = client()
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send().await
            .map_err(|e| format!("POST {path}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("POST {path} → HTTP {}", resp.status().as_u16()));
        }
        Ok(())
    };
    match with_timeout(fut, path).await {
        Some(result) => result,
        None => Err(format!("POST {path}: timeout")),
    }
}

pub async fn http_post_json<T: for<'de> Deserialize<'de>>(path: &str, body: &str) -> Option<T> {
    let url = format!("{API_URL}{path}");
    with_timeout(
        async {
            let resp = client()
                .post(&url)
                .header("Content-Type", "application/json")
                .body(body.to_string())
                .send().await.ok()?;
            if resp.status().is_success() {
                resp.json::<T>().await.ok()
            } else {
                None
            }
        },
        path,
    ).await.flatten()
}

pub async fn http_post_result<T: for<'de> Deserialize<'de>>(path: &str, body: &str) -> Result<T, String> {
    let url = format!("{API_URL}{path}");
    let fut = async {
        let resp = client()
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send().await
            .map_err(|e| format!("POST {path}: {e}"))?;
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if status.is_success() {
            serde_json::from_str::<T>(&body_text).map_err(|e| format!("JSON parse: {e}"))
        } else {
            let msg = serde_json::from_str::<serde_json::Value>(&body_text)
                .ok()
                .and_then(|v| v.get("message").or_else(|| v.get("error")).and_then(|m| m.as_str()).map(|s| s.to_string()))
                .unwrap_or(body_text);
            Err(format!("HTTP {}: {}", status.as_u16(), msg))
        }
    };
    match with_timeout(fut, path).await {
        Some(result) => result,
        None => Err(format!("POST {path}: timeout")),
    }
}

pub async fn http_delete(path: &str) -> Result<(), String> {
    let url = format!("{API_URL}{path}");
    let fut = async {
        let resp = client()
            .delete(&url)
            .send().await
            .map_err(|e| format!("DELETE {path}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("DELETE {path} → HTTP {}", resp.status().as_u16()));
        }
        Ok(())
    };
    match with_timeout(fut, path).await {
        Some(result) => result,
        None => Err(format!("DELETE {path}: timeout")),
    }
}
