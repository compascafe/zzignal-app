use serde::Serialize;
use crate::controllers::worker::PriceLevel;
use std::cell::UnsafeCell;
use std::hint;
use std::sync::atomic::{AtomicU64, Ordering};

const RING_CAP: usize = 4096;
const RING_MASK: usize = RING_CAP - 1;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BinanceState {
    pub timestamp:         u64,
    pub mid_price:         f64,
    pub micro_price:       f64,
    pub total_liquidity:   f64,
    pub binance_vol_100ms: f64,
    pub imbalance:         f32,
}

impl BinanceState {
    pub const EMPTY: Self = Self { timestamp: 0, mid_price: 0.0, micro_price: 0.0, total_liquidity: 0.0, binance_vol_100ms: 0.0, imbalance: 0.0 };
}

impl Default for BinanceState {
    fn default() -> Self { Self { timestamp: 0, mid_price: 0.0, micro_price: 0.0, total_liquidity: 0.0, binance_vol_100ms: 0.0, imbalance: 0.0 } }
}

pub struct PriceRingBuffer {
    slots: Box<[UnsafeCell<BinanceState>]>,
    write_seq: AtomicU64,
}

unsafe impl Send for PriceRingBuffer {}
unsafe impl Sync for PriceRingBuffer {}

impl PriceRingBuffer {
    pub fn new() -> Self {
        let mut vec = Vec::with_capacity(RING_CAP);
        for _ in 0..RING_CAP { vec.push(UnsafeCell::new(BinanceState::default())); }
        Self { slots: vec.into_boxed_slice(), write_seq: AtomicU64::new(0) }
    }
    pub fn push(&self, state: BinanceState) {
        let seq = self.write_seq.fetch_add(1, Ordering::Release);
        let idx = (seq as usize) & RING_MASK;
        unsafe { self.slots[idx].get().write(state); }
    }
    pub fn get_closest_to(&self, target_ts: u64) -> Option<BinanceState> {
        let mut seq = self.write_seq.load(Ordering::Acquire);
        if seq == 0 {
            for _ in 0..16 { hint::spin_loop(); seq = self.write_seq.load(Ordering::Acquire); if seq != 0 { break; } }
            if seq == 0 { return None; }
        }
        let count = seq.min(RING_CAP as u64);
        let base = seq.wrapping_sub(count);
        let mut lo: u64 = 0;
        let mut hi: u64 = count.saturating_sub(1);
        let ts_lo = unsafe { (*self.slots[((base + lo) as usize) & RING_MASK].get()).timestamp };
        let ts_hi = unsafe { (*self.slots[((base + hi) as usize) & RING_MASK].get()).timestamp };
        if target_ts <= ts_lo { return Some(unsafe { *self.slots[((base + lo) as usize) & RING_MASK].get() }); }
        if target_ts >= ts_hi { return Some(unsafe { *self.slots[((base + hi) as usize) & RING_MASK].get() }); }
        while lo + 1 < hi {
            let mid = lo + (hi - lo) / 2;
            let phys = ((base + mid) as usize) & RING_MASK;
            let ts_mid = unsafe { (*self.slots[phys].get()).timestamp };
            if ts_mid <= target_ts { lo = mid; } else { hi = mid; }
        }
        let state_lo = unsafe { *self.slots[((base + lo) as usize) & RING_MASK].get() };
        let state_hi = unsafe { *self.slots[((base + hi) as usize) & RING_MASK].get() };
        let dist_lo = if target_ts >= state_lo.timestamp { target_ts - state_lo.timestamp } else { state_lo.timestamp - target_ts };
        let dist_hi = if target_ts >= state_hi.timestamp { target_ts - state_hi.timestamp } else { state_hi.timestamp - target_ts };
        Some(if dist_lo <= dist_hi { state_lo } else { state_hi })
    }
    pub fn latest(&self) -> Option<BinanceState> {
        let seq = self.write_seq.load(Ordering::Acquire);
        if seq == 0 { return None; }
        unsafe { Some(*self.slots[((seq.wrapping_sub(1)) as usize) & RING_MASK].get()) }
    }
    pub fn len(&self) -> usize { (self.write_seq.load(Ordering::Acquire) as usize).min(RING_CAP) }
    pub fn clear(&self) {
        for i in 0..RING_CAP { unsafe { *self.slots[i].get() = BinanceState::EMPTY; } }
        self.write_seq.store(0, Ordering::Release);
    }
}

impl Default for PriceRingBuffer {
    fn default() -> Self { Self::new() }
}

/// Snapshot completo del orderbook de Polymarket
#[derive(Debug, Clone, Serialize)]
pub struct PolyDepthFrame {
    pub ts_unix_ms:  i64,
    pub side:        u8,
    pub bids:        Vec<PriceLevel>,
    pub asks:        Vec<PriceLevel>,
}

/// Snapshot del order book de Binance (top 20 niveles)
#[derive(Debug, Clone)]
pub struct BinanceDepth {
    pub last_update_id: u64,
    pub bids:           Vec<PriceLevel>,
    pub asks:           Vec<PriceLevel>,
    pub event_time:     i64,
    pub local_time:     i64,
    pub btc_price:      f64,
    pub btc_volume_24h: f64,
}

impl Default for BinanceDepth {
    fn default() -> Self {
        Self { last_update_id: 0, bids: vec![], asks: vec![], event_time: 0, local_time: 0, btc_price: 0.0, btc_volume_24h: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum EventType {
    BookUpdate,
    Trade,
    BinanceTick,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self { Self::BookUpdate => "BOOK_UPDATE", Self::Trade => "TRADE", Self::BinanceTick => "BINANCE_TICK" }
    }
}

/// CSV record — 27 essential columns
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
    pub poly_bid:            f64,
    pub poly_ask:            f64,
    pub poly_mid:            f64,
    pub poly_spread:         f64,
    pub poly_bid_vol_all:    f64,
    pub poly_ask_vol_all:    f64,
    pub poly_imbalance:      f64,
    pub session_id:          i32,
    pub price_velocity:       f64,
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
}

impl Default for CsvRecord {
    fn default() -> Self {
        Self {
            ts_local: String::new(), ts_exchange: String::new(), event_type: EventType::BookUpdate,
            latencia_ms: 0, binance_price: 0.0, binance_imbalance: 0.0, binance_vol_24h: 0.0,
            btc_vol: 0.0, poly_bid: 0.0, poly_ask: 0.0, poly_mid: 0.0,
            poly_spread: 0.0, poly_bid_vol_all: 0.0, poly_ask_vol_all: 0.0,
            poly_imbalance: 0.0, session_id: 0, price_velocity: 0.0,
            spoofing_flag: 0, pnr_seconds_left: 0,
            clob_trade_up: 0.0, clob_trade_dn: 0.0, clob_trade_up_vol: 0.0, clob_trade_dn_vol: 0.0,
            clob_trade_count_up: 0, clob_trade_count_dn: 0,
            tick_gap_ms: 0, ask_wall: 0, dump_score: 0,
        }
    }
}

impl CsvRecord {
    pub fn csv_header() -> &'static str {
        concat!(
        "time,ts_exchange,event,latencia_ms,",
        "binance_price,binance_imbalance,binance_vol_24h,btc_vol,btc_vel,",
        "bid,ask,mid,spread,bid_vol,ask_vol,imbalance,",
        "spoof,tick_gap_ms,ask_wall,dump_score,secs_left,",
        "clob_trade_up,clob_trade_dn,clob_trade_up_vol,clob_trade_dn_vol,clob_trade_count_up,clob_trade_count_dn",
        )
    }

    pub fn to_csv_line(&self) -> String {
        use std::fmt::Write;
        let mut s = String::with_capacity(400);
        let _ = write!(s, "{},{},{},{},", self.ts_local, self.ts_exchange, self.event_type.as_str(), self.latencia_ms);
        let _ = write!(s, "{},{},{},{},{},", self.binance_price, self.binance_imbalance, self.binance_vol_24h, self.btc_vol, self.price_velocity);
        let _ = write!(s, "{},{},{},{},{},{},{},", self.poly_bid, self.poly_ask, self.poly_mid, self.poly_spread, self.poly_bid_vol_all, self.poly_ask_vol_all, self.poly_imbalance);
        let _ = write!(s, "{},{},{},{},{},", self.spoofing_flag, self.tick_gap_ms, self.ask_wall, self.dump_score, self.pnr_seconds_left);
        let _ = write!(s, "{},{},{},{},{},{}", self.clob_trade_up, self.clob_trade_dn, self.clob_trade_up_vol, self.clob_trade_dn_vol, self.clob_trade_count_up, self.clob_trade_count_dn);
        s
    }
}
