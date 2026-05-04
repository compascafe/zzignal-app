use serde::Serialize;
use crate::modules::core::worker::PriceLevel;

pub use crate::modules::hft::ring_buffer::{BinanceState, PriceRingBuffer};

/// Snapshot completo del orderbook de Polymarket (todos los niveles).
/// Se captura en cada BOOK_UPDATE y se almacena en un buffer circular en AppState.
#[derive(Debug, Clone, Serialize)]
pub struct PolyDepthFrame {
    pub ts_unix_ms:  i64,             // timestamp local (unix ms)
    pub side:        u8,              // 0 = UP, 1 = DOWN
    pub bids:        Vec<PriceLevel>, // todos los niveles bid
    pub asks:        Vec<PriceLevel>, // todos los niveles ask
}

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
    // ─── PNR: Point of No Return (last 5 min analysis) ─────────────────────
    pub pnr_active:             u8,    // 1 = inside last 300s window
    pub pnr_seconds_left:       i32,   // seconds until session close
    pub pnr_price:              f64,   // poly_mid at this tick
    pub pnr_return_up:          f64,   // 1.0 - poly_ask (expected return if UP)
    pub pnr_return_down:        f64,   // poly_bid - 0.0 (expected return if DOWN)
    pub pnr_volatility_1m:      f64,   // max price swing in last 60s
    pub pnr_confidence:         f64,   // |poly_mid - 0.5| * 2 (0-1 scale)
    pub pnr_trend:              i8,    // +1 UP, -1 DOWN, 0 flat
    pub pnr_spread_pct:         f64,   // spread / mid (or 1.0 if one-sided)
    // ─── Cerbero 70-80: price in [0.70, 0.80] ─────────────────────────────
    pub cerbero70_active:       u8,
    pub cerbero70_price:        f64,
    pub cerbero70_dir:          i8,    // 1=UP, -1=DOWN, 0=N/A
    // ─── Cerbero 80-90: price in [0.80, 0.90] ─────────────────────────────
    pub cerbero80_active:       u8,
    pub cerbero80_price:        f64,
    pub cerbero80_dir:          i8,
    // ─── Cerbero 90-98: price in [0.90, 0.98] ─────────────────────────────
    pub cerbero90_active:       u8,
    pub cerbero90_price:        f64,
    pub cerbero90_dir:          i8,
    // ─── Fenix 35-65: price in [0.35, 0.65] ───────────────────────────────
    pub fenix35_active:         u8,
    pub fenix35_price:          f64,
    pub fenix35_dir:            i8,
    // ─── Fenix 30-50: price in [0.30, 0.50] ───────────────────────────────
    pub fenix30_active:         u8,
    pub fenix30_price:          f64,
    pub fenix30_dir:            i8,
    // ─── Fenix 45-55: price in [0.45, 0.55] ───────────────────────────────
    pub fenix45_active:         u8,
    pub fenix45_price:          f64,
    pub fenix45_dir:            i8,
    // ─── Fenix Trading (paper-trading simulation) ───────────────────────────
    pub fenix35_trade:          u8,    // 1 = active trade
    pub fenix30_trade:          u8,
    pub fenix45_trade:          u8,
    pub fenix40_trade:          u8,
    pub fenix4550_trade:        u8,
    // ─── Fenix skip reason (per-tick diagnostic) ────────────────────────────
    pub fenix35_skip:           u8,    // 0=none, 1=trend blocked, 2=spread blocked, 3=volume blocked
    pub fenix30_skip:           u8,
    pub fenix45_skip:           u8,
    pub fenix40_skip:           u8,
    pub fenix4550_skip:         u8,
    // ─── Fenix live PnL per strategy ───────────────────────────────────────
    pub fenix35_entry:          f64,
    pub fenix35_pnl:            f64,
    pub fenix30_entry:          f64,
    pub fenix30_pnl:            f64,
    pub fenix45_entry:          f64,
    pub fenix45_pnl:            f64,
    pub fenix40_entry:          f64,
    pub fenix40_pnl:            f64,
    pub fenix4550_entry:        f64,
    pub fenix4550_pnl:          f64,
    // ─── Fenix target + exit ───────────────────────────────────────────────
    pub fenix35_target:         f64,   // exit target price
    pub fenix30_target:         f64,
    pub fenix45_target:         f64,
    pub fenix40_target:         f64,
    pub fenix4550_target:       f64,
    pub fenix35_exit:           u8,    // 1 = exited (target hit)
    pub fenix30_exit:           u8,
    pub fenix45_exit:           u8,
    pub fenix40_exit:           u8,
    pub fenix4550_exit:         u8,
    // ─── Fenix delta + velocity composite signal ──────────────────────────
    pub fenix_signal:           u8,    // 0=none, 1=UP, 2=DOWN (delta+velocity)
    // ─── Market Pressure metrics ──────────────────────────────────────────
    pub pressure_bid_floor:     f64,   // lowest bid price with vol > 10
    pub pressure_ask_ceiling:   f64,   // highest ask price with vol > 10
    pub pressure_band:          f64,   // ask_ceiling - bid_floor (effective spread)
    pub pressure_index:         f64,   // (mid - floor) / band → 0=DOWN, 1=UP
    pub pressure_skew:          f64,   // (bid_vol - ask_vol) / total within band
    // ─── Odiseo Strategies — bidirectional momentum paper-trading ────────
    // Odiseo 90 UP
    pub odiseo90_up_active:        u8,
    pub odiseo90_up_entry_price:   f64,
    pub odiseo90_up_size:          f64,
    pub odiseo90_up_pnl:           f64,
    pub odiseo90_up_exit_price:    f64,
    pub odiseo90_up_exit_reason:   u8,    // 0=none, 1=TP, 2=SL-micro, 3=SL-trend, 4=SL-hard
    pub odiseo90_up_balance:       f64,
    // Odiseo 90 DOWN
    pub odiseo90_down_active:        u8,
    pub odiseo90_down_entry_price:   f64,
    pub odiseo90_down_size:          f64,
    pub odiseo90_down_pnl:           f64,
    pub odiseo90_down_exit_price:    f64,
    pub odiseo90_down_exit_reason:   u8,
    pub odiseo90_down_balance:       f64,
    // Odiseo 93 UP
    pub odiseo93_up_active:        u8,
    pub odiseo93_up_entry_price:   f64,
    pub odiseo93_up_size:          f64,
    pub odiseo93_up_pnl:           f64,
    pub odiseo93_up_exit_price:    f64,
    pub odiseo93_up_exit_reason:   u8,
    pub odiseo93_up_balance:       f64,
    // Odiseo 93 DOWN
    pub odiseo93_down_active:        u8,
    pub odiseo93_down_entry_price:   f64,
    pub odiseo93_down_size:          f64,
    pub odiseo93_down_pnl:           f64,
    pub odiseo93_down_exit_price:    f64,
    pub odiseo93_down_exit_reason:   u8,
    pub odiseo93_down_balance:       f64,
    // Odiseo 95 UP
    pub odiseo95_up_active:        u8,
    pub odiseo95_up_entry_price:   f64,
    pub odiseo95_up_size:          f64,
    pub odiseo95_up_pnl:           f64,
    pub odiseo95_up_exit_price:    f64,
    pub odiseo95_up_exit_reason:   u8,
    pub odiseo95_up_balance:       f64,
    // Odiseo 95 DOWN
    pub odiseo95_down_active:        u8,
    pub odiseo95_down_entry_price:   f64,
    pub odiseo95_down_size:          f64,
    pub odiseo95_down_pnl:           f64,
    pub odiseo95_down_exit_price:    f64,
    pub odiseo95_down_exit_reason:   u8,
    pub odiseo95_down_balance:       f64,
    pub odiseo_signal:          u8,    // 0=none, 1=UP entry, 2=DOWN entry, 3=both
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
            pnr_active:             0,
            pnr_seconds_left:       0,
            pnr_price:              0.0,
            pnr_return_up:          0.0,
            pnr_return_down:        0.0,
            pnr_volatility_1m:      0.0,
            pnr_confidence:         0.0,
            pnr_trend:              0,
            pnr_spread_pct:         0.0,
            cerbero70_active:        0, cerbero70_price: 0.0, cerbero70_dir: 0,
            cerbero80_active:        0, cerbero80_price: 0.0, cerbero80_dir: 0,
            cerbero90_active:        0, cerbero90_price: 0.0, cerbero90_dir: 0,
            fenix35_active:          0, fenix35_price: 0.0, fenix35_dir: 0,
            fenix30_active:          0, fenix30_price: 0.0, fenix30_dir: 0,
            fenix45_active:          0, fenix45_price: 0.0,             fenix45_dir:             0,
            fenix35_trade:           0,
            fenix30_trade:           0,
            fenix45_trade:           0,
            fenix40_trade:           0,
            fenix4550_trade:         0,
            fenix35_skip:            0,
            fenix30_skip:            0,
            fenix45_skip:            0,
            fenix40_skip:            0,
            fenix4550_skip:          0,
            fenix35_entry:           0.0, fenix35_pnl: 0.0,
            fenix30_entry:           0.0, fenix30_pnl: 0.0,
            fenix45_entry:           0.0, fenix45_pnl: 0.0,
            fenix40_entry:           0.0, fenix40_pnl: 0.0,
            fenix4550_entry:         0.0, fenix4550_pnl: 0.0,
            fenix35_target:          0.0,
            fenix30_target:          0.0,
            fenix45_target:          0.0,
            fenix40_target:          0.0,
            fenix4550_target:        0.0,
            fenix35_exit:            0,
            fenix30_exit:            0,
            fenix45_exit:            0,
            fenix40_exit:            0,
            fenix4550_exit:          0,
            fenix_signal:            0,
            pressure_bid_floor:      0.0,
            pressure_ask_ceiling:    0.0,
            pressure_band:           0.0,
            pressure_index:          0.5,
            pressure_skew:           0.0,
            odiseo90_up_active:       0, odiseo90_up_entry_price: 0.0, odiseo90_up_size: 0.0,
            odiseo90_up_pnl:          0.0, odiseo90_up_exit_price: 0.0, odiseo90_up_exit_reason: 0,
            odiseo90_up_balance:      20.0,
            odiseo90_down_active:     0, odiseo90_down_entry_price: 0.0, odiseo90_down_size: 0.0,
            odiseo90_down_pnl:        0.0, odiseo90_down_exit_price: 0.0, odiseo90_down_exit_reason: 0,
            odiseo90_down_balance:    20.0,
            odiseo93_up_active:       0, odiseo93_up_entry_price: 0.0, odiseo93_up_size: 0.0,
            odiseo93_up_pnl:          0.0, odiseo93_up_exit_price: 0.0, odiseo93_up_exit_reason: 0,
            odiseo93_up_balance:      20.0,
            odiseo93_down_active:     0, odiseo93_down_entry_price: 0.0, odiseo93_down_size: 0.0,
            odiseo93_down_pnl:        0.0, odiseo93_down_exit_price: 0.0, odiseo93_down_exit_reason: 0,
            odiseo93_down_balance:    20.0,
            odiseo95_up_active:       0, odiseo95_up_entry_price: 0.0, odiseo95_up_size: 0.0,
            odiseo95_up_pnl:          0.0, odiseo95_up_exit_price: 0.0, odiseo95_up_exit_reason: 0,
            odiseo95_up_balance:      20.0,
            odiseo95_down_active:     0, odiseo95_down_entry_price: 0.0, odiseo95_down_size: 0.0,
            odiseo95_down_pnl:        0.0, odiseo95_down_exit_price: 0.0, odiseo95_down_exit_reason: 0,
            odiseo95_down_balance:    20.0,
            odiseo_signal:           0,
        }
    }
}
