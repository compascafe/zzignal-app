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
    pub btc_vol:             f64,   // real-time BTC volume from Binance aggTrade
    pub poly_bid:            f64,
    pub poly_ask:            f64,
    pub poly_mid:            f64,
    pub poly_spread:         f64,
    pub poly_bid_vol_all:    f64,
    pub poly_ask_vol_all:    f64,
    pub poly_imbalance:      f64,
    /// Session ID — para filtrar mem_hft y evitar fuga de datos entre sesiones.
    pub session_id:          i32,
    // ─── Advanced HFT Metrics ────────────────────────────────────────────
    pub trades_per_second:    f64,  // Binance TRADE count in last rolling 1s
    pub price_velocity:       f64,  // Δprice/Δtime over 500ms window (USD/s)
    pub btc_acel:             f64,  // Δvelocity/Δtime (USD/s²) — price acceleration
    pub poly_liquidity_delta: f64,  // Δpoly_ask_vol_all vs previous tick
    pub absorption_ratio:     f64,  // trade_vol / |Δprice| — high = absorption
    pub price_gap_ratio:      f64,  // (binance_micro - poly_mid) / binance_micro * 100
    pub spoofing_flag:        u8,   // 1 = >30% vol drop with no poly trade
    pub tape_speed_flag:      u8,   // 1 = high volume spike detected
    pub gap_alert_flag:       u8,   // 1 = price gap > 0.05%
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
    pub odiseo_signal:          u8,    // 0=none, 1=UP entry, 2=DOWN entry, 3=both
    // ─── Last Trade Price ─────────────────────────────────────────────────
    pub clob_trade_up:         f64,   // último precio CLOB trade UP
    pub clob_trade_dn:         f64,   // último precio CLOB trade DOWN
    pub clob_trade_up_vol:     f64,   // volumen del último trade CLOB UP
    pub clob_trade_dn_vol:     f64,   // volumen del último trade CLOB DOWN
    pub clob_trade_up_ts:      String,// timestamp HH:MM:SS.mmm último trade UP
    pub clob_trade_dn_ts:      String,// timestamp HH:MM:SS.mmm último trade DOWN
    pub od83_event:            String, // IN_UP|IN_DN|OUT_UP|OUT_DN|empty — Odiseo 83 entry/exit marker
    // ─── Odiseo 83 Timing ────────────────────────────────────────────────
    pub od83_up_mode:           String, // PAPER | LIVE
    pub od83_dn_mode:           String, // PAPER | LIVE
    pub od83_up_at:             String, // HH:MM:SS.mmm entry timestamp UTC-5
    pub od83_dn_at:             String,
    pub od83_up_fill_ms:        i64,    // ms until fill confirmation (0=paper)
    pub od83_dn_fill_ms:        i64,
    // ─── Anti-Flash Dump ──────────────────────────────────────────────────
    pub tick_gap_ms:            i64,    // ms since last tick (>2000 = frozen)
    pub bid_drain:              f64,    // % bid_vol lost vs 5 ticks ago (-75% = whales leaving)
    pub ask_wall:               u8,     // 1 if ask_vol > 3x bid_vol (one-sided imminent)
    pub dump_score:             u8,     // 0=normal 1=warning 2=critical 3=dead
    // ─── Anti-Reversion ───────────────────────────────────────────────────
    pub btc_delta:              f64,    // BTC change in USD since Odiseo entered
    pub mid_from_entry:         f64,    // poly_mid - entry_price (distance from entry)
    pub adverse_ticks:          u8,     // consecutive ticks moving against position
    pub vol_bleed:              f64,    // % volume of position's side lost vs 5s ago
    pub reversal_score:         u8,     // 0=safe 1=alert 2=danger 3=exit
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
            btc_vol:             0.0,
            poly_bid:            0.0,
            poly_ask:            0.0,
            poly_mid:            0.0,
            poly_spread:         0.0,
            poly_bid_vol_all:    0.0,
            poly_ask_vol_all:    0.0,
            poly_imbalance:      0.0,
            session_id:          0,
            trades_per_second:    0.0,
            price_velocity:       0.0,
            btc_acel:             0.0,
            poly_liquidity_delta: 0.0,
            absorption_ratio:     0.0,
            price_gap_ratio:      0.0,
            spoofing_flag:        0,
            tape_speed_flag:      0,
            gap_alert_flag:       0,
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
            odiseo_signal:           0,
            clob_trade_up:        0.0,
            clob_trade_dn:        0.0,
            clob_trade_up_vol:    0.0,
            clob_trade_dn_vol:    0.0,
            clob_trade_up_ts:     String::new(),
            clob_trade_dn_ts:     String::new(),
            od83_event:            String::new(),
            od83_up_mode:           String::new(),
            od83_dn_mode:           String::new(),
            od83_up_at:             String::new(),
            od83_dn_at:             String::new(),
            od83_up_fill_ms:        0,
            od83_dn_fill_ms:        0,
            tick_gap_ms:            0,
            bid_drain:              0.0,
            ask_wall:               0,
            dump_score:             0,
            btc_delta:              0.0,
            mid_from_entry:         0.0,
            adverse_ticks:          0,
            vol_bleed:              0.0,
            reversal_score:         0,
        }
    }
}

/// Single source of truth for CSV serialization (304 columns).
/// All CSV export paths (per-session file, live REST, DB fallback) use this.
impl CsvRecord {
    /// Column names in exact order matching `to_csv_fields()`.
            pub fn csv_header() -> &'static str {
        "time,ts_exchange,event,latencia_ms,binance_price,binance_micro_price,\
         binance_imbalance,binance_vol_100ms,binance_vol_24h,btc_vol,bid,ask,mid,spread,\
         bid_vol,ask_vol,imbalance,trades_per_second,btc_vel,btc_acel,\
         poly_liquidity_delta,absorption_ratio,price_gap_ratio,spoof,tape,gap,\
         pnr_active,secs_left,pnr_price,pnr_return_up,pnr_return_down,\
         pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct,p_bid_lo,\
         p_ask_hi,p_band,p_index,p_skew,od83_up,od83_up_entry,od83_up_sz,\
         od83_up_pnl,od83_up_exit,od83_up_r,od83_up_bal,od83_dn,od83_dn_entry,\
         od83_dn_sz,od83_dn_pnl,od83_dn_exit,od83_dn_r,od83_dn_bal,live_up,\
         live_dn,live_bal,od_signal,clob_trade_up,clob_trade_dn,\
         clob_trade_up_vol,clob_trade_dn_vol,clob_trade_up_ts,clob_trade_dn_ts,od83_event,\
         od83_up_mode,od83_dn_mode,\
         od83_up_at,od83_dn_at,od83_up_fill_ms,od83_dn_fill_ms,tick_gap_ms,\
         bid_drain,ask_wall,dump_score,btc_delta,mid_from_entry,adverse_ticks,\
         vol_bleed,reversal_score"
    }    /// Returns 304 CSV fields as strings in the exact order of `csv_header()`.
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
        f.push(self.btc_vol.to_string());
        f.push(self.poly_bid.to_string());
        f.push(self.poly_ask.to_string());
        f.push(self.poly_mid.to_string());
        f.push(self.poly_spread.to_string());
        f.push(self.poly_bid_vol_all.to_string());
        f.push(self.poly_ask_vol_all.to_string());
        f.push(self.poly_imbalance.to_string());
        f.push(self.trades_per_second.to_string());
        f.push(self.price_velocity.to_string());
        f.push(self.btc_acel.to_string());
        f.push(self.poly_liquidity_delta.to_string());
        f.push(self.absorption_ratio.to_string());
        f.push(self.price_gap_ratio.to_string());
        f.push(self.spoofing_flag.to_string());
        f.push(self.tape_speed_flag.to_string());
        f.push(self.gap_alert_flag.to_string());
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
        f.push(self.odiseo_signal.to_string());
        f.push(self.clob_trade_up.to_string());
        f.push(self.clob_trade_dn.to_string());
        f.push(self.clob_trade_up_vol.to_string());
        f.push(self.clob_trade_dn_vol.to_string());
        f.push(self.clob_trade_up_ts.clone());
        f.push(self.clob_trade_dn_ts.clone());
        f.push(self.od83_event.clone());
        f.push(self.od83_up_mode.clone());
        f.push(self.od83_dn_mode.clone());
        f.push(self.od83_up_at.clone());
        f.push(self.od83_dn_at.clone());
        f.push(self.od83_up_fill_ms.to_string());
        f.push(self.od83_dn_fill_ms.to_string());
        f.push(self.tick_gap_ms.to_string());
        f.push(self.bid_drain.to_string());
        f.push(self.ask_wall.to_string());
        f.push(self.dump_score.to_string());
        f.push(self.btc_delta.to_string());
        f.push(self.mid_from_entry.to_string());
        f.push(self.adverse_ticks.to_string());
        f.push(self.vol_bleed.to_string());
        f.push(self.reversal_score.to_string());
        f
    }

    /// Convenience: `to_csv_fields().join(",")`
    #[inline]
    pub fn to_csv_line(&self) -> String {
        self.to_csv_fields().join(",")
    }
}
