//! Hydra 85 — T-5 Certainty Strategy (Wisdom v2)
//! Entry at T-300s when poly_mid > 0.85 (UP) or < 0.15 (DOWN).

use chrono::{DateTime, Utc};
use crate::modules::hft::strategy_framework::{StrategyEngine, StrategyParams, StrategySession};

pub type T5Snapshot = StrategySession;

pub struct T5Manager {
    engine: StrategyEngine,
}

impl T5Manager {
    pub fn new() -> Self {
        Self {
            engine: StrategyEngine::new(StrategyParams {
                name: "Hydra 85".into(), code: "t5".into(),
                seconds_before_end: 300, min_up: 0.85, min_down: 0.15,
                take_profit_up: 0.95, take_profit_down: 0.05,
                reversal_stop: 0.03, max_volatility: 0.03, volatility_window: 120,
            }),
        }
    }
    pub fn on_session_start(&self, sid: i32, end: DateTime<Utc>) { self.engine.on_session_start(sid, end); }
    pub fn on_tick(&self, now: DateTime<Utc>, sid: i32, mid: f64, bid: f64, ask: f64) -> Option<T5Snapshot> {
        self.engine.on_tick(now, sid, mid, bid, ask)
    }
    pub fn track_volatility(&self, sid: i32, mid: f64) { self.engine.track_volatility(sid, mid); }
    pub fn on_session_close(&self, sid: i32, outcome: &str, poly: f64) -> Option<T5Snapshot> {
        self.engine.on_session_close(sid, outcome, poly)
    }
    pub fn get_prediction(&self, sid: i32) -> (String, f64, bool) {
        let (p, e, a) = self.engine.get_csv_fields(sid);
        (p, e, a > 0)
    }
    pub fn seconds_left(&self, sid: i32) -> i64 { self.engine.seconds_left(sid) }
    pub fn accuracy(&self) -> f64 { self.engine.accuracy() }
    pub fn cumulative_pnl(&self) -> f64 { self.engine.cumulative_pnl() }
    pub fn export_wisdom2(&self) -> String { self.engine.export_json() }
}
