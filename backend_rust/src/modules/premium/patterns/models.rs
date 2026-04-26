use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tipo de patrón detectado en el order book
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PatternType {
    /// Una orden grande (wall) aparece en el book
    Wall { side: String },
    /// El spread se ensancha repentinamente (> 2 desviaciones estándar)
    SpreadAnomaly,
    /// Desbalance entre bid volume y ask volume
    DepthImbalance { bias: String },  // "bid" | "ask"
    /// Una orden grande desaparece (spoofing)
    Spoof,
    /// El mid price cruza una media móvil con momentum
    MomentumShift { direction: String },
    /// Velocidad de cambio del order book (muchas actualizaciones en poco tiempo)
    HighActivity,
    /// El book se queda sin liquidez en un lado
    LiquidityVacuum { side: String },
}

impl PatternType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PatternType::Wall { .. }        => "wall",
            PatternType::SpreadAnomaly       => "spread_anomaly",
            PatternType::DepthImbalance { .. } => "depth_imbalance",
            PatternType::Spoof              => "spoof",
            PatternType::MomentumShift { .. } => "momentum_shift",
            PatternType::HighActivity       => "high_activity",
            PatternType::LiquidityVacuum { .. } => "liquidity_vacuum",
        }
    }
}

/// Una señal generada por el detector de patrones
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Signal {
    pub id:          i64,
    pub pattern:     String,
    pub side:        String,       // "up" | "down"
    pub severity:    String,       // "low" | "medium" | "high"
    pub description: String,
    pub data:        serde_json::Value,  // detalles del patrón (prices, sizes, etc.)
    pub ts:          DateTime<Utc>,
    pub created_at:  DateTime<Utc>,
}

/// Alerta que se envía al frontend en tiempo real
#[derive(Debug, Clone, Serialize)]
pub struct Alert {
    pub pattern:     String,
    pub side:        String,
    pub severity:    String,
    pub message:     String,
    pub price:       f64,
    pub ts:          DateTime<Utc>,
}

/// Configuración del detector (qué patrones buscar y con qué sensibilidad)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    pub wall_threshold:        f64,  // tamaño mínimo para considerar un wall
    pub spread_std_multiplier: f64,  // cuántas desviaciones para anomalía de spread
    pub imbalance_ratio:       f64,  // ratio bid/ask para considerar desbalance
    pub spoof_lookback_secs:   i64,  // ventana para detectar spoofing
    pub momentum_window:       i64,  // ticks para momentum shift
    pub min_activity_ticks:    i32,  // ticks/segundo para high activity
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            wall_threshold:        1000.0,
            spread_std_multiplier: 2.0,
            imbalance_ratio:       3.0,
            spoof_lookback_secs:   5,
            momentum_window:       10,
            min_activity_ticks:    20,
        }
    }
}
