use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Vela de order book para un timeframe específico.
/// Una por (interval, side, open_time).
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OrderBookCandle {
    pub id:          i64,
    pub interval:    String,       // "1m", "5m", "15m", "1h", "4h", "1d"
    pub side:        String,       // "up" | "down"
    pub open_time:   DateTime<Utc>,

    // Best Bid OHLC
    pub bid_open:    f64,
    pub bid_high:    f64,
    pub bid_low:     f64,
    pub bid_close:   f64,

    // Best Ask OHLC
    pub ask_open:    f64,
    pub ask_high:    f64,
    pub ask_low:     f64,
    pub ask_close:   f64,

    // Spread OHLC
    pub spread_open:  f64,
    pub spread_high:  f64,
    pub spread_low:   f64,
    pub spread_close: f64,

    // Mid Price OHLC
    pub mid_open:    f64,
    pub mid_high:    f64,
    pub mid_low:     f64,
    pub mid_close:   f64,

    /// Suma del tamaño de los bids en los top N niveles del book
    pub bid_volume:  f64,
    /// Suma del tamaño de los asks en los top N niveles del book
    pub ask_volume:  f64,

    /// Cuántos ticks (actualizaciones del book) se recibieron en este período
    pub tick_count:  i32,

    pub created_at:  DateTime<Utc>,
}

/// Estado interno de la vela en construcción (no se persiste)
#[derive(Debug, Clone)]
pub struct CandleBuilder {
    pub interval:    String,
    pub side:        String,
    pub open_time:   DateTime<Utc>,

    pub bid_open:    f64,
    pub bid_high:    f64,
    pub bid_low:     f64,

    pub ask_open:    f64,
    pub ask_high:    f64,
    pub ask_low:     f64,

    pub spread_open:  f64,
    pub spread_high:  f64,
    pub spread_low:   f64,

    pub mid_open:    f64,
    pub mid_high:    f64,
    pub mid_low:     f64,

    pub bid_volume:  f64,
    pub ask_volume:  f64,

    pub tick_count:  i32,

    pub last_bid:    f64,
    pub last_ask:    f64,
    pub last_spread: f64,
    pub last_mid:    f64,
}

impl CandleBuilder {
    pub fn new(interval: String, side: String, open_time: DateTime<Utc>, bid: f64, ask: f64) -> Self {
        let spread = ask - bid;
        let mid = (bid + ask) / 2.0;
        Self {
            interval: interval.clone(),
            side,
            open_time,
            bid_open: bid, bid_high: bid, bid_low: bid,
            ask_open: ask, ask_high: ask, ask_low: ask,
            spread_open: spread, spread_high: spread, spread_low: spread,
            mid_open: mid, mid_high: mid, mid_low: mid,
            bid_volume: 0.0,
            ask_volume: 0.0,
            tick_count: 1,
            last_bid: bid, last_ask: ask, last_spread: spread, last_mid: mid,
        }
    }

    /// Actualiza la vela con un nuevo tick del order book
    pub fn update(&mut self, bid: f64, ask: f64, bid_vol: f64, ask_vol: f64) {
        let spread = ask - bid;
        let mid = (bid + ask) / 2.0;

        self.bid_high   = self.bid_high.max(bid);
        self.bid_low    = self.bid_low.min(bid);
        self.ask_high   = self.ask_high.max(ask);
        self.ask_low    = self.ask_low.min(ask);
        self.spread_high = self.spread_high.max(spread);
        self.spread_low  = self.spread_low.min(spread);
        self.mid_high   = self.mid_high.max(mid);
        self.mid_low    = self.mid_low.min(mid);

        self.bid_volume += bid_vol;
        self.ask_volume += ask_vol;
        self.tick_count += 1;

        self.last_bid = bid;
        self.last_ask = ask;
        self.last_spread = spread;
        self.last_mid = mid;
    }

    /// Finaliza la vela y la convierte al tipo persistible
    pub fn finish(self) -> OrderBookCandle {
        OrderBookCandle {
            id: 0, // asignado por la DB
            interval: self.interval,
            side: self.side,
            open_time: self.open_time,
            bid_open: self.bid_open,
            bid_high: self.bid_high,
            bid_low: self.bid_low,
            bid_close: self.last_bid,
            ask_open: self.ask_open,
            ask_high: self.ask_high,
            ask_low: self.ask_low,
            ask_close: self.last_ask,
            spread_open: self.spread_open,
            spread_high: self.spread_high,
            spread_low: self.spread_low,
            spread_close: self.last_spread,
            mid_open: self.mid_open,
            mid_high: self.mid_high,
            mid_low: self.mid_low,
            mid_close: self.last_mid,
            bid_volume: self.bid_volume,
            ask_volume: self.ask_volume,
            tick_count: self.tick_count,
            created_at: Utc::now(),
        }
    }
}

/// Los timeframes que soporta el Collector
pub const TIMEFRAMES: &[(&str, i64)] = &[
    ("1m",  60),
    ("5m",  300),
    ("15m", 900),
    ("1h",  3600),
    ("4h",  14400),
    ("1d",  86400),
];
