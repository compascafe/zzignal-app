//! Polymarket Orderbook — Full Depth in Memory
//!
//! Layer 1: Raw Market Data. Stores every bid/ask level from CLOB WebSocket.
//! Two independent buffers (UP + DOWN), each holding the last N snapshots.
//!
//! Responsibilities:
//!   - Receive BOOK_UPDATE from CLOB WS → push full snapshot
//!   - Provide accessors: best_bid, best_ask, mid, spread, depth profile
//!   - NEVER compute trading signals (that's Layer 2/3)
//!
//! Memory: ~300 frames × 2 sides × avg 50 levels × 2 (bid+ask) × 16 bytes ≈ 1 MB

use std::collections::VecDeque;

use serde::Serialize;

use crate::modules::core::worker::PriceLevel;

/// Maximum snapshots retained per side (~5 minutes at 1 update/s).
const MAX_FRAMES: usize = 300;

// ─── Frame ─────────────────────────────────────────────────────────────────

/// Snapshot completo del orderbook de Polymarket (todos los niveles).
#[derive(Debug, Clone, Serialize)]
pub struct PolyDepthFrame {
    pub ts_unix_ms: i64,
    pub side: u8, // 0 = UP, 1 = DOWN
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
}

// ─── Top-of-Book Snapshot ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TopOfBook {
    pub best_bid: f64,
    pub best_ask: f64,
    pub bid_vol: f64, // total bid volume (all levels)
    pub ask_vol: f64, // total ask volume (all levels)
    pub mid: f64,
    pub spread: f64,
    pub imbalance: f64, // bid_vol / ask_vol (clamped)
    pub ts_unix_ms: i64,
}

impl Default for TopOfBook {
    fn default() -> Self {
        Self {
            best_bid: 0.0, best_ask: 0.0,
            bid_vol: 0.0, ask_vol: 0.0,
            mid: 0.0, spread: 0.0, imbalance: 1.0,
            ts_unix_ms: 0,
        }
    }
}

// ─── Depth Profile ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DepthProfile {
    pub side: u8,
    pub ts_unix_ms: i64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub mid: f64,
    pub spread: f64,
    pub spread_pct: f64, // spread / mid * 100
    pub bid_vol_total: f64,
    pub ask_vol_total: f64,
    pub bid_vol_top5: f64,
    pub ask_vol_top5: f64,
    pub imbalance: f64,
    pub bid_levels: usize, // number of bid levels
    pub ask_levels: usize, // number of ask levels
    pub bids: Vec<PriceLevel>, // top N levels (for display)
    pub asks: Vec<PriceLevel>,
}

// ─── Orderbook Manager ─────────────────────────────────────────────────────

/// Buffer circular con snapshots del orderbook de Polymarket.
/// Un lado (UP o DOWN) por instancia.
pub struct PolyOrderbookSide {
    frames: VecDeque<PolyDepthFrame>,
}

impl PolyOrderbookSide {
    pub fn new() -> Self {
        Self { frames: VecDeque::with_capacity(MAX_FRAMES) }
    }

    /// Push a new full-depth snapshot from CLOB WebSocket.
    pub fn push(&mut self, ts_unix_ms: i64, side: u8, bids: Vec<PriceLevel>, asks: Vec<PriceLevel>) {
        if self.frames.len() >= MAX_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(PolyDepthFrame { ts_unix_ms, side, bids, asks });
    }

    /// Most recent snapshot (cloned).
    pub fn latest(&self) -> Option<&PolyDepthFrame> {
        self.frames.back()
    }

    /// Last N snapshots, most recent first.
    pub fn recent(&self, n: usize) -> Vec<&PolyDepthFrame> {
        self.frames.iter().rev().take(n).collect()
    }

    /// Number of stored frames.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Top-of-book from the latest frame.
    pub fn top_of_book(&self) -> TopOfBook {
        match self.frames.back() {
            None => TopOfBook::default(),
            Some(f) => {
                let best_bid = f.bids.first().map(|l| l.price).unwrap_or(0.0);
                let best_ask = f.asks.first().map(|l| l.price).unwrap_or(0.0);
                let bid_vol: f64 = f.bids.iter().map(|l| l.size).sum();
                let ask_vol: f64 = f.asks.iter().map(|l| l.size).sum();
                let mid = if best_bid > 0.0 && best_ask > 0.0 {
                    (best_bid + best_ask) / 2.0
                } else if best_bid > 0.0 {
                    best_bid
                } else if best_ask > 0.0 {
                    best_ask
                } else {
                    0.0
                };
                let spread = if best_bid > 0.0 && best_ask > 0.0 {
                    best_ask - best_bid
                } else {
                    0.0
                };
                let imbalance = if ask_vol > 0.0 { bid_vol / ask_vol } else { 1.0 };
                TopOfBook {
                    best_bid, best_ask, bid_vol, ask_vol,
                    mid, spread,
                    imbalance: if imbalance.is_finite() { imbalance } else { 0.0 },
                    ts_unix_ms: f.ts_unix_ms,
                }
            }
        }
    }

    /// Full depth profile from latest frame (up to max_levels for display).
    pub fn depth_profile(&self, max_levels: usize) -> DepthProfile {
        match self.frames.back() {
            None => DepthProfile {
                side: 0, ts_unix_ms: 0,
                best_bid: 0.0, best_ask: 0.0, mid: 0.0, spread: 0.0, spread_pct: 0.0,
                bid_vol_total: 0.0, ask_vol_total: 0.0,
                bid_vol_top5: 0.0, ask_vol_top5: 0.0,
                imbalance: 1.0,
                bid_levels: 0, ask_levels: 0,
                bids: vec![], asks: vec![],
            },
            Some(f) => {
                let best_bid = f.bids.first().map(|l| l.price).unwrap_or(0.0);
                let best_ask = f.asks.first().map(|l| l.price).unwrap_or(0.0);
                let mid = if best_bid > 0.0 && best_ask > 0.0 {
                    (best_bid + best_ask) / 2.0
                } else if best_bid > 0.0 { best_bid } else if best_ask > 0.0 { best_ask } else { 0.0 };
                let spread = if best_bid > 0.0 && best_ask > 0.0 { best_ask - best_bid } else { 0.0 };
                let spread_pct = if mid > 0.0 { (spread / mid) * 100.0 } else { 0.0 };
                let bid_vol_total: f64 = f.bids.iter().map(|l| l.size).sum();
                let ask_vol_total: f64 = f.asks.iter().map(|l| l.size).sum();
                let bid_vol_top5: f64 = f.bids.iter().take(5).map(|l| l.size).sum();
                let ask_vol_top5: f64 = f.asks.iter().take(5).map(|l| l.size).sum();
                let imbalance = if ask_vol_total > 0.0 { bid_vol_total / ask_vol_total } else { 1.0 };
                DepthProfile {
                    side: f.side, ts_unix_ms: f.ts_unix_ms,
                    best_bid, best_ask, mid, spread, spread_pct,
                    bid_vol_total, ask_vol_total,
                    bid_vol_top5, ask_vol_top5,
                    imbalance: if imbalance.is_finite() { imbalance } else { 0.0 },
                    bid_levels: f.bids.len(), ask_levels: f.asks.len(),
                    bids: f.bids.iter().take(max_levels).cloned().collect(),
                    asks: f.asks.iter().take(max_levels).cloned().collect(),
                }
            }
        }
    }

    /// Time-weighted average mid price over last window_ms milliseconds.
    pub fn twap_mid(&self, window_ms: i64) -> f64 {
        let now = self.frames.back().map(|f| f.ts_unix_ms).unwrap_or(0);
        let cutoff = now.saturating_sub(window_ms);
        let mut weighted_sum = 0.0;
        let mut total_time = 0;
        let mut prev_ts = cutoff;
        for f in &self.frames {
            if f.ts_unix_ms < cutoff { continue; }
            let dt = f.ts_unix_ms - prev_ts;
            if dt <= 0 { continue; }
            let best_bid = f.bids.first().map(|l| l.price).unwrap_or(0.0);
            let best_ask = f.asks.first().map(|l| l.price).unwrap_or(0.0);
            let mid = if best_bid > 0.0 && best_ask > 0.0 {
                (best_bid + best_ask) / 2.0
            } else if best_bid > 0.0 { best_bid } else if best_ask > 0.0 { best_ask } else { 0.0 };
            weighted_sum += mid * dt as f64;
            total_time += dt;
            prev_ts = f.ts_unix_ms;
        }
        if total_time > 0 { weighted_sum / total_time as f64 } else { 0.0 }
    }
}

// ─── Combined Orderbook (UP + DOWN) ────────────────────────────────────────

/// Orderbook completo de Polymarket: lado UP + lado DOWN.
/// Cada lado mantiene su propio buffer circular independiente.
pub struct PolyOrderbook {
    pub up:   PolyOrderbookSide,
    pub down: PolyOrderbookSide,
}

impl PolyOrderbook {
    pub fn new() -> Self {
        Self {
            up:   PolyOrderbookSide::new(),
            down: PolyOrderbookSide::new(),
        }
    }

    /// Push a snapshot for a specific side (0=UP, 1=DOWN).
    pub fn push_update(&mut self, ts_unix_ms: i64, side: u8, bids: Vec<PriceLevel>, asks: Vec<PriceLevel>) {
        match side {
            0 => self.up.push(ts_unix_ms, side, bids, asks),
            1 => self.down.push(ts_unix_ms, side, bids, asks),
            _ => {}
        }
    }

    /// Get top-of-book for both sides simultaneously.
    pub fn top_of_book_both(&self) -> (TopOfBook, TopOfBook) {
        (self.up.top_of_book(), self.down.top_of_book())
    }

    /// Total frames across both sides.
    pub fn total_frames(&self) -> usize {
        self.up.len() + self.down.len()
    }
}

impl Default for PolyOrderbook {
    fn default() -> Self { Self::new() }
}
