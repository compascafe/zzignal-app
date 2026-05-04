//! Binance BTC Feed — Price, Volume, Depth in Memory
//!
//! Layer 1: Raw Market Data. Wraps the lock-free ring buffer + WebSocket depth
//! from Binance, providing a clean access interface for upper layers.
//!
//! Responsibilities:
//!   - Expose latest BTC price, micro-price, volume, imbalance
//!   - Timestamp-based lookback (cross-exchange latency calculation)
//!   - NEVER compute trading signals (that's Layer 2/3)

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::modules::hft::types::{BinanceDepth, BinanceState, PriceRingBuffer};

// ─── Binance Feed ──────────────────────────────────────────────────────────

/// Unified access to real-time Binance BTC data.
/// Wraps the raw depth snapshot + lock-free ring buffer.
pub struct BinanceFeed {
    pub depth:        Arc<RwLock<Option<BinanceDepth>>>,
    pub ring:         Arc<PriceRingBuffer>,
}

impl BinanceFeed {
    pub fn new(
        depth: Arc<RwLock<Option<BinanceDepth>>>,
        ring:  Arc<PriceRingBuffer>,
    ) -> Self {
        Self { depth, ring }
    }

    /// Latest BTC mid price from depth snapshot (or 0.0 if not connected).
    pub async fn btc_price(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| d.btc_price).unwrap_or(0.0)
    }

    /// Latest BTC 24h volume.
    pub async fn btc_vol_24h(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| d.btc_volume_24h).unwrap_or(0.0)
    }

    /// Latest Binance mid-price (best bid + best ask) / 2.
    pub async fn mid_price(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| (d.bids.first().map(|l| l.price).unwrap_or(0.0)
                    + d.asks.first().map(|l| l.price).unwrap_or(0.0)) / 2.0)
            .unwrap_or(0.0)
    }

    /// Volume-weighted micro price from the last depth snapshot.
    pub async fn micro_price(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| {
                // Top 5 weighted mid
                let b5: f64 = d.bids.iter().take(5).map(|l| l.price * l.size).sum();
                let a5: f64 = d.asks.iter().take(5).map(|l| l.price * l.size).sum();
                let v5: f64 = d.bids.iter().take(5).map(|l| l.size).sum::<f64>()
                            + d.asks.iter().take(5).map(|l| l.size).sum::<f64>();
                if v5 > 0.0 { (b5 + a5) / v5 } else { 0.0 }
            })
            .unwrap_or(0.0)
    }

    /// Depth imbalance from latest snapshot.
    pub async fn imbalance(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| {
                let bv: f64 = d.bids.iter().map(|l| l.size).sum();
                let av: f64 = d.asks.iter().map(|l| l.size).sum();
                let total = bv + av;
                if total > 0.0 { (bv - av) / total } else { 0.0 }
            })
            .unwrap_or(0.0)
    }

    /// Total liquidity (bid + ask volume from top 20).
    pub async fn total_liquidity(&self) -> f64 {
        self.depth.read().await.as_ref()
            .map(|d| {
                let bv: f64 = d.bids.iter().take(20).map(|l| l.size).sum();
                let av: f64 = d.asks.iter().take(20).map(|l| l.size).sum();
                bv + av
            })
            .unwrap_or(0.0)
    }

    /// Closest ring buffer entry to a given timestamp (for cross-exchange latency).
    pub fn closest_to_ts(&self, ts_ms: i64) -> Option<BinanceState> {
        self.ring.get_closest_to(ts_ms as u64)
    }

    /// Latest entry from the ring buffer.
    pub fn latest_ring(&self) -> Option<BinanceState> {
        self.ring.latest()
    }

    /// Number of entries in the ring buffer.
    pub fn ring_len(&self) -> usize {
        self.ring.len()
    }

    /// Clone the latest full depth snapshot.
    pub async fn depth_snapshot(&self) -> Option<BinanceDepth> {
        self.depth.read().await.clone()
    }
}
