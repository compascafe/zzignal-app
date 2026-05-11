use chrono::Local;
use serde::Deserialize;

pub const WS_URL: &str = "ws://localhost:8080/ws";
pub const API_URL: &str = "http://localhost:8080";

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

// ─── HTTP Helpers ───────────────────────────────────────────────────

use std::time::Duration;

fn client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    })
}

pub async fn http_get<T: for<'de> Deserialize<'de>>(path: &str) -> Option<T> {
    let url = format!("{API_URL}{path}");
    client().get(&url).send().await.ok()?.json::<T>().await.ok()
}

pub async fn http_post(path: &str, body: &str) -> Result<(), String> {
    let resp = client()
        .post(format!("{API_URL}{path}"))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send().await
        .map_err(|e| format!("POST {path}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("POST {path} → HTTP {}", resp.status().as_u16()));
    }
    Ok(())
}
