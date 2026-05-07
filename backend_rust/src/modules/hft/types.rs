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
    pub binance_imbalance:   f32,
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
    pub price_velocity:       f64,  // Δprice/Δtime over 500ms window (USD/s)
    pub btc_acel:             f64,  // Δvelocity/Δtime (USD/s²) — price acceleration
    pub btc_vol_ratio:        f64,  // btc_vol / (bid_vol+ask_vol) — real vs resting liquidity
    pub poly_liquidity_delta: f64,  // 3-tick avg Δask_vol — liquidity change
    pub price_impact:         f64,  // |trade_up-trade_dn|/(vol_up+vol_dn) — Amihud illiquidity
    pub depth_concentration:  f64,  // max(bid_vol,ask_vol)/(total) — one-sided depth
    pub spoofing_flag:        u8,   // 1 = >30% vol drop with no poly trade
    pub pnr_seconds_left:       i32,   // seconds until session close
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
    // ─── Last Trade Price ─────────────────────────────────────────────────
    pub clob_trade_up:         f64,   // último precio CLOB trade UP
    pub clob_trade_dn:         f64,   // último precio CLOB trade DOWN
    pub clob_trade_up_vol:     f64,   // volumen del último trade CLOB UP
    pub clob_trade_dn_vol:     f64,   // volumen del último trade CLOB DOWN
    pub clob_trade_count_up:  u16,   // N° of qualifying trades in UP window
    pub clob_trade_count_dn:  u16,   // N° of qualifying trades in DOWN window
    pub od83_event:            String, // IN_UP|IN_DN|OUT_UP|OUT_DN|empty — Odiseo 83 entry/exit marker
    pub od83_filters:          u8,    // enabled filter bitmask: 0=all_off 255=all_on (8 bits=8 filters)
    // ─── Odiseo 83 Timing ────────────────────────────────────────────────
    pub od83_up_mode:           String, // PAPER | LIVE
    pub od83_dn_mode:           String, // PAPER | LIVE
    pub od83_up_at:             String, // HH:MM:SS.mmm entry timestamp UTC-5
    pub od83_dn_at:             String,
    // ─── Anti-Flash Dump ──────────────────────────────────────────────────
    pub tick_gap_ms:            i64,    // ms since last tick (>2000 = frozen)
    pub ask_wall:               u8,     // 1 if ask_vol > 3x bid_vol (one-sided imminent)
    pub dump_score:             u8,     // 0=normal 1=warning 2=critical 3=dead
    // ─── Anti-Reversion ───────────────────────────────────────────────────
    pub mid_from_entry:         f64,    // poly_mid - entry_price (distance from entry)
}

impl Default for CsvRecord {
    fn default() -> Self {
        Self {
            ts_local:            String::new(),
            ts_exchange:         String::new(),
            event_type:          EventType::BookUpdate,
            latencia_ms:         0,
            binance_price:       0.0,
            binance_imbalance:   0.0,
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
            price_velocity:       0.0,
            btc_acel:             0.0,
            btc_vol_ratio:        0.0,
            poly_liquidity_delta: 0.0,
            price_impact:         0.0,
            depth_concentration:  0.5,
            spoofing_flag:        0,
            pnr_seconds_left:       0,
            odiseo83_up_active:       0, odiseo83_up_entry_price: 0.0, odiseo83_up_size: 0.0,
            odiseo83_up_pnl:          0.0, odiseo83_up_exit_price: 0.0, odiseo83_up_exit_reason: 0,
            odiseo83_up_balance:      20.0,
            odiseo83_down_active:     0, odiseo83_down_entry_price: 0.0, odiseo83_down_size: 0.0,
            odiseo83_down_pnl:        0.0, odiseo83_down_exit_price: 0.0, odiseo83_down_exit_reason: 0,
            odiseo83_down_balance:    20.0,
            clob_trade_up:        0.0,
            clob_trade_dn:        0.0,
            clob_trade_up_vol:    0.0,
            clob_trade_dn_vol:    0.0,
            clob_trade_count_up: 0,
            clob_trade_count_dn: 0,
            od83_event:            String::new(),
            od83_filters:          0,
            od83_up_mode:           String::new(),
            od83_dn_mode:           String::new(),
            od83_up_at:             String::new(),
            od83_dn_at:             String::new(),
            tick_gap_ms:            0,
            ask_wall:               0,
            dump_score:             0,
            mid_from_entry:         0.0,
        }
    }
}

/// Single source of truth for CSV serialization (304 columns).
/// All CSV export paths (per-session file, live REST, DB fallback) use this.
impl CsvRecord {
    /// Column names in exact order matching `to_csv_fields()`.
            pub fn csv_header() -> &'static str {
        concat!(
        "time,ts_exchange,event,latencia_ms,",
        "binance_price,binance_imbalance,binance_vol_24h,btc_vol,btc_vel,btc_acel,btc_vol_ratio,",
        "bid,ask,mid,spread,bid_vol,ask_vol,imbalance,",
        "spoof,tick_gap_ms,ask_wall,dump_score,secs_left,",
        "poly_liquidity_delta,price_impact,depth_concentration,",
        "clob_trade_up,clob_trade_dn,clob_trade_up_vol,clob_trade_dn_vol,clob_trade_count_up,clob_trade_count_dn,od83_filters,",
        "od83_up,od83_up_entry,od83_up_sz,od83_up_pnl,od83_up_exit,od83_up_r,od83_up_bal,",
        "od83_dn,od83_dn_entry,od83_dn_sz,od83_dn_pnl,od83_dn_exit,od83_dn_r,od83_dn_bal,",
        "od83_event,od83_up_mode,od83_dn_mode,",
        "od83_up_at,od83_dn_at,",
        "mid_from_entry",
        )
    }    /// Returns 304 CSV fields as strings in the exact order of `csv_header()`.
    /// Used by all three CSV export paths (per-session file, live REST, DB fallback).
    pub fn to_csv_fields(&self) -> Vec<String> {
        let mut f: Vec<String> = Vec::with_capacity(64);
        // ── FASE 0: IDENTIDAD (4) ──
        f.push(self.ts_local.clone());
        f.push(self.ts_exchange.clone());
        f.push(self.event_type.as_str().to_string());
        f.push(self.latencia_ms.to_string());
        // ── FASE 1: MERCADO BTC (7) ──
        f.push(self.binance_price.to_string());
        f.push(self.binance_imbalance.to_string());
        f.push(self.binance_vol_24h.to_string());
        f.push(self.btc_vol.to_string());
        f.push(self.price_velocity.to_string());
        f.push(self.btc_acel.to_string());
        f.push(self.btc_vol_ratio.to_string());
        // ── FASE 2: ORDER BOOK (7) ──
        f.push(self.poly_bid.to_string());
        f.push(self.poly_ask.to_string());
        f.push(self.poly_mid.to_string());
        f.push(self.poly_spread.to_string());
        f.push(self.poly_bid_vol_all.to_string());
        f.push(self.poly_ask_vol_all.to_string());
        f.push(self.poly_imbalance.to_string());
        // ── FASE 3: RIESGO (7) ──
        f.push(self.spoofing_flag.to_string());
        f.push(self.tick_gap_ms.to_string());
        f.push(self.ask_wall.to_string());
        f.push(self.dump_score.to_string());
        f.push(self.pnr_seconds_left.to_string());
        // ── FASE 4: LIQUIDITY (3) ──
        f.push(self.poly_liquidity_delta.to_string());
        f.push(self.price_impact.to_string());
        f.push(self.depth_concentration.to_string());
        // ── FASE 5: TRIGGER (7) ──
        f.push(self.clob_trade_up.to_string());
        f.push(self.clob_trade_dn.to_string());
        f.push(self.clob_trade_up_vol.to_string());
        f.push(self.clob_trade_dn_vol.to_string());
        f.push(self.clob_trade_count_up.to_string());
        f.push(self.clob_trade_count_dn.to_string());
        f.push(self.od83_filters.to_string());
        // ── FASE 6: ODISEO UP (7) ──
        f.push(self.odiseo83_up_active.to_string());
        f.push(self.odiseo83_up_entry_price.to_string());
        f.push(self.odiseo83_up_size.to_string());
        f.push(self.odiseo83_up_pnl.to_string());
        f.push(self.odiseo83_up_exit_price.to_string());
        f.push(self.odiseo83_up_exit_reason.to_string());
        f.push(self.odiseo83_up_balance.to_string());
        // ── FASE 7: ODISEO DOWN (7) ──
        f.push(self.odiseo83_down_active.to_string());
        f.push(self.odiseo83_down_entry_price.to_string());
        f.push(self.odiseo83_down_size.to_string());
        f.push(self.odiseo83_down_pnl.to_string());
        f.push(self.odiseo83_down_exit_price.to_string());
        f.push(self.odiseo83_down_exit_reason.to_string());
        f.push(self.odiseo83_down_balance.to_string());
        // ── FASE 8: EVENTOS + MODE (3) ──
        f.push(self.od83_event.clone());
        f.push(self.od83_up_mode.clone());
        f.push(self.od83_dn_mode.clone());
        // ── FASE 9: TIMING (2) ──
        f.push(self.od83_up_at.clone());
        f.push(self.od83_dn_at.clone());
        // ── FASE 10: POSICIÓN (1) ──
        f.push(self.mid_from_entry.to_string());
        f
    }

    /// Convenience: `to_csv_fields().join(",")`
    #[inline]
    pub fn to_csv_line(&self) -> String {
        self.to_csv_fields().join(",")
    }
}
