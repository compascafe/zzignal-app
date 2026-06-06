// BTC price state — single source of truth for all BTC-related display values
// Feeds: BTC panel, BTC Δ panel, S3 Δ indicator, velocity/acceleration/volatility

pub struct BtcState {
    pub price:      f64,  // current BTC price
    pub open:       f64,  // session opening price (price to beat)
    pub velocity:   f64,  // USD/s
    pub accel:      f64,  // USD/s²
    pub volatility: f64,  // EMA of |velocity|
    pub vol_1m:     f64,  // volume in last 60s
    pub vol_ses:    f64,  // cumulative volume since session start
    prev_price:     f64,
    prev_time_ms:   i64,
}

impl BtcState {
    pub fn new() -> Self {
        Self {
            price: 0.0, open: 0.0, velocity: 0.0, accel: 0.0,
            volatility: 0.0, vol_1m: 0.0, vol_ses: 0.0,
            prev_price: 0.0, prev_time_ms: 0,
        }
    }

    /// Call on every new price tick from REST poll or WS
    pub fn update_price(&mut self, price: f64, ts_ms: i64) {
        if self.prev_price > 0.0 && self.prev_time_ms > 0 && ts_ms > self.prev_time_ms {
            let dt = ((ts_ms - self.prev_time_ms) as f64 / 1000.0).max(0.1);
            self.velocity = (price - self.prev_price) / dt;
            let alpha = 0.1;
            self.volatility = alpha * self.velocity.abs() + (1.0 - alpha) * self.volatility;
            if self.prev_price > 0.0 {
                self.accel = (self.velocity - ((self.prev_price - if self.price > 0.0 { self.price } else { self.prev_price }) / dt)) / dt;
            }
        }
        self.prev_price = self.price;
        self.price = price;
        self.prev_time_ms = ts_ms;
    }

    /// Set the session opening price
    pub fn set_open(&mut self, open: f64) {
        if open > 0.0 { self.open = open; }
    }

    /// Reference price for delta: session open > current price
    pub fn reference(&self) -> f64 {
        if self.open > 0.0 { self.open } else { self.price }
    }

    /// Delta: current - reference
    pub fn delta(&self) -> f64 { self.price - self.reference() }

    /// Delta percentage
    pub fn delta_pct(&self) -> f64 {
        let r = self.reference();
        if r > 0.0 { (self.price / r - 1.0) * 100.0 } else { 0.0 }
    }
}
