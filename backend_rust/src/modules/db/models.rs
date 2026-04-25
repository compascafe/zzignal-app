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
