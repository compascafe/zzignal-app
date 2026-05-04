//! RL Feedback Loop — Reinforcement Learning from Historical Accuracy
//!
//! Layer 4: Learning. Reads accuracy metrics from past sessions and
//! adjusts parameters in Layer 2 (CP width) and Layer 3 (confirmation ticks).
//!
//! Feedback modes:
//!   - Cold start: default parameters
//!   - Warm: last 10 sessions accuracy → adjust CP uncertainty range
//!   - Hot: last session trade-by-trade → adjust per-strategy confirm_ticks

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

// ─── RL Feedback ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RlConfig {
    pub enabled:                 bool,
    pub cp_widening_factor:      f64,  // multiplies CP range when accuracy < 50%
    pub cp_narrowing_factor:     f64,  // divides CP range when accuracy > 70%
    pub confirm_ticks_min:       u32,
    pub confirm_ticks_max:       u32,
    pub accuracy_threshold_low:  f64,  // widen below this
    pub accuracy_threshold_high: f64,  // narrow above this
}

impl Default for RlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cp_widening_factor: 1.5,
            cp_narrowing_factor: 0.8,
            confirm_ticks_min: 1,
            confirm_ticks_max: 20,
            accuracy_threshold_low: 0.40,
            accuracy_threshold_high: 0.65,
        }
    }
}

/// Tracks trade-by-trade results and adjusts strategy parameters.
pub struct RlFeedback {
    pub config:           RlConfig,
    /// Last N trade outcomes (true = win, false = loss) per strategy.
    pub trade_history:    Mutex<VecDeque<bool>>,
    /// Cumulative stats.
    pub total_trades:     Mutex<u64>,
    pub total_wins:       Mutex<u64>,
}

impl RlFeedback {
    pub fn new() -> Self {
        Self {
            config: RlConfig::default(),
            trade_history: Mutex::new(VecDeque::with_capacity(100)),
            total_trades:  Mutex::new(0),
            total_wins:    Mutex::new(0),
        }
    }

    pub fn with_config(config: RlConfig) -> Self {
        Self {
            config,
            trade_history: Mutex::new(VecDeque::with_capacity(100)),
            total_trades:  Mutex::new(0),
            total_wins:    Mutex::new(0),
        }
    }

    /// Record a trade outcome. Returns the adjusted parameters.
    pub fn record_trade(&self, correct: bool) -> RlAdjustment {
        let mut hist = self.trade_history.lock().unwrap();
        if hist.len() >= 100 { hist.pop_front(); }
        hist.push_back(correct);

        let mut trades = self.total_trades.lock().unwrap();
        let mut wins = self.total_wins.lock().unwrap();
        *trades += 1;
        if correct { *wins += 1; }

        self.compute_adjustment(&hist)
    }

    /// Compute parameter adjustment based on recent accuracy.
    fn compute_adjustment(&self, history: &VecDeque<bool>) -> RlAdjustment {
        if history.len() < 5 || !self.config.enabled {
            return RlAdjustment::default();
        }

        let recent: Vec<&bool> = history.iter().rev().take(10).collect();
        let accuracy = recent.iter().filter(|w| ***w).count() as f64 / recent.len() as f64;

        let mut adj = RlAdjustment::default();

        if accuracy < self.config.accuracy_threshold_low {
            adj.widen_cp = true;
            adj.cp_multiplier = self.config.cp_widening_factor;
            adj.increase_confirm = true;
            adj.confirm_delta = 2;
        } else if accuracy > self.config.accuracy_threshold_high {
            adj.narrow_cp = true;
            adj.cp_multiplier = self.config.cp_narrowing_factor;
            adj.decrease_confirm = true;
            adj.confirm_delta = 1;
        }

        adj.recent_accuracy = accuracy;
        adj.sample_size = recent.len() as u32;
        adj
    }

    /// Get current accuracy.
    pub fn accuracy(&self) -> f64 {
        let trades = *self.total_trades.lock().unwrap();
        let wins = *self.total_wins.lock().unwrap();
        if trades > 0 { wins as f64 / trades as f64 } else { 0.5 }
    }

    /// Reset all statistics.
    pub fn reset(&self) {
        self.trade_history.lock().unwrap().clear();
        *self.total_trades.lock().unwrap() = 0;
        *self.total_wins.lock().unwrap() = 0;
    }
}

impl Default for RlFeedback {
    fn default() -> Self { Self::new() }
}

// ─── RL Adjustment ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RlAdjustment {
    pub widen_cp:         bool,
    pub narrow_cp:        bool,
    pub cp_multiplier:    f64,
    pub increase_confirm: bool,
    pub decrease_confirm: bool,
    pub confirm_delta:    u32,
    pub recent_accuracy:  f64,
    pub sample_size:      u32,
}

impl Default for RlAdjustment {
    fn default() -> Self {
        Self {
            widen_cp: false, narrow_cp: false, cp_multiplier: 1.0,
            increase_confirm: false, decrease_confirm: false, confirm_delta: 0,
            recent_accuracy: 0.5, sample_size: 0,
        }
    }
}
