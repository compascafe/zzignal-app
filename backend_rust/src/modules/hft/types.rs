use serde::Serialize;
use crate::modules::core::worker::PriceLevel;

pub use crate::modules::hft::ring_buffer::{BinanceState, PriceRingBuffer};

/// Snapshot completo del order book de Binance (top 20 niveles)
#[derive(Debug, Clone)]
pub struct BinanceDepth {
    pub last_update_id: u64,
    pub bids:           Vec<PriceLevel>,
    pub asks:           Vec<PriceLevel>,
    pub event_time:     i64,   // Binance E (ms)
    pub local_time:     i64,   // nuestra máquina (ms)
    pub btc_price:      f64,   // último precio del ticker
    pub btc_volume_24h: f64,   // volumen 24h del ticker
}

impl Default for BinanceDepth {
    fn default() -> Self {
        Self {
            last_update_id: 0,
            bids: vec![],
            asks: vec![],
            event_time: 0,
            local_time: 0,
            btc_price: 0.0,
            btc_volume_24h: 0.0,
        }
    }
}

/// Tipo de evento en el pipeline unificado
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum EventType {
    BookUpdate,
    Trade,
    BinanceTick,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BookUpdate => "BOOK_UPDATE",
            Self::Trade => "TRADE",
            Self::BinanceTick => "BINANCE_TICK",
        }
    }
}

/// Registro unificado para CSV de 20 columnas (ordenado según especificación).
#[derive(Debug, Clone)]
pub struct CsvRecord {
    pub ts_local:            String,
    pub ts_exchange:         String,
    pub event_type:          EventType,
    pub latencia_ms:         i64,
    pub binance_price:       f64,
    pub binance_micro_price: f64,
    pub binance_imbalance:   f32,
    pub binance_vol_100ms:   f64,
    pub binance_vol_24h:     f64,
    pub poly_bid:            f64,
    pub poly_ask:            f64,
    pub poly_mid:            f64,
    pub poly_spread:         f64,
    pub poly_bid_vol_all:    f64,
    pub poly_ask_vol_all:    f64,
    pub poly_imbalance:      f64,
    pub trade_side:          String,
    pub trade_price:         f64,
    pub trade_size:          f64,
    pub is_informed:         u8,
    /// Session ID — para filtrar mem_hft y evitar fuga de datos entre sesiones.
    pub session_id:          i32,
    // ─── Strategy A: Imbalance Divergence ────────────────────────────────
    pub imba_status:          String,  // IDLE|OPEN|CLOSED
    pub imba_side:            String,  // BUY|SELL
    pub imba_entry_price:     f64,
    pub imba_exit_price:      f64,
    pub imba_trade_pnl:       f64,
    pub imba_balance:         f64,
    // ─── Strategy B: Liquidity Grabbing ──────────────────────────────────
    pub liqb_status:          String,  // IDLE|OPEN|CLOSED
    pub liqb_side:            String,  // BUY|SELL
    pub liqb_entry_price:     f64,
    pub liqb_exit_price:      f64,
    pub liqb_trade_pnl:       f64,
    pub liqb_balance:         f64,
    // ─── Advanced HFT Metrics ────────────────────────────────────────────
    pub trades_per_second:    f64,  // Binance TRADE count in last rolling 1s
    pub price_velocity:       f64,  // Δprice/Δtime over 500ms window (USD/s)
    pub poly_liquidity_delta: f64,  // Δpoly_ask_vol_all vs previous tick
    pub absorption_ratio:     f64,  // trade_vol / |Δprice| — high = absorption
    pub price_gap_ratio:      f64,  // (binance_micro - poly_mid) / binance_micro * 100
    pub spoofing_flag:        u8,   // 1 = >30% vol drop with no poly trade
    pub tape_speed_flag:      u8,   // 1 = high volume spike detected
    pub gap_alert_flag:       u8,   // 1 = price gap > 0.05%
    // ─── HFT Bollinger Bands & Confluence ─────────────────────────────────
    pub bollinger_sma:         f64,   // SMA(200) of Binance mid prices
    pub bollinger_upper:       f64,   // SMA + 2σ
    pub bollinger_lower:       f64,   // SMA - 2σ
    pub mean_reversion_signal: u8,    // 0=none, 1=Short (touch upper+neg imb), 2=Long (touch lower+pos imb)
    pub technical_confluence:  u8,    // 1 = all confluence conditions met
    pub trend_direction:       i8,    // -1=below SMA, 0=at SMA, 1=above SMA
    pub signal_label:          String,// e.g. 'BOLLINGER_TOUCH', 'VOL_CONFLUENCE', 'GAP_ARBITRAGE', ''
    // ─── Volatility & Master Signal ───────────────────────────────────────
    pub realized_volatility:   f64,   // rolling std dev of last 200 binance prices (σ)
    pub high_volatility_event: u8,    // 1 = current vol > 2x session average vol
    pub bollinger_position:    i8,    // 1=above upper, -1=below lower, 0=inside bands
    pub master_signal:         u8,    // 1=Buy (lower touch+imb>0.8+vel>0), 2=Sell (upper touch+imb<-0.8+vel<0)
    // ─── Conformal Prediction Risk Validation ─────────────────────────────
    pub cp_uncertainty_range:  f64,   // width of 95% confidence interval (USD)
    pub cp_valid_signal:       u8,    // 1 = signal passes CP validation, 0 = blocked (too erratic)
    // ─── Adaptive Risk Engine: Macro 24h + Feedback ───────────────────────
    pub macro_slope:           f64,   // SMA200 slope via linear regression on last 50 points
    pub vfi_value:             f64,   // Volume Flow Indicator (VFI) current value
    pub macd_hist:             f64,   // MACD(3,10,16) histogram value
    pub predicted_bias:        String,// "UP" or "DOWN" — initial bias from macro warm-up
    pub is_feedback_adjusted:  u8,    // 1 = CP intervals widened due to low accuracy feedback
    pub dynamic_rsi:            f64,   // Rolling RSI(14) updated each minute during session
    pub vfi_confidence:         f64,   // VFI volume strength ratio (0-1 normalized)
    pub db_accuracy_factor:     f64,   // Risk multiplier from historical memory (1.0 = neutral, >1 = widen)
    // ─── T-5 Certainty Strategy (Wisdom v2) ─────────────────────────────────
    pub t5_prediction:          String,// "UP", "DOWN", or "" — T-5 prediction
    pub t5_entry_price:         f64,   // price at T-5 capture point
    pub t5_correct:             u8,    // 1 = correct, filled at session close
    // ─── T-3 Aggressive Strategy (Wisdom v3) ───────────────────────────────
    pub t3_prediction:          String,
    pub t3_entry_price:         f64,
    pub t3_active:              u8,    // 1 = trade active
}

impl Default for CsvRecord {
    fn default() -> Self {
        Self {
            ts_local:            String::new(),
            ts_exchange:         String::new(),
            event_type:          EventType::BookUpdate,
            latencia_ms:         0,
            binance_price:       0.0,
            binance_micro_price: 0.0,
            binance_imbalance:   0.0,
            binance_vol_100ms:   0.0,
            binance_vol_24h:     0.0,
            poly_bid:            0.0,
            poly_ask:            0.0,
            poly_mid:            0.0,
            poly_spread:         0.0,
            poly_bid_vol_all:    0.0,
            poly_ask_vol_all:    0.0,
            poly_imbalance:      0.0,
            trade_side:          String::new(),
            trade_price:         0.0,
            trade_size:          0.0,
            is_informed:         0,
            session_id:          0,
            imba_status:         "IDLE".to_string(),
            imba_side:           String::new(),
            imba_entry_price:    0.0,
            imba_exit_price:     0.0,
            imba_trade_pnl:      0.0,
            imba_balance:        0.0,
            liqb_status:         "IDLE".to_string(),
            liqb_side:           String::new(),
            liqb_entry_price:    0.0,
            liqb_exit_price:     0.0,
            liqb_trade_pnl:      0.0,
            liqb_balance:        0.0,
            trades_per_second:    0.0,
            price_velocity:       0.0,
            poly_liquidity_delta: 0.0,
            absorption_ratio:     0.0,
            price_gap_ratio:      0.0,
            spoofing_flag:        0,
            tape_speed_flag:      0,
            gap_alert_flag:       0,
            bollinger_sma:        0.0,
            bollinger_upper:      0.0,
            bollinger_lower:      0.0,
            mean_reversion_signal: 0,
            technical_confluence:  0,
            trend_direction:      0,
            signal_label:         String::new(),
            realized_volatility:   0.0,
            high_volatility_event: 0,
            bollinger_position:    0,
            master_signal:         0,
            cp_uncertainty_range:  0.0,
            cp_valid_signal:       0,
            macro_slope:           0.0,
            vfi_value:             0.0,
            macd_hist:             0.0,
            predicted_bias:        String::new(),
            is_feedback_adjusted:  0,
            dynamic_rsi:           0.0,
            vfi_confidence:        0.0,
            db_accuracy_factor:    1.0,
            t5_prediction:         String::new(),
            t5_entry_price:        0.0,
            t5_correct:            0,
            t3_prediction:         String::new(),
            t3_entry_price:        0.0,
            t3_active:             0,
        }
    }
}
