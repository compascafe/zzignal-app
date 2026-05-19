//! Hydra 90 — T-3 Aggressive Strategy (Wisdom v3)
//! Entry at T-180s when poly_mid >= 0.90 (UP) or <= 0.10 (DOWN).

use chrono::{DateTime, Utc};
use crate::services::strategies::framework::{StrategyEngine, StrategyParams, StrategySession};

pub type T3Snapshot = StrategySession;

pub struct T3Manager {
    engine: StrategyEngine,
}

impl T3Manager {
    pub fn new() -> Self {
        Self {
            engine: StrategyEngine::new(StrategyParams {
                name: "Hydra 90".into(), code: "t3".into(),
                seconds_before_end: 180, min_up: 0.90, min_down: 0.10,
                take_profit_up: 0.95, take_profit_down: 0.05,
                reversal_stop: 0.015, max_volatility: 0.02, volatility_window: 60,
            }),
        }
    }
    pub fn on_session_start(&self, sid: i32, end: DateTime<Utc>) { self.engine.on_session_start(sid, end); }
    pub fn on_tick(&self, now: DateTime<Utc>, sid: i32, mid: f64, bid: f64, ask: f64) -> Option<T3Snapshot> {
        self.engine.on_tick(now, sid, mid, bid, ask)
    }
    pub fn track_volatility(&self, sid: i32, mid: f64) { self.engine.track_volatility(sid, mid); }
    pub fn on_session_close(&self, sid: i32, outcome: &str, poly: f64) -> Option<T3Snapshot> {
        self.engine.on_session_close(sid, outcome, poly)
    }
    pub fn get_prediction(&self, sid: i32) -> (String, f64, bool) {
        let (p, e, a) = self.engine.get_csv_fields(sid);
        (p, e, a > 0)
    }
    pub fn accuracy(&self) -> f64 { self.engine.accuracy() }
    pub fn cumulative_pnl(&self) -> f64 { self.engine.cumulative_pnl() }
    pub fn export_wisdom3(&self) -> String { self.engine.export_json() }
}
