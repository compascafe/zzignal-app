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
    // ─── Market Pressure metrics ──────────────────────────────────────────
    pub pressure_bid_floor:     f64,   // lowest bid price with vol > 10
    pub pressure_ask_ceiling:   f64,   // highest ask price with vol > 10
    pub pressure_band:          f64,   // ask_ceiling - bid_floor (effective spread)
    pub pressure_index:         f64,   // (mid - floor) / band → 0=DOWN, 1=UP
    pub pressure_skew:          f64,   // (bid_vol - ask_vol) / total within band
    // ─── Odiseo Strategies — bidirectional momentum paper-trading ────────
    // Odiseo 83 UP (estrategia principal — entry≥0.83, tp=0.97, sl_hard=0.81)
    pub odiseo83_up_active:        u8,
    pub odiseo83_up_entry_price:   f64,
    pub odiseo83_up_size:          f64,
    pub odiseo83_up_pnl:           f64,
    pub odiseo83_up_exit_price:    f64,
    pub odiseo83_up_exit_reason:   u8,
    pub odiseo83_up_balance:       f64,
    // Odiseo 85 DOWN
    pub odiseo83_down_active:        u8,
    pub odiseo83_down_entry_price:   f64,
    pub odiseo83_down_size:          f64,
    pub odiseo83_down_pnl:           f64,
    pub odiseo83_down_exit_price:    f64,
    pub odiseo83_down_exit_reason:   u8,
    pub odiseo83_down_balance:       f64,
    // ─── Odiseo 83 LIVE (real-money traceability) ──────────────────────────
    pub odiseo83_up_live_pnl:       f64,   // cumulative real PnL from UP LIVE trades
    pub odiseo83_down_live_pnl:     f64,   // cumulative real PnL from DOWN LIVE trades
    pub live_usdc_balance:          f64,   // real USDC balance from Polymarket
    // Odiseo 86 UP
    pub odiseo86_up_active:        u8,
    pub odiseo86_up_entry_price:   f64,
    pub odiseo86_up_size:          f64,
    pub odiseo86_up_pnl:           f64,
    pub odiseo86_up_exit_price:    f64,
    pub odiseo86_up_exit_reason:   u8,
    pub odiseo86_up_balance:       f64,
    // Odiseo 86 DOWN
    pub odiseo86_down_active:        u8,
    pub odiseo86_down_entry_price:   f64,
    pub odiseo86_down_size:          f64,
    pub odiseo86_down_pnl:           f64,
    pub odiseo86_down_exit_price:    f64,
    pub odiseo86_down_exit_reason:   u8,
    pub odiseo86_down_balance:       f64,
    // Odiseo 87 UP
    pub odiseo87_up_active:        u8,
    pub odiseo87_up_entry_price:   f64,
    pub odiseo87_up_size:          f64,
    pub odiseo87_up_pnl:           f64,
    pub odiseo87_up_exit_price:    f64,
    pub odiseo87_up_exit_reason:   u8,
    pub odiseo87_up_balance:       f64,
    // Odiseo 87 DOWN
    pub odiseo87_down_active:        u8,
    pub odiseo87_down_entry_price:   f64,
    pub odiseo87_down_size:          f64,
    pub odiseo87_down_pnl:           f64,
    pub odiseo87_down_exit_price:    f64,
    pub odiseo87_down_exit_reason:   u8,
    pub odiseo87_down_balance:       f64,
    // Odiseo 88 UP
    pub odiseo88_up_active:        u8,
    pub odiseo88_up_entry_price:   f64,
    pub odiseo88_up_size:          f64,
    pub odiseo88_up_pnl:           f64,
    pub odiseo88_up_exit_price:    f64,
    pub odiseo88_up_exit_reason:   u8,
    pub odiseo88_up_balance:       f64,
    // Odiseo 88 DOWN
    pub odiseo88_down_active:        u8,
    pub odiseo88_down_entry_price:   f64,
    pub odiseo88_down_size:          f64,
    pub odiseo88_down_pnl:           f64,
    pub odiseo88_down_exit_price:    f64,
    pub odiseo88_down_exit_reason:   u8,
    pub odiseo88_down_balance:       f64,
    // Odiseo 89 UP
    pub odiseo89_up_active:        u8,
    pub odiseo89_up_entry_price:   f64,
    pub odiseo89_up_size:          f64,
    pub odiseo89_up_pnl:           f64,
    pub odiseo89_up_exit_price:    f64,
    pub odiseo89_up_exit_reason:   u8,
    pub odiseo89_up_balance:       f64,
    // Odiseo 89 DOWN
    pub odiseo89_down_active:        u8,
    pub odiseo89_down_entry_price:   f64,
    pub odiseo89_down_size:          f64,
    pub odiseo89_down_pnl:           f64,
    pub odiseo89_down_exit_price:    f64,
    pub odiseo89_down_exit_reason:   u8,
    pub odiseo89_down_balance:       f64,
    // Odiseo 90 UP
    pub odiseo90_up_active:        u8,
    pub odiseo90_up_entry_price:   f64,
    pub odiseo90_up_size:          f64,
    pub odiseo90_up_pnl:           f64,
    pub odiseo90_up_exit_price:    f64,
    pub odiseo90_up_exit_reason:   u8,
    pub odiseo90_up_balance:       f64,
    // Odiseo 90 DOWN
    pub odiseo90_down_active:        u8,
    pub odiseo90_down_entry_price:   f64,
    pub odiseo90_down_size:          f64,
    pub odiseo90_down_pnl:           f64,
    pub odiseo90_down_exit_price:    f64,
    pub odiseo90_down_exit_reason:   u8,
    pub odiseo90_down_balance:       f64,
    // Odiseo 91 UP
    pub odiseo91_up_active:        u8,
    pub odiseo91_up_entry_price:   f64,
    pub odiseo91_up_size:          f64,
    pub odiseo91_up_pnl:           f64,
    pub odiseo91_up_exit_price:    f64,
    pub odiseo91_up_exit_reason:   u8,
    pub odiseo91_up_balance:       f64,
    // Odiseo 91 DOWN
    pub odiseo91_down_active:        u8,
    pub odiseo91_down_entry_price:   f64,
    pub odiseo91_down_size:          f64,
    pub odiseo91_down_pnl:           f64,
    pub odiseo91_down_exit_price:    f64,
    pub odiseo91_down_exit_reason:   u8,
    pub odiseo91_down_balance:       f64,
    // Odiseo 92 UP
    pub odiseo92_up_active:        u8,
    pub odiseo92_up_entry_price:   f64,
    pub odiseo92_up_size:          f64,
    pub odiseo92_up_pnl:           f64,
    pub odiseo92_up_exit_price:    f64,
    pub odiseo92_up_exit_reason:   u8,
    pub odiseo92_up_balance:       f64,
    // Odiseo 92 DOWN
    pub odiseo92_down_active:        u8,
    pub odiseo92_down_entry_price:   f64,
    pub odiseo92_down_size:          f64,
    pub odiseo92_down_pnl:           f64,
    pub odiseo92_down_exit_price:    f64,
    pub odiseo92_down_exit_reason:   u8,
    pub odiseo92_down_balance:       f64,
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
    // ─── Odiseo 94-97 (solo últimos 10 min) ──────────────────────────────
    pub odiseo94_up_active:        u8,
    pub odiseo94_up_entry_price:   f64,
    pub odiseo94_up_size:          f64,
    pub odiseo94_up_pnl:           f64,
    pub odiseo94_up_exit_price:    f64,
    pub odiseo94_up_exit_reason:   u8,
    pub odiseo94_up_balance:       f64,
    pub odiseo94_down_active:        u8,
    pub odiseo94_down_entry_price:   f64,
    pub odiseo94_down_size:          f64,
    pub odiseo94_down_pnl:           f64,
    pub odiseo94_down_exit_price:    f64,
    pub odiseo94_down_exit_reason:   u8,
    pub odiseo94_down_balance:       f64,
    // ─── Odiseo 96-97 (solo últimos 10 min) ──────────────────────────────
    pub odiseo96_up_active:        u8,
    pub odiseo96_up_entry_price:   f64,
    pub odiseo96_up_size:          f64,
    pub odiseo96_up_pnl:           f64,
    pub odiseo96_up_exit_price:    f64,
    pub odiseo96_up_exit_reason:   u8,
    pub odiseo96_up_balance:       f64,
    pub odiseo96_down_active:        u8,
    pub odiseo96_down_entry_price:   f64,
    pub odiseo96_down_size:          f64,
    pub odiseo96_down_pnl:           f64,
    pub odiseo96_down_exit_price:    f64,
    pub odiseo96_down_exit_reason:   u8,
    pub odiseo96_down_balance:       f64,
    // ─── Last Trade Price ─────────────────────────────────────────────────
    pub last_trade_up:          f64,   // último precio de trade del token UP
    pub last_trade_down:        f64,   // último precio de trade del token DOWN
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
            pnr_active:             0,
            pnr_seconds_left:       0,
            pnr_price:              0.0,
            pnr_return_up:          0.0,
            pnr_return_down:        0.0,
            pnr_volatility_1m:      0.0,
            pnr_confidence:         0.0,
            pnr_trend:              0,
            pnr_spread_pct:         0.0,
            pressure_bid_floor:      0.0,
            pressure_ask_ceiling:    0.0,
            pressure_band:           0.0,
            pressure_index:          0.5,
            pressure_skew:           0.0,
            odiseo83_up_active:       0, odiseo83_up_entry_price: 0.0, odiseo83_up_size: 0.0,
            odiseo83_up_pnl:          0.0, odiseo83_up_exit_price: 0.0, odiseo83_up_exit_reason: 0,
            odiseo83_up_balance:      20.0,
            odiseo83_down_active:     0, odiseo83_down_entry_price: 0.0, odiseo83_down_size: 0.0,
            odiseo83_down_pnl:        0.0, odiseo83_down_exit_price: 0.0, odiseo83_down_exit_reason: 0,
            odiseo83_down_balance:    20.0,
            odiseo83_up_live_pnl:      0.0, odiseo83_down_live_pnl: 0.0,
            live_usdc_balance:         0.0,
            odiseo86_up_active:       0, odiseo86_up_entry_price: 0.0, odiseo86_up_size: 0.0,
            odiseo86_up_pnl:          0.0, odiseo86_up_exit_price: 0.0, odiseo86_up_exit_reason: 0,
            odiseo86_up_balance:      20.0,
            odiseo86_down_active:     0, odiseo86_down_entry_price: 0.0, odiseo86_down_size: 0.0,
            odiseo86_down_pnl:        0.0, odiseo86_down_exit_price: 0.0, odiseo86_down_exit_reason: 0,
            odiseo86_down_balance:    20.0,
            odiseo87_up_active:       0, odiseo87_up_entry_price: 0.0, odiseo87_up_size: 0.0,
            odiseo87_up_pnl:          0.0, odiseo87_up_exit_price: 0.0, odiseo87_up_exit_reason: 0,
            odiseo87_up_balance:      20.0,
            odiseo87_down_active:     0, odiseo87_down_entry_price: 0.0, odiseo87_down_size: 0.0,
            odiseo87_down_pnl:        0.0, odiseo87_down_exit_price: 0.0, odiseo87_down_exit_reason: 0,
            odiseo87_down_balance:    20.0,
            odiseo88_up_active:       0, odiseo88_up_entry_price: 0.0, odiseo88_up_size: 0.0,
            odiseo88_up_pnl:          0.0, odiseo88_up_exit_price: 0.0, odiseo88_up_exit_reason: 0,
            odiseo88_up_balance:      20.0,
            odiseo88_down_active:     0, odiseo88_down_entry_price: 0.0, odiseo88_down_size: 0.0,
            odiseo88_down_pnl:        0.0, odiseo88_down_exit_price: 0.0, odiseo88_down_exit_reason: 0,
            odiseo88_down_balance:    20.0,
            odiseo89_up_active:       0, odiseo89_up_entry_price: 0.0, odiseo89_up_size: 0.0,
            odiseo89_up_pnl:          0.0, odiseo89_up_exit_price: 0.0, odiseo89_up_exit_reason: 0,
            odiseo89_up_balance:      20.0,
            odiseo89_down_active:     0, odiseo89_down_entry_price: 0.0, odiseo89_down_size: 0.0,
            odiseo89_down_pnl:        0.0, odiseo89_down_exit_price: 0.0, odiseo89_down_exit_reason: 0,
            odiseo89_down_balance:    20.0,
            odiseo90_up_active:       0, odiseo90_up_entry_price: 0.0, odiseo90_up_size: 0.0,
            odiseo90_up_pnl:          0.0, odiseo90_up_exit_price: 0.0, odiseo90_up_exit_reason: 0,
            odiseo90_up_balance:      20.0,
            odiseo90_down_active:     0, odiseo90_down_entry_price: 0.0, odiseo90_down_size: 0.0,
            odiseo90_down_pnl:        0.0, odiseo90_down_exit_price: 0.0, odiseo90_down_exit_reason: 0,
            odiseo90_down_balance:    20.0,
            odiseo91_up_active:       0, odiseo91_up_entry_price: 0.0, odiseo91_up_size: 0.0,
            odiseo91_up_pnl:          0.0, odiseo91_up_exit_price: 0.0, odiseo91_up_exit_reason: 0,
            odiseo91_up_balance:      20.0,
            odiseo91_down_active:     0, odiseo91_down_entry_price: 0.0, odiseo91_down_size: 0.0,
            odiseo91_down_pnl:        0.0, odiseo91_down_exit_price: 0.0, odiseo91_down_exit_reason: 0,
            odiseo91_down_balance:    20.0,
            odiseo92_up_active:       0, odiseo92_up_entry_price: 0.0, odiseo92_up_size: 0.0,
            odiseo92_up_pnl:          0.0, odiseo92_up_exit_price: 0.0, odiseo92_up_exit_reason: 0,
            odiseo92_up_balance:      20.0,
            odiseo92_down_active:     0, odiseo92_down_entry_price: 0.0, odiseo92_down_size: 0.0,
            odiseo92_down_pnl:        0.0, odiseo92_down_exit_price: 0.0, odiseo92_down_exit_reason: 0,
            odiseo92_down_balance:    20.0,
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
            odiseo94_up_active:       0, odiseo94_up_entry_price: 0.0, odiseo94_up_size: 0.0,
            odiseo94_up_pnl:          0.0, odiseo94_up_exit_price: 0.0, odiseo94_up_exit_reason: 0,
            odiseo94_up_balance:      20.0,
            odiseo94_down_active:     0, odiseo94_down_entry_price: 0.0, odiseo94_down_size: 0.0,
            odiseo94_down_pnl:        0.0, odiseo94_down_exit_price: 0.0, odiseo94_down_exit_reason: 0,
            odiseo94_down_balance:    20.0,
            odiseo96_up_active:       0, odiseo96_up_entry_price: 0.0, odiseo96_up_size: 0.0,
            odiseo96_up_pnl:          0.0, odiseo96_up_exit_price: 0.0, odiseo96_up_exit_reason: 0,
            odiseo96_up_balance:      20.0,
            odiseo96_down_active:     0, odiseo96_down_entry_price: 0.0, odiseo96_down_size: 0.0,
            odiseo96_down_pnl:        0.0, odiseo96_down_exit_price: 0.0, odiseo96_down_exit_reason: 0,
            odiseo96_down_balance:    20.0,
            last_trade_up:           0.0,
            last_trade_down:         0.0,
        }
    }
}

/// Single source of truth for CSV serialization (304 columns).
/// All CSV export paths (per-session file, live REST, DB fallback) use this.
impl CsvRecord {
    /// Column names in exact order matching `to_csv_fields()`.
    pub fn csv_header() -> &'static str {
        "ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,\
         binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,\
         poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,\
         trade_price,trade_size,is_informed,\
         imba_status,imba_side,imba_entry_price,imba_exit_price,imba_trade_pnl,imba_balance,\
         liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance,\
         trades_per_second,price_velocity,poly_liquidity_delta,absorption_ratio,\
         price_gap_ratio,spoofing_flag,tape_speed_flag,gap_alert_flag,\
         bollinger_sma,bollinger_upper,bollinger_lower,mean_reversion_signal,\
         technical_confluence,trend_direction,signal_label,realized_volatility,\
         high_volatility_event,bollinger_position,master_signal,cp_uncertainty_range,\
         cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,\
         is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor,\
         pnr_active,pnr_seconds_left,pnr_price,pnr_return_up,pnr_return_down,\
         pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct,\
         pressure_bid_floor,pressure_ask_ceiling,pressure_band,pressure_index,pressure_skew,\
         odiseo83_up_active,odiseo83_up_entry_price,odiseo83_up_size,\
         odiseo83_up_pnl,odiseo83_up_exit_price,odiseo83_up_exit_reason,odiseo83_up_balance,\
         odiseo83_down_active,odiseo83_down_entry_price,odiseo83_down_size,\
         odiseo83_down_pnl,odiseo83_down_exit_price,odiseo83_down_exit_reason,odiseo83_down_balance,\
         odiseo83_up_live_pnl,odiseo83_down_live_pnl,live_usdc_balance,\
         odiseo86_up_active,odiseo86_up_entry_price,odiseo86_up_size,\
         odiseo86_up_pnl,odiseo86_up_exit_price,odiseo86_up_exit_reason,odiseo86_up_balance,\
         odiseo86_down_active,odiseo86_down_entry_price,odiseo86_down_size,\
         odiseo86_down_pnl,odiseo86_down_exit_price,odiseo86_down_exit_reason,odiseo86_down_balance,\
         odiseo87_up_active,odiseo87_up_entry_price,odiseo87_up_size,\
         odiseo87_up_pnl,odiseo87_up_exit_price,odiseo87_up_exit_reason,odiseo87_up_balance,\
         odiseo87_down_active,odiseo87_down_entry_price,odiseo87_down_size,\
         odiseo87_down_pnl,odiseo87_down_exit_price,odiseo87_down_exit_reason,odiseo87_down_balance,\
         odiseo88_up_active,odiseo88_up_entry_price,odiseo88_up_size,\
         odiseo88_up_pnl,odiseo88_up_exit_price,odiseo88_up_exit_reason,odiseo88_up_balance,\
         odiseo88_down_active,odiseo88_down_entry_price,odiseo88_down_size,\
         odiseo88_down_pnl,odiseo88_down_exit_price,odiseo88_down_exit_reason,odiseo88_down_balance,\
         odiseo89_up_active,odiseo89_up_entry_price,odiseo89_up_size,\
         odiseo89_up_pnl,odiseo89_up_exit_price,odiseo89_up_exit_reason,odiseo89_up_balance,\
         odiseo89_down_active,odiseo89_down_entry_price,odiseo89_down_size,\
         odiseo89_down_pnl,odiseo89_down_exit_price,odiseo89_down_exit_reason,odiseo89_down_balance,\
         odiseo90_up_active,odiseo90_up_entry_price,odiseo90_up_size,\
         odiseo90_up_pnl,odiseo90_up_exit_price,odiseo90_up_exit_reason,odiseo90_up_balance,\
         odiseo90_down_active,odiseo90_down_entry_price,odiseo90_down_size,\
         odiseo90_down_pnl,odiseo90_down_exit_price,odiseo90_down_exit_reason,odiseo90_down_balance,\
         odiseo91_up_active,odiseo91_up_entry_price,odiseo91_up_size,\
         odiseo91_up_pnl,odiseo91_up_exit_price,odiseo91_up_exit_reason,odiseo91_up_balance,\
         odiseo91_down_active,odiseo91_down_entry_price,odiseo91_down_size,\
         odiseo91_down_pnl,odiseo91_down_exit_price,odiseo91_down_exit_reason,odiseo91_down_balance,\
         odiseo92_up_active,odiseo92_up_entry_price,odiseo92_up_size,\
         odiseo92_up_pnl,odiseo92_up_exit_price,odiseo92_up_exit_reason,odiseo92_up_balance,\
         odiseo92_down_active,odiseo92_down_entry_price,odiseo92_down_size,\
         odiseo92_down_pnl,odiseo92_down_exit_price,odiseo92_down_exit_reason,odiseo92_down_balance,\
         odiseo93_up_active,odiseo93_up_entry_price,odiseo93_up_size,\
         odiseo93_up_pnl,odiseo93_up_exit_price,odiseo93_up_exit_reason,odiseo93_up_balance,\
         odiseo93_down_active,odiseo93_down_entry_price,odiseo93_down_size,\
         odiseo93_down_pnl,odiseo93_down_exit_price,odiseo93_down_exit_reason,odiseo93_down_balance,\
         odiseo95_up_active,odiseo95_up_entry_price,odiseo95_up_size,\
         odiseo95_up_pnl,odiseo95_up_exit_price,odiseo95_up_exit_reason,odiseo95_up_balance,\
         odiseo95_down_active,odiseo95_down_entry_price,odiseo95_down_size,\
         odiseo95_down_pnl,odiseo95_down_exit_price,odiseo95_down_exit_reason,odiseo95_down_balance,\
         odiseo_signal,\
         odiseo94_up_active,odiseo94_up_entry_price,odiseo94_up_size,\
         odiseo94_up_pnl,odiseo94_up_exit_price,odiseo94_up_exit_reason,odiseo94_up_balance,\
         odiseo94_down_active,odiseo94_down_entry_price,odiseo94_down_size,\
         odiseo94_down_pnl,odiseo94_down_exit_price,odiseo94_down_exit_reason,odiseo94_down_balance,\
         odiseo96_up_active,odiseo96_up_entry_price,odiseo96_up_size,\
         odiseo96_up_pnl,odiseo96_up_exit_price,odiseo96_up_exit_reason,odiseo96_up_balance,\
         odiseo96_down_active,odiseo96_down_entry_price,odiseo96_down_size,\
         odiseo96_down_pnl,odiseo96_down_exit_price,odiseo96_down_exit_reason,odiseo96_down_balance,\
         last_trade_up,last_trade_down"
    }

    /// Returns 304 CSV fields as strings in the exact order of `csv_header()`.
    /// Used by all three CSV export paths (per-session file, live REST, DB fallback).
    pub fn to_csv_fields(&self) -> Vec<String> {
        let mut f: Vec<String> = Vec::with_capacity(304);
        f.push(self.ts_local.clone());
        f.push(self.ts_exchange.clone());
        f.push(self.event_type.as_str().to_string());
        f.push(self.latencia_ms.to_string());
        f.push(self.binance_price.to_string());
        f.push(self.binance_micro_price.to_string());
        f.push(self.binance_imbalance.to_string());
        f.push(self.binance_vol_100ms.to_string());
        f.push(self.binance_vol_24h.to_string());
        f.push(self.poly_bid.to_string());
        f.push(self.poly_ask.to_string());
        f.push(self.poly_mid.to_string());
        f.push(self.poly_spread.to_string());
        f.push(self.poly_bid_vol_all.to_string());
        f.push(self.poly_ask_vol_all.to_string());
        f.push(self.poly_imbalance.to_string());
        f.push(self.trade_side.clone());
        f.push(self.trade_price.to_string());
        f.push(self.trade_size.to_string());
        f.push(self.is_informed.to_string());
        f.push(self.imba_status.clone());
        f.push(self.imba_side.clone());
        f.push(self.imba_entry_price.to_string());
        f.push(self.imba_exit_price.to_string());
        f.push(self.imba_trade_pnl.to_string());
        f.push(self.imba_balance.to_string());
        f.push(self.liqb_status.clone());
        f.push(self.liqb_side.clone());
        f.push(self.liqb_entry_price.to_string());
        f.push(self.liqb_exit_price.to_string());
        f.push(self.liqb_trade_pnl.to_string());
        f.push(self.liqb_balance.to_string());
        f.push(self.trades_per_second.to_string());
        f.push(self.price_velocity.to_string());
        f.push(self.poly_liquidity_delta.to_string());
        f.push(self.absorption_ratio.to_string());
        f.push(self.price_gap_ratio.to_string());
        f.push(self.spoofing_flag.to_string());
        f.push(self.tape_speed_flag.to_string());
        f.push(self.gap_alert_flag.to_string());
        f.push(self.bollinger_sma.to_string());
        f.push(self.bollinger_upper.to_string());
        f.push(self.bollinger_lower.to_string());
        f.push(self.mean_reversion_signal.to_string());
        f.push(self.technical_confluence.to_string());
        f.push(self.trend_direction.to_string());
        f.push(self.signal_label.clone());
        f.push(self.realized_volatility.to_string());
        f.push(self.high_volatility_event.to_string());
        f.push(self.bollinger_position.to_string());
        f.push(self.master_signal.to_string());
        f.push(self.cp_uncertainty_range.to_string());
        f.push(self.cp_valid_signal.to_string());
        f.push(self.macro_slope.to_string());
        f.push(self.vfi_value.to_string());
        f.push(self.macd_hist.to_string());
        f.push(self.predicted_bias.clone());
        f.push(self.is_feedback_adjusted.to_string());
        f.push(self.dynamic_rsi.to_string());
        f.push(self.vfi_confidence.to_string());
        f.push(self.db_accuracy_factor.to_string());
        f.push(self.pnr_active.to_string());
        f.push(self.pnr_seconds_left.to_string());
        f.push(self.pnr_price.to_string());
        f.push(self.pnr_return_up.to_string());
        f.push(self.pnr_return_down.to_string());
        f.push(self.pnr_volatility_1m.to_string());
        f.push(self.pnr_confidence.to_string());
        f.push(self.pnr_trend.to_string());
        f.push(self.pnr_spread_pct.to_string());
        f.push(self.pressure_bid_floor.to_string());
        f.push(self.pressure_ask_ceiling.to_string());
        f.push(self.pressure_band.to_string());
        f.push(self.pressure_index.to_string());
        f.push(self.pressure_skew.to_string());
        f.push(self.odiseo83_up_active.to_string());
        f.push(self.odiseo83_up_entry_price.to_string());
        f.push(self.odiseo83_up_size.to_string());
        f.push(self.odiseo83_up_pnl.to_string());
        f.push(self.odiseo83_up_exit_price.to_string());
        f.push(self.odiseo83_up_exit_reason.to_string());
        f.push(self.odiseo83_up_balance.to_string());
        f.push(self.odiseo83_down_active.to_string());
        f.push(self.odiseo83_down_entry_price.to_string());
        f.push(self.odiseo83_down_size.to_string());
        f.push(self.odiseo83_down_pnl.to_string());
        f.push(self.odiseo83_down_exit_price.to_string());
        f.push(self.odiseo83_down_exit_reason.to_string());
        f.push(self.odiseo83_down_balance.to_string());
        f.push(self.odiseo83_up_live_pnl.to_string());
        f.push(self.odiseo83_down_live_pnl.to_string());
        f.push(self.live_usdc_balance.to_string());
        f.push(self.odiseo86_up_active.to_string());
        f.push(self.odiseo86_up_entry_price.to_string());
        f.push(self.odiseo86_up_size.to_string());
        f.push(self.odiseo86_up_pnl.to_string());
        f.push(self.odiseo86_up_exit_price.to_string());
        f.push(self.odiseo86_up_exit_reason.to_string());
        f.push(self.odiseo86_up_balance.to_string());
        f.push(self.odiseo86_down_active.to_string());
        f.push(self.odiseo86_down_entry_price.to_string());
        f.push(self.odiseo86_down_size.to_string());
        f.push(self.odiseo86_down_pnl.to_string());
        f.push(self.odiseo86_down_exit_price.to_string());
        f.push(self.odiseo86_down_exit_reason.to_string());
        f.push(self.odiseo86_down_balance.to_string());
        f.push(self.odiseo87_up_active.to_string());
        f.push(self.odiseo87_up_entry_price.to_string());
        f.push(self.odiseo87_up_size.to_string());
        f.push(self.odiseo87_up_pnl.to_string());
        f.push(self.odiseo87_up_exit_price.to_string());
        f.push(self.odiseo87_up_exit_reason.to_string());
        f.push(self.odiseo87_up_balance.to_string());
        f.push(self.odiseo87_down_active.to_string());
        f.push(self.odiseo87_down_entry_price.to_string());
        f.push(self.odiseo87_down_size.to_string());
        f.push(self.odiseo87_down_pnl.to_string());
        f.push(self.odiseo87_down_exit_price.to_string());
        f.push(self.odiseo87_down_exit_reason.to_string());
        f.push(self.odiseo87_down_balance.to_string());
        f.push(self.odiseo88_up_active.to_string());
        f.push(self.odiseo88_up_entry_price.to_string());
        f.push(self.odiseo88_up_size.to_string());
        f.push(self.odiseo88_up_pnl.to_string());
        f.push(self.odiseo88_up_exit_price.to_string());
        f.push(self.odiseo88_up_exit_reason.to_string());
        f.push(self.odiseo88_up_balance.to_string());
        f.push(self.odiseo88_down_active.to_string());
        f.push(self.odiseo88_down_entry_price.to_string());
        f.push(self.odiseo88_down_size.to_string());
        f.push(self.odiseo88_down_pnl.to_string());
        f.push(self.odiseo88_down_exit_price.to_string());
        f.push(self.odiseo88_down_exit_reason.to_string());
        f.push(self.odiseo88_down_balance.to_string());
        f.push(self.odiseo89_up_active.to_string());
        f.push(self.odiseo89_up_entry_price.to_string());
        f.push(self.odiseo89_up_size.to_string());
        f.push(self.odiseo89_up_pnl.to_string());
        f.push(self.odiseo89_up_exit_price.to_string());
        f.push(self.odiseo89_up_exit_reason.to_string());
        f.push(self.odiseo89_up_balance.to_string());
        f.push(self.odiseo89_down_active.to_string());
        f.push(self.odiseo89_down_entry_price.to_string());
        f.push(self.odiseo89_down_size.to_string());
        f.push(self.odiseo89_down_pnl.to_string());
        f.push(self.odiseo89_down_exit_price.to_string());
        f.push(self.odiseo89_down_exit_reason.to_string());
        f.push(self.odiseo89_down_balance.to_string());
        f.push(self.odiseo90_up_active.to_string());
        f.push(self.odiseo90_up_entry_price.to_string());
        f.push(self.odiseo90_up_size.to_string());
        f.push(self.odiseo90_up_pnl.to_string());
        f.push(self.odiseo90_up_exit_price.to_string());
        f.push(self.odiseo90_up_exit_reason.to_string());
        f.push(self.odiseo90_up_balance.to_string());
        f.push(self.odiseo90_down_active.to_string());
        f.push(self.odiseo90_down_entry_price.to_string());
        f.push(self.odiseo90_down_size.to_string());
        f.push(self.odiseo90_down_pnl.to_string());
        f.push(self.odiseo90_down_exit_price.to_string());
        f.push(self.odiseo90_down_exit_reason.to_string());
        f.push(self.odiseo90_down_balance.to_string());
        f.push(self.odiseo91_up_active.to_string());
        f.push(self.odiseo91_up_entry_price.to_string());
        f.push(self.odiseo91_up_size.to_string());
        f.push(self.odiseo91_up_pnl.to_string());
        f.push(self.odiseo91_up_exit_price.to_string());
        f.push(self.odiseo91_up_exit_reason.to_string());
        f.push(self.odiseo91_up_balance.to_string());
        f.push(self.odiseo91_down_active.to_string());
        f.push(self.odiseo91_down_entry_price.to_string());
        f.push(self.odiseo91_down_size.to_string());
        f.push(self.odiseo91_down_pnl.to_string());
        f.push(self.odiseo91_down_exit_price.to_string());
        f.push(self.odiseo91_down_exit_reason.to_string());
        f.push(self.odiseo91_down_balance.to_string());
        f.push(self.odiseo92_up_active.to_string());
        f.push(self.odiseo92_up_entry_price.to_string());
        f.push(self.odiseo92_up_size.to_string());
        f.push(self.odiseo92_up_pnl.to_string());
        f.push(self.odiseo92_up_exit_price.to_string());
        f.push(self.odiseo92_up_exit_reason.to_string());
        f.push(self.odiseo92_up_balance.to_string());
        f.push(self.odiseo92_down_active.to_string());
        f.push(self.odiseo92_down_entry_price.to_string());
        f.push(self.odiseo92_down_size.to_string());
        f.push(self.odiseo92_down_pnl.to_string());
        f.push(self.odiseo92_down_exit_price.to_string());
        f.push(self.odiseo92_down_exit_reason.to_string());
        f.push(self.odiseo92_down_balance.to_string());
        f.push(self.odiseo93_up_active.to_string());
        f.push(self.odiseo93_up_entry_price.to_string());
        f.push(self.odiseo93_up_size.to_string());
        f.push(self.odiseo93_up_pnl.to_string());
        f.push(self.odiseo93_up_exit_price.to_string());
        f.push(self.odiseo93_up_exit_reason.to_string());
        f.push(self.odiseo93_up_balance.to_string());
        f.push(self.odiseo93_down_active.to_string());
        f.push(self.odiseo93_down_entry_price.to_string());
        f.push(self.odiseo93_down_size.to_string());
        f.push(self.odiseo93_down_pnl.to_string());
        f.push(self.odiseo93_down_exit_price.to_string());
        f.push(self.odiseo93_down_exit_reason.to_string());
        f.push(self.odiseo93_down_balance.to_string());
        f.push(self.odiseo95_up_active.to_string());
        f.push(self.odiseo95_up_entry_price.to_string());
        f.push(self.odiseo95_up_size.to_string());
        f.push(self.odiseo95_up_pnl.to_string());
        f.push(self.odiseo95_up_exit_price.to_string());
        f.push(self.odiseo95_up_exit_reason.to_string());
        f.push(self.odiseo95_up_balance.to_string());
        f.push(self.odiseo95_down_active.to_string());
        f.push(self.odiseo95_down_entry_price.to_string());
        f.push(self.odiseo95_down_size.to_string());
        f.push(self.odiseo95_down_pnl.to_string());
        f.push(self.odiseo95_down_exit_price.to_string());
        f.push(self.odiseo95_down_exit_reason.to_string());
        f.push(self.odiseo95_down_balance.to_string());
        f.push(self.odiseo_signal.to_string());
        f.push(self.odiseo94_up_active.to_string());
        f.push(self.odiseo94_up_entry_price.to_string());
        f.push(self.odiseo94_up_size.to_string());
        f.push(self.odiseo94_up_pnl.to_string());
        f.push(self.odiseo94_up_exit_price.to_string());
        f.push(self.odiseo94_up_exit_reason.to_string());
        f.push(self.odiseo94_up_balance.to_string());
        f.push(self.odiseo94_down_active.to_string());
        f.push(self.odiseo94_down_entry_price.to_string());
        f.push(self.odiseo94_down_size.to_string());
        f.push(self.odiseo94_down_pnl.to_string());
        f.push(self.odiseo94_down_exit_price.to_string());
        f.push(self.odiseo94_down_exit_reason.to_string());
        f.push(self.odiseo94_down_balance.to_string());
        f.push(self.odiseo96_up_active.to_string());
        f.push(self.odiseo96_up_entry_price.to_string());
        f.push(self.odiseo96_up_size.to_string());
        f.push(self.odiseo96_up_pnl.to_string());
        f.push(self.odiseo96_up_exit_price.to_string());
        f.push(self.odiseo96_up_exit_reason.to_string());
        f.push(self.odiseo96_up_balance.to_string());
        f.push(self.odiseo96_down_active.to_string());
        f.push(self.odiseo96_down_entry_price.to_string());
        f.push(self.odiseo96_down_size.to_string());
        f.push(self.odiseo96_down_pnl.to_string());
        f.push(self.odiseo96_down_exit_price.to_string());
        f.push(self.odiseo96_down_exit_reason.to_string());
        f.push(self.odiseo96_down_balance.to_string());
        f.push(self.last_trade_up.to_string());
        f.push(self.last_trade_down.to_string());
        f
    }

    /// Convenience: `to_csv_fields().join(",")`
    #[inline]
    pub fn to_csv_line(&self) -> String {
        self.to_csv_fields().join(",")
    }
}
