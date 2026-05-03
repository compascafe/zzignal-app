//! Hydra No Return — PNR Analysis Strategy (Wisdom v4)
//!
//! Tracks which price levels and remaining times yield profitable entries
//! in the last 5 minutes of a session. Feeds into RL for threshold optimization.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use tracing::info;

// ─── PNR Bucket: accumulator for a price+time bin ─────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct PnrBucket {
    pub count:       u64,
    pub correct_up:  u64,
    pub correct_down: u64,
    pub total_return_up: f64,
    pub total_return_down: f64,
}

// ─── Wisdom4 State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Wisdom4State {
    pub version:          String,
    pub strategy_name:    String,
    pub exported_at:      String,
    pub total_sessions:   u64,
    pub pnr_points:       u64,
    pub optimal_up:       Option<f64>,
    pub optimal_up_sec:   Option<i32>,
    pub optimal_down:     Option<f64>,
    pub optimal_down_sec: Option<i32>,
    pub rl_min_up:        f64,
    pub rl_min_down:      f64,
    pub rl_min_seconds:   i32,
}

impl Default for Wisdom4State {
    fn default() -> Self {
        Self {
            version: "4.0".into(), strategy_name: "Hydra No Return".into(),
            exported_at: String::new(), total_sessions: 0, pnr_points: 0,
            optimal_up: None, optimal_up_sec: None,
            optimal_down: None, optimal_down_sec: None,
            rl_min_up: 0.85, rl_min_down: 0.15, rl_min_seconds: 60,
        }
    }
}

// ─── PNR Manager ───────────────────────────────────────────────────────────

pub struct PnrManager {
    wisdom: Mutex<Wisdom4State>,
    buckets: Mutex<HashMap<(i32, i32), PnrBucket>>,
    pending_ticks: Mutex<HashMap<i32, Vec<(i32, f64, f64, f64)>>>, // session_id → [(sec, price, ret_up, ret_down)]
}

impl PnrManager {
    pub fn new() -> Self {
        Self {
            wisdom: Mutex::new(Wisdom4State::default()),
            buckets: Mutex::new(HashMap::new()),
            pending_ticks: Mutex::new(HashMap::new()),
        }
    }

    /// Accumulate one PNR tick during session. Flushed at session close.
    pub fn accumulate_tick(
        &self,
        session_id: i32,
        seconds_left: i32,
        price: f64,
        return_up: f64,
        return_down: f64,
    ) {
        let mut pending = self.pending_ticks.lock().unwrap();
        pending.entry(session_id).or_insert_with(Vec::new)
            .push((seconds_left, price, return_up, return_down));
    }

    /// Flush all accumulated ticks for a session into the bucket system.
    pub fn flush_session(&self, session_id: i32, actual_outcome: &str) {
        let mut pending = self.pending_ticks.lock().unwrap();
        if let Some(ticks) = pending.remove(&session_id) {
            for (sec, price, ret_up, ret_down) in ticks {
                self.feed_pnr(sec, price, ret_up, ret_down, actual_outcome);
            }
        }
        drop(pending);
        self.on_session_close(session_id);
    }

    /// Feed one PNR data point into buckets.
    fn feed_pnr(
        &self,
        seconds_left: i32,
        price: f64,
        return_up: f64,
        return_down: f64,
        actual_outcome: &str,
    ) {
        let price_bucket = (price * 100.0) as i32; // 0.9234 → 92
        let time_bucket = (seconds_left / 15) * 15; // round to 15s
        let mut buckets = self.buckets.lock().unwrap();
        let entry = buckets.entry((price_bucket, time_bucket)).or_default();
        entry.count += 1;
        entry.total_return_up += return_up;
        entry.total_return_down += return_down;

        let up = actual_outcome.eq_ignore_ascii_case("up");
        let down = actual_outcome.eq_ignore_ascii_case("down");
        if up { entry.correct_up += 1; }
        if down { entry.correct_down += 1; }

        let mut w = self.wisdom.lock().unwrap();
        w.pnr_points += 1;
    }

    /// Called at session close. Analyzes PNR data and updates optimal thresholds.
    pub fn on_session_close(&self, _session_id: i32) {
        let mut w = self.wisdom.lock().unwrap();
        w.total_sessions += 1;

        let buckets = self.buckets.lock().unwrap();
        if buckets.is_empty() { return; }

        // Find best UP bucket: highest avg return with count >= 3
        let mut best_up_price = None;
        let mut best_up_sec = None;
        let mut best_up_ret = 0.0;
        let mut best_down_price = None;
        let mut best_down_sec = None;
        let mut best_down_ret = 0.0;

        for ((price_i, sec), b) in buckets.iter() {
            let price = *price_i as f64 / 100.0;
            if b.count >= 3 {
                let avg_up = if b.correct_up > 0 { b.total_return_up / b.count as f64 } else { 0.0 };
                let avg_down = if b.correct_down > 0 { b.total_return_down / b.count as f64 } else { 0.0 };
                if avg_up > best_up_ret && price > 0.50 {
                    best_up_ret = avg_up;
                    best_up_price = Some(price.max(0.85));
                    best_up_sec = Some(*sec);
                }
                if avg_down > best_down_ret && price < 0.50 {
                    best_down_ret = avg_down;
                    best_down_price = Some(price.min(0.15));
                    best_down_sec = Some(*sec);
                }
            }
        }

        if let Some(p) = best_up_price {
            w.optimal_up = Some(p);
            w.optimal_up_sec = best_up_sec;
            // RL: adjust threshold toward optimal
            w.rl_min_up = (w.rl_min_up * 0.8 + p * 0.2).clamp(0.80, 0.95);
        }
        if let Some(p) = best_down_price {
            w.optimal_down = Some(p);
            w.optimal_down_sec = best_down_sec;
            w.rl_min_down = (w.rl_min_down * 0.8 + p * 0.2).clamp(0.05, 0.20);
        }

        info!("[Hydra No Return] optimal UP: {:?}sec @ {:?} | optimal DOWN: {:?}sec @ {:?} | RL thresholds: UP≥{:.2} DOWN≤{:.2}",
            w.optimal_up_sec, w.optimal_up, w.optimal_down_sec, w.optimal_down, w.rl_min_up, w.rl_min_down);
    }

    pub fn accuracy_up(&self, price: f64, seconds: i32) -> f64 {
        let pb = (price * 100.0) as i32;
        let tb = (seconds / 15) * 15;
        let buckets = self.buckets.lock().unwrap();
        if let Some(b) = buckets.get(&(pb, tb)) {
            if b.count > 0 { b.correct_up as f64 / b.count as f64 } else { 0.0 }
        } else { 0.0 }
    }

    pub fn export_json(&self) -> String {
        let mut w = self.wisdom.lock().unwrap().clone();
        w.exported_at = Utc::now().to_rfc3339();
        serde_json::to_string_pretty(&w).unwrap_or_default()
    }
}
