use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Snapshot del order book (UP o DOWN) — lo que guarda el scheduler
#[derive(Debug, Clone, Serialize)]
pub struct OrderBookSnapshotData {
    pub side:        String,
    pub best_bid:    Option<f64>,
    pub best_bid_sz: Option<f64>,
    pub best_ask:    Option<f64>,
    pub best_ask_sz: Option<f64>,
    pub spread:      Option<f64>,
    pub depth_bids:  Vec<PriceLevelData>,
    pub depth_asks:  Vec<PriceLevelData>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PriceLevelData {
    pub price: f64,
    pub size:  f64,
}

/// Ejecución programada (limit/market/scalp) para el scheduler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledExecution {
    pub scheduled_at: DateTime<Utc>,
    pub side:         String,   // "buy" | "sell"
    pub outcome:      String,   // "up" | "down"
    pub order_type:   String,   // "limit" | "market" | "scalp"
    pub price:        Option<f64>,
    pub size:         Option<f64>,
    pub amount_usdc:  Option<f64>,
    pub target_price: Option<f64>,
    pub notes:        Option<String>,
}

// ─── Session Recorder ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSession {
    pub name:            String,
    pub scheduled_start: Option<DateTime<Utc>>,
    pub scheduled_end:   Option<DateTime<Utc>>,
    pub duration_min:    i32,
    pub depth_levels:    i32,
    pub indefinite:      Option<bool>,  // true = sin fecha de fin, se detiene manualmente
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RecordingSession {
    pub id:              i32,
    pub name:            String,
    pub scheduled_start: DateTime<Utc>,
    pub scheduled_end:   DateTime<Utc>,
    pub started_at:      Option<DateTime<Utc>>,
    pub stopped_at:      Option<DateTime<Utc>>,
    pub duration_min:    i32,
    pub market_id:       Option<String>,
    pub market_title:    Option<String>,
    pub capture_mode:    String,
    pub depth_levels:    i32,
    pub strike_price:    Option<f64>,
    pub final_price:     Option<f64>,
    pub outcome_result:  Option<String>,
    pub btc_price_start: Option<f64>,
    pub btc_price_end:   Option<f64>,
    pub status:          String,
    pub tick_count:      i32,
    pub trade_count:     i32,
    pub created_at:      DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SessionSnapshot {
    pub id:          i32,
    pub session_id:  i32,
    pub ts:          DateTime<Utc>,
    pub side:        String,
    pub best_bid:    Option<f64>,
    pub best_bid_sz: Option<f64>,
    pub best_ask:    Option<f64>,
    pub best_ask_sz: Option<f64>,
    pub spread:      Option<f64>,
    pub mid_price:   Option<f64>,
    pub bid_volume:  Option<f64>,
    pub ask_volume:  Option<f64>,
    pub depth_bids:  Option<serde_json::Value>,
    pub depth_asks:  Option<serde_json::Value>,
    pub btc_price:   Option<f64>,
    pub created_at:  DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SessionTrade {
    pub id:          i32,
    pub session_id:  i32,
    pub ts:          DateTime<Utc>,
    pub side:        String,
    pub trade_side:  String,
    pub price:       f64,
    pub size:        f64,
    pub btc_price:   Option<f64>,
    pub created_at:  DateTime<Utc>,
}
