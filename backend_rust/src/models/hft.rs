use serde::Serialize;
use crate::controllers::worker::PriceLevel;

pub use crate::utils::ring_buffer::{BinanceState, PriceRingBuffer};

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

/// Registro unificado para CSV (33 columnas esenciales — mercado, riesgo, liquidez).
#[derive(Debug, Clone)]
pub struct CsvRecord {
    pub ts_local:            String,
    pub ts_exchange:         String,
    pub event_type:          EventType,
    pub latencia_ms:         i64,
    pub binance_price:       f64,
    pub binance_imbalance:   f32,
    pub binance_vol_24h:     f64,
    pub btc_vol:             f64,
    pub btc_volatility:       f64,
    pub poly_bid:            f64,
    pub poly_ask:            f64,
    pub poly_mid:            f64,
    pub poly_spread:         f64,
    pub poly_bid_vol_all:    f64,
    pub poly_ask_vol_all:    f64,
    pub poly_imbalance:      f64,
    pub session_id:          i32,
    pub price_velocity:       f64,
    pub btc_acel:             f64,
    pub btc_vol_ratio:        f64,
    pub price_impact:         f64,
    pub depth_concentration:  f64,
    pub spoofing_flag:        u8,
    pub pnr_seconds_left:       i32,
    pub clob_trade_up:         f64,
    pub clob_trade_dn:         f64,
    pub clob_trade_up_vol:     f64,
    pub clob_trade_dn_vol:     f64,
    pub clob_trade_count_up:  u16,
    pub clob_trade_count_dn:  u16,
    pub tick_gap_ms:            i64,
    pub ask_wall:               u8,
    pub dump_score:             u8,
    pub token_momentum:         f64,
    // ─── Cross-book imbalance depth profile ────────────────────────────────
    pub comb_imb_d10:           f64,  // cross-book imbalance at 10 levels
    pub comb_imb_d20:           f64,  // cross-book imbalance at 20 levels
    pub comb_imb_d30:           f64,  // cross-book imbalance at 30 levels
    pub imb_gradient:           f64,  // d30 - d10 (>0=deep bullish, <0=bearish hidden)
    pub imb_velocity:           f64,  // Δcomb_imb_d10 / Δs
    pub imb_accel:              f64,  // Δvelocity / Δs
    pub wall_score:             f64,  // max(level_vol / avg_vol) in best 30 levels
    pub wall_side:              u8,   // 0=none 1=up_bid 2=up_ask 3=dn_bid 4=dn_ask
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
            btc_volatility:       0.0,
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
            price_impact:         0.0,
            depth_concentration:  0.5,
            spoofing_flag:        0,
            pnr_seconds_left:       0,
            clob_trade_up:        0.0,
            clob_trade_dn:        0.0,
            clob_trade_up_vol:    0.0,
            clob_trade_dn_vol:    0.0,
            clob_trade_count_up: 0,
            clob_trade_count_dn: 0,
            tick_gap_ms:            0,
            ask_wall:               0,
            dump_score:             0,
            token_momentum:         0.0,
            comb_imb_d10:           1.0,
            comb_imb_d20:           1.0,
            comb_imb_d30:           1.0,
            imb_gradient:           0.0,
            imb_velocity:           0.0,
            imb_accel:              0.0,
            wall_score:             0.0,
            wall_side:              0,
        }
    }
}

/// Single source of truth for CSV serialization (41 columns).
/// All CSV export paths (per-session file, live REST, DB fallback) use this.
impl CsvRecord {
    /// Column names in exact order matching `to_csv_fields()`.
            pub fn csv_header() -> &'static str {
        concat!(
        "time,ts_exchange,event,latencia_ms,",
        "binance_price,binance_imbalance,binance_vol_24h,btc_vol,btc_vel,btc_acel,btc_volatility,btc_vol_ratio,",
        "bid,ask,mid,spread,bid_vol,ask_vol,imbalance,",
        "spoof,tick_gap_ms,ask_wall,dump_score,secs_left,",
        "price_impact,depth_concentration,",
        "clob_trade_up,clob_trade_dn,clob_trade_up_vol,clob_trade_dn_vol,clob_trade_count_up,clob_trade_count_dn,",
        "token_momentum,",
        "comb_imb_d10,comb_imb_d20,comb_imb_d30,imb_gradient,imb_velocity,imb_accel,wall_score,wall_side",
        )
    }    /// Returns 41 CSV fields as strings in the exact order of `csv_header()`.
    /// Used by all three CSV export paths (per-session file, live REST, DB fallback).
    pub fn to_csv_fields(&self) -> Vec<String> {
        let mut f: Vec<String> = Vec::with_capacity(41);
        // ── FASE 0: IDENTIDAD (4) ──
        f.push(self.ts_local.clone());
        f.push(self.ts_exchange.clone());
        f.push(self.event_type.as_str().to_string());
        f.push(self.latencia_ms.to_string());
        // ── FASE 1: MERCADO BTC (8) ──
        f.push(self.binance_price.to_string());
        f.push(self.binance_imbalance.to_string());
        f.push(self.binance_vol_24h.to_string());
        f.push(self.btc_vol.to_string());
        f.push(self.price_velocity.to_string());
        f.push(self.btc_acel.to_string());
        f.push(self.btc_volatility.to_string());
        f.push(self.btc_vol_ratio.to_string());
        // ── FASE 2: ORDER BOOK (7) ──
        f.push(self.poly_bid.to_string());
        f.push(self.poly_ask.to_string());
        f.push(self.poly_mid.to_string());
        f.push(self.poly_spread.to_string());
        f.push(self.poly_bid_vol_all.to_string());
        f.push(self.poly_ask_vol_all.to_string());
        f.push(self.poly_imbalance.to_string());
        // ── FASE 3: RIESGO (5) ──
        f.push(self.spoofing_flag.to_string());
        f.push(self.tick_gap_ms.to_string());
        f.push(self.ask_wall.to_string());
        f.push(self.dump_score.to_string());
        f.push(self.pnr_seconds_left.to_string());
        // ── FASE 4: LIQUIDEZ (2) ──
        f.push(self.price_impact.to_string());
        f.push(self.depth_concentration.to_string());
        // ── FASE 5: TRIGGER (6) ──
        f.push(self.clob_trade_up.to_string());
        f.push(self.clob_trade_dn.to_string());
        f.push(self.clob_trade_up_vol.to_string());
        f.push(self.clob_trade_dn_vol.to_string());
        f.push(self.clob_trade_count_up.to_string());
        f.push(self.clob_trade_count_dn.to_string());
        // ── FASE 6: MOMENTUM (1) ──
        f.push(self.token_momentum.to_string());
        // ── FASE 7: DEPTH PROFILE (8) ──
        f.push(self.comb_imb_d10.to_string());
        f.push(self.comb_imb_d20.to_string());
        f.push(self.comb_imb_d30.to_string());
        f.push(self.imb_gradient.to_string());
        f.push(self.imb_velocity.to_string());
        f.push(self.imb_accel.to_string());
        f.push(self.wall_score.to_string());
        f.push(self.wall_side.to_string());
        f
    }

    /// Convenience: join all fields with commas.
    pub fn to_csv_line(&self) -> String {
        self.to_csv_fields().join(",")
    }
}

