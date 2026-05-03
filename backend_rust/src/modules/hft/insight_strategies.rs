//! Insight Strategies — Range-based observation (Cerbero & Fenix families)
//!
//! These strategies do NOT execute trades. They OBSERVE what happens
//! when poly_mid enters a specific price range at any point during the
//! 15-minute session, then compare against the final outcome.
//!
//! Goal: discover which price ranges are most predictive of the outcome.
//!
//! Cerbero family (UP-biased ranges):
//!   70-80: poly_mid ∈ [0.70, 0.80]
//!   80-90: poly_mid ∈ [0.80, 0.90]
//!   90-98: poly_mid ∈ [0.90, 0.98]
//! Fenix family (neutral/DOWN-biased ranges):
//!   35-65: poly_mid ∈ [0.35, 0.65]
//!   30-50: poly_mid ∈ [0.30, 0.50]
//!   45-55: poly_mid ∈ [0.45, 0.55]

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::Utc;
use serde::Serialize;
use tracing::info;

// ─── Insight Definition ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InsightDef {
    pub name:     &'static str,  // "Cerbero 70-80"
    pub code:     &'static str,  // "cerbero70"
    pub min:      f64,           // 0.70
    pub max:      f64,           // 0.80
    pub direction: &'static str, // "UP", "DOWN", "ANY"
}

// ─── All 6 strategies ──────────────────────────────────────────────────────

pub static INSIGHT_DEFS: &[InsightDef] = &[
    // ─── Cerbero family ────────────────────────────────────────────────────
    InsightDef { name: "Cerbero 70-80", code: "cerbero70", min: 0.70, max: 0.80, direction: "UP" },
    InsightDef { name: "Cerbero 80-90", code: "cerbero80", min: 0.80, max: 0.90, direction: "UP" },
    InsightDef { name: "Cerbero 90-98", code: "cerbero90", min: 0.90, max: 0.98, direction: "UP" },
    // ─── Fenix family ──────────────────────────────────────────────────────
    InsightDef { name: "Fenix 35-65",   code: "fenix35",   min: 0.35, max: 0.65, direction: "ANY" },
    InsightDef { name: "Fenix 30-50",   code: "fenix30",   min: 0.30, max: 0.50, direction: "DOWN" },
    InsightDef { name: "Fenix 45-55",   code: "fenix45",   min: 0.45, max: 0.55, direction: "ANY" },
];

// ─── Per-Session Accumulator ───────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct SessionInsight {
    /// Ticks where price was in range: (direction_was_up, price)
    ticks: Vec<(bool, f64)>,
    /// Final verdict after session close
    matched: Option<bool>,
}

// ─── Cumulative Stats ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct InsightStats {
    pub name:          String,
    pub code:          String,
    pub range_min:     f64,
    pub range_max:     f64,
    pub sessions:      u64,
    pub ticks_in_range: u64,
    pub predicted_up:   u64,
    pub predicted_down: u64,
    pub correct:       u64,
    pub accuracy:      f64,
    pub last_10:       Vec<bool>,
}

impl InsightStats {
    fn new(def: &InsightDef) -> Self {
        Self {
            name: def.name.into(), code: def.code.into(),
            range_min: def.min, range_max: def.max,
            sessions: 0, ticks_in_range: 0, predicted_up: 0, predicted_down: 0,
            correct: 0, accuracy: 0.0, last_10: Vec::with_capacity(10),
        }
    }
}

// ─── Insight Manager ───────────────────────────────────────────────────────

pub struct InsightManager {
    /// Per-session accumulators: session_id → per-strategy data
    sessions: Mutex<HashMap<i32, Vec<SessionInsight>>>,
    /// Cumulative stats per strategy
    stats:    Mutex<Vec<InsightStats>>,
}

impl InsightManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            stats:    Mutex::new(INSIGHT_DEFS.iter().map(InsightStats::new).collect()),
        }
    }

    /// For each insight strategy, check if poly_mid is in range and record.
    /// Returns a vec of (code, active, direction_up) for CSV population.
    pub fn on_tick(&self, session_id: i32, poly_mid: f64) -> Vec<(String, u8, u8)> {
        let mut sessions = self.sessions.lock().unwrap();
        let acc = sessions.entry(session_id).or_insert_with(|| {
            INSIGHT_DEFS.iter().map(|_| SessionInsight::default()).collect()
        });

        let mut results = Vec::with_capacity(INSIGHT_DEFS.len());
        for (i, def) in INSIGHT_DEFS.iter().enumerate() {
            if poly_mid >= def.min && poly_mid <= def.max {
                let dir_up = poly_mid > 0.5;
                acc[i].ticks.push((dir_up, poly_mid));
                results.push((def.code.to_string(), 1, if dir_up { 1 } else { 0 }));
            } else {
                results.push((def.code.to_string(), 0, 0));
            }
        }
        results
    }

    /// Called at session close. Evaluates each insight against actual outcome.
    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str) {
        let actual_up = actual_outcome.eq_ignore_ascii_case("up");
        let actual_down = actual_outcome.eq_ignore_ascii_case("down");

        let mut sessions = self.sessions.lock().unwrap();
        let acc = match sessions.remove(&session_id) {
            Some(a) => a,
            None => return,
        };

        let mut stats = self.stats.lock().unwrap();
        for (i, def) in INSIGHT_DEFS.iter().enumerate() {
            let insight = &acc[i];
            if insight.ticks.is_empty() { continue; }

            // Determine predicted direction from the majority of ticks
            let up_count = insight.ticks.iter().filter(|(up, _)| *up).count();
            let down_count = insight.ticks.len() - up_count;
            let predicted_up = match def.direction {
                "UP"   => true,
                "DOWN" => false,
                _      => up_count > down_count,
            };

            let correct = if actual_down || actual_up {
                predicted_up == actual_up
            } else {
                false // tie
            };

            stats[i].sessions += 1;
            stats[i].ticks_in_range += insight.ticks.len() as u64;
            if predicted_up { stats[i].predicted_up += 1; } else { stats[i].predicted_down += 1; }
            if correct { stats[i].correct += 1; }
            stats[i].accuracy = stats[i].correct as f64 / stats[i].sessions as f64;
            stats[i].last_10.push(correct);
            if stats[i].last_10.len() > 10 { stats[i].last_10.remove(0); }
        }
    }

    /// Get stats for a specific strategy by code.
    pub fn get_stats(&self, code: &str) -> Option<InsightStats> {
        let stats = self.stats.lock().unwrap();
        stats.iter().find(|s| s.code == code).cloned()
    }

    /// Export all insight stats as JSON.
    pub fn export_json(&self) -> String {
        let stats = self.stats.lock().unwrap().clone();
        serde_json::to_string_pretty(&stats).unwrap_or_default()
    }

    /// Get CSV fields for all strategies as (active, matched) pairs.
    /// matched is populated at session close; during session it's 0.
    pub fn get_csv_fields(&self, _session_id: i32) -> Vec<(String, u8)> {
        // During session, return active flags only. Matched set at close.
        // For simplicity, return (code, 0) pairs — active is set by on_tick caller.
        INSIGHT_DEFS.iter().map(|d| (d.code.to_string(), 0u8)).collect()
    }
}
