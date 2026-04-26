use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Una regla de condición: IF <metric> <operator> <value>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub metric:   String,  // "spread", "mid_price", "bid_volume", "ask_volume",
                           // "btc_price", "imbalance", "wall_price", "wall_size"
    pub operator: String,  // "gt", "lt", "gte", "lte", "eq", "cross_above", "cross_below"
    pub value:    f64,     // threshold
    pub side:     String,  // "up" | "down" | "both"
}

/// Acción a ejecutar cuando las condiciones se cumplen
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub order_type:  String,  // "limit" | "market" | "scalp"
    pub outcome:     String,  // "up" | "down"
    pub price:       Option<f64>,
    pub size:        Option<f64>,
    pub amount_usdc: Option<f64>,
    pub target_price:Option<f64>,
}

/// Una estrategia: un conjunto de condiciones + una acción
/// conditions y action se almacenan como JSONB en PostgreSQL.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Strategy {
    pub id:               i64,
    pub name:             String,
    pub description:      String,
    pub enabled:          bool,
    pub conditions:       serde_json::Value,
    pub action:           serde_json::Value,
    pub cooldown_secs:    i32,
    pub max_positions:    i32,
    pub max_size_total:   f64,
    pub stop_loss_pct:    f64,
    pub take_profit_pct:  f64,
    pub created_at:       DateTime<Utc>,
    pub last_executed_at: Option<DateTime<Utc>>,
}

impl Strategy {
    pub fn conditions_parsed(&self) -> Vec<Condition> {
        serde_json::from_value(self.conditions.clone()).unwrap_or_default()
    }

    pub fn action_parsed(&self) -> Action {
        serde_json::from_value(self.action.clone()).unwrap_or(Action {
            order_type: "limit".into(),
            outcome: "up".into(),
            price: None,
            size: None,
            amount_usdc: None,
            target_price: None,
        })
    }
}

/// Registro de ejecución de una estrategia
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ExecutionLog {
    pub id:          i64,
    pub strategy_id: i64,
    pub strategy_name: String,
    pub outcome:     String,
    pub side:        String,
    pub order_type:  String,
    pub price:       Option<f64>,
    pub size:        Option<f64>,
    pub result:      String,       // "success" | "failed" | "skipped"
    pub reason:      Option<String>,
    pub btc_price:   Option<f64>,
    pub ts:          DateTime<Utc>,
}

/// Nueva estrategia (payload de creación)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewStrategy {
    pub name:             String,
    pub description:      String,
    pub conditions:       Vec<Condition>,
    pub action:           Action,
    pub cooldown_secs:    Option<i32>,
    pub max_positions:    Option<i32>,
    pub max_size_total:   Option<f64>,
    pub stop_loss_pct:    Option<f64>,
    pub take_profit_pct:  Option<f64>,
}
