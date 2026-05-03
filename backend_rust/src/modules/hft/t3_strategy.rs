//! T-3 Aggressive Strategy — Wisdom v3
//!
//! Hypothesis: At 3 minutes remaining, if poly_mid ≥ 0.90 and no volatility
//! spikes, the outcome is virtually certain. Enter aggressively, exit at 0.95.
//!
//! Key differences from T-5:
//!   - Entry at T-180s (3 min before close) vs T-300s
//!   - Stricter threshold: 0.90 (UP) / 0.10 (DOWN) vs 0.85/0.15
//!   - Tighter volatility check: 0.02 swing in last 60s vs 0.03 in 120s
//!   - Faster reversal detection
//!   - Feeds wisdom3_state.json independently for A/B comparison

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::info;

// ─── T-3 Constants ─────────────────────────────────────────────────────────

const T3_SECONDS_BEFORE_END: i64 = 180;  // 3 minutes
const T3_VOLATILITY_WINDOW: i64 = 60;    // 1 minute before T-3
const T3_MIN_CONFIDENCE_UP: f64 = 0.90;
const T3_MIN_CONFIDENCE_DOWN: f64 = 0.10;
const T3_MAX_REVERSAL: f64 = 0.02;       // entry gate: max swing in window
const T3_TAKE_PROFIT_UP: f64 = 0.95;
const T3_TAKE_PROFIT_DOWN: f64 = 0.05;
const T3_REVERSAL_STOP: f64 = 0.015;     // tighter stop (0.90 → reversal past 0.885 is bad)

// ─── Trade State (same shape as T5) ────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum T3Direction { Up, Down }

#[derive(Debug, Clone, Copy, PartialEq)]
enum T3Status {
    Pending, Active, TakeProfit, StoppedOut, Settled, Filtered, NoSignal,
}

#[derive(Debug, Clone)]
struct T3Trade {
    direction:     T3Direction,
    entry_price:   f64,
    entry_time:    DateTime<Utc>,
    exit_price:    f64,
    exit_time:     Option<DateTime<Utc>>,
    exit_reason:   T3Status,
    virtual_pnl:   f64,
    peak_favorable: f64,
    peak_adverse:   f64,
    reversal_count: u32,
    time_at_extreme_ms: i64,
    spread_entry:  f64,
    spread_exit:   f64,
    reversal_speed: f64,
    entry_quality_score: f64,
    last_price_check: DateTime<Utc>,
}

impl T3Trade {
    fn new_up(price: f64, spread: f64, now: DateTime<Utc>) -> Self {
        Self {
            direction: T3Direction::Up, entry_price: price, entry_time: now,
            exit_price: 0.0, exit_time: None, exit_reason: T3Status::Active,
            virtual_pnl: 0.0, peak_favorable: price, peak_adverse: price,
            reversal_count: 0, time_at_extreme_ms: 0,
            spread_entry: spread, spread_exit: 0.0,
            reversal_speed: 0.0, entry_quality_score: 0.0,
            last_price_check: now,
        }
    }
    fn new_down(price: f64, spread: f64, now: DateTime<Utc>) -> Self {
        Self {
            direction: T3Direction::Down, entry_price: price, entry_time: now,
            exit_price: 0.0, exit_time: None, exit_reason: T3Status::Active,
            virtual_pnl: 0.0, peak_favorable: price, peak_adverse: price,
            reversal_count: 0, time_at_extreme_ms: 0,
            spread_entry: spread, spread_exit: 0.0,
            reversal_speed: 0.0, entry_quality_score: 0.0,
            last_price_check: now,
        }
    }
}

#[derive(Debug, Clone)]
pub struct T3Snapshot {
    pub session_id:      i32,
    pub scheduled_end:   DateTime<Utc>,
    pub captured:        bool,
    pub poly_mid:        f64,
    pub poly_bid:        f64,
    pub poly_ask:        f64,
    pub volatility_1min: f64,
    pub prediction:      String,
    pub entry_price:     f64,
    pub exit_price:      f64,
    pub actual_outcome:  String,
    pub correct:         bool,
    pub virtual_pnl:     f64,
    pub resolved:        bool,
    pub trade:           Option<T3Trade>,
    pub trade_active:    bool,
    pub exit_reason:     String,
}

impl T3Snapshot {
    fn new(session_id: i32, scheduled_end: DateTime<Utc>) -> Self {
        Self {
            session_id, scheduled_end,
            captured: false, poly_mid: 0.0, poly_bid: 0.0, poly_ask: 0.0,
            volatility_1min: 0.0, prediction: String::new(),
            entry_price: 0.0, exit_price: 0.0, actual_outcome: String::new(),
            correct: false, virtual_pnl: 0.0, resolved: false,
            trade: None, trade_active: false, exit_reason: String::new(),
        }
    }
}

// ─── Wisdom3 State ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Wisdom3State {
    pub version:             String,
    pub exported_at:         String,
    pub total_sessions:      u64,
    pub predictions_made:    u64,
    pub predictions_up:      u64,
    pub predictions_down:    u64,
    pub correct_predictions: u64,
    pub accuracy:            f64,
    pub cumulative_pnl:      f64,
    pub avg_pnl_per_trade:   f64,
    pub filtered_by_volatility: u64,
    pub trades_entered:      u64,
    pub trades_tp:           u64,
    pub trades_stopped:      u64,
    pub trades_settled:      u64,
    pub best_trade_pnl:      f64,
    pub worst_trade_pnl:     f64,
    pub last_10_results:     Vec<bool>,
}

impl Default for Wisdom3State {
    fn default() -> Self {
        Self {
            version: "3.0".into(), exported_at: String::new(),
            total_sessions: 0, predictions_made: 0, predictions_up: 0, predictions_down: 0,
            correct_predictions: 0, accuracy: 0.0, cumulative_pnl: 0.0, avg_pnl_per_trade: 0.0,
            filtered_by_volatility: 0, trades_entered: 0, trades_tp: 0, trades_stopped: 0,
            trades_settled: 0, best_trade_pnl: 0.0, worst_trade_pnl: 0.0,
            last_10_results: Vec::with_capacity(10),
        }
    }
}

// ─── T3 Manager ────────────────────────────────────────────────────────────

pub struct T3Manager {
    snapshots: Mutex<HashMap<i32, T3Snapshot>>,
    wisdom:    Mutex<Wisdom3State>,
}

impl T3Manager {
    pub fn new() -> Self {
        Self { snapshots: Mutex::new(HashMap::new()), wisdom: Mutex::new(Wisdom3State::default()) }
    }

    pub fn on_session_start(&self, session_id: i32, scheduled_end: DateTime<Utc>) {
        self.snapshots.lock().unwrap().insert(session_id, T3Snapshot::new(session_id, scheduled_end));
    }

    pub fn on_tick(
        &self, now: DateTime<Utc>, session_id: i32,
        poly_mid: f64, poly_bid: f64, poly_ask: f64,
    ) -> Option<T3Snapshot> {
        let mut snapshots = self.snapshots.lock().unwrap();
        let snap = match snapshots.get_mut(&session_id) {
            Some(s) => s, None => return None,
        };

        // Monitor active trade
        if snap.trade_active {
            let mut trade = snap.trade.clone().unwrap();
            let (tp, stopped) = t3_check_exit(&trade, poly_mid, poly_bid, poly_ask);
            if tp || stopped {
                t3_close(&mut trade, snap, poly_mid, poly_bid, poly_ask, now);
                snap.trade = Some(trade.clone());
                let ct = trade.clone();
                let cs = snap.clone();
                drop(snapshots);
                let mut w = self.wisdom.lock().unwrap();
                t3_update_wisdom(&mut w, &ct, &cs);
                return Some(cs);
            }
            t3_update_peaks(&mut trade, poly_mid, now);
            snap.trade = Some(trade);
            return None;
        }

        // T-3 entry gate
        if snap.captured { return None; }
        let t3_time = snap.scheduled_end - chrono::Duration::seconds(T3_SECONDS_BEFORE_END);
        if now < t3_time { return None; }

        snap.captured = true;
        snap.poly_mid = poly_mid;
        snap.poly_bid = poly_bid;
        snap.poly_ask = poly_ask;

        if snap.volatility_1min > T3_MAX_REVERSAL {
            snap.exit_reason = "FILTERED".into();
            let mut w = self.wisdom.lock().unwrap();
            w.filtered_by_volatility += 1;
            info!("[T3] Session #{} T-3: FILTERED by volatility", session_id);
            return Some(snap.clone());
        }

        let spread = if poly_ask > 0.0 && poly_bid > 0.0 { poly_ask - poly_bid } else { 0.0 };
        if poly_mid >= T3_MIN_CONFIDENCE_UP && poly_ask > 0.0 {
            snap.prediction = "UP".into();
            snap.entry_price = poly_ask;
            snap.trade = Some(T3Trade::new_up(poly_ask, spread, now));
            snap.trade_active = true;
            snap.exit_reason = "ACTIVE".into();
            info!("[T3] Session #{}: ENTER UP buy@{:.4} target@{}", session_id, poly_ask, T3_TAKE_PROFIT_UP);
        } else if poly_mid <= T3_MIN_CONFIDENCE_DOWN && poly_bid > 0.0 {
            snap.prediction = "DOWN".into();
            snap.entry_price = poly_bid;
            snap.trade = Some(T3Trade::new_down(poly_bid, spread, now));
            snap.trade_active = true;
            snap.exit_reason = "ACTIVE".into();
            info!("[T3] Session #{}: ENTER DOWN sell@{:.4} target@{}", session_id, poly_bid, T3_TAKE_PROFIT_DOWN);
        }
        Some(snap.clone())
    }

    pub fn track_volatility(&self, session_id: i32, poly_mid: f64) {
        let mut snapshots = self.snapshots.lock().unwrap();
        let snap = match snapshots.get_mut(&session_id) {
            Some(s) => s, None => return,
        };
        if snap.captured || snap.trade_active { return; }
        let window_start = snap.scheduled_end
            - chrono::Duration::seconds(T3_SECONDS_BEFORE_END)
            - chrono::Duration::seconds(T3_VOLATILITY_WINDOW);
        let now = Utc::now();
        if now >= window_start && snap.poly_mid > 0.0 {
            let dev = (poly_mid - snap.poly_mid).abs();
            if dev > snap.volatility_1min { snap.volatility_1min = dev; }
        }
        if snap.poly_mid == 0.0 { snap.poly_mid = poly_mid; }
    }

    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str, final_poly: f64) -> Option<T3Snapshot> {
        // Work on a clone to avoid borrow conflicts
        let (mut cs, resolved_trade) = {
            let mut snapshots = self.snapshots.lock().unwrap();
            let snap = match snapshots.get_mut(&session_id) {
                Some(s) => s, None => return None,
            };
            snap.exit_price = final_poly;
            snap.actual_outcome = actual_outcome.to_string();
            snap.resolved = true;

            let resolved = if snap.trade_active {
                let mut trade = snap.trade.clone().unwrap();
                if trade.exit_reason == T3Status::Active {
                    let predicted_up = snap.prediction == "UP";
                    trade.exit_price = final_poly;
                    trade.exit_reason = T3Status::Settled;
                    trade.virtual_pnl = if actual_outcome.eq_ignore_ascii_case("up") {
                        if predicted_up { 1.0 - snap.entry_price } else { -(1.0 - snap.entry_price) }
                    } else if actual_outcome.eq_ignore_ascii_case("down") {
                        if !predicted_up { snap.entry_price - 0.0 } else { -snap.entry_price }
                    } else {
                        if predicted_up { final_poly - snap.entry_price } else { snap.entry_price - final_poly }
                    };
                    snap.virtual_pnl = trade.virtual_pnl;
                    snap.exit_reason = "SETTLED".into();
                    snap.trade_active = false;
                }
                snap.trade = Some(trade.clone());
                Some(trade)
            } else { None };
            (snap.clone(), resolved)
        };

        // Update wisdom (outside snap lock)
        if let Some(ref trade) = resolved_trade {
            let mut w = self.wisdom.lock().unwrap();
            t3_update_wisdom(&mut w, trade, &cs);
        }

        // Prediction accuracy
        if !cs.prediction.is_empty() {
            let mut w = self.wisdom.lock().unwrap();
            let predicted_up = cs.prediction == "UP";
            let actual_up = actual_outcome.eq_ignore_ascii_case("up");
            cs.correct = predicted_up == actual_up && !actual_outcome.eq_ignore_ascii_case("tie");
            w.total_sessions += 1;
            w.predictions_made += 1;
            if predicted_up { w.predictions_up += 1; } else { w.predictions_down += 1; }
            if cs.correct { w.correct_predictions += 1; }
            w.accuracy = w.correct_predictions as f64 / w.predictions_made as f64;
            w.last_10_results.push(cs.correct);
            if w.last_10_results.len() > 10 { w.last_10_results.remove(0); }
        } else {
            self.wisdom.lock().unwrap().total_sessions += 1;
        }

        info!("[T3] Session #{} RESOLVED: pred={} actual={} correct={} exit={} pnl={:.4}",
            session_id, cs.prediction, actual_outcome, cs.correct, cs.exit_reason, cs.virtual_pnl);
        Some(cs)
    }

    pub fn accuracy(&self) -> f64 { self.wisdom.lock().unwrap().accuracy }
    pub fn cumulative_pnl(&self) -> f64 { self.wisdom.lock().unwrap().cumulative_pnl }

    pub fn export_wisdom3(&self) -> String {
        let mut w = self.wisdom.lock().unwrap().clone();
        w.exported_at = Utc::now().to_rfc3339();
        serde_json::to_string_pretty(&w).unwrap_or_default()
    }

    pub fn get_prediction(&self, session_id: i32) -> (String, f64, bool) {
        let snap = self.snapshots.lock().unwrap();
        if let Some(s) = snap.get(&session_id) {
            (s.prediction.clone(), s.entry_price, s.trade_active)
        } else { (String::new(), 0.0, false) }
    }
}

// ─── T3 Helpers ────────────────────────────────────────────────────────────

fn t3_check_exit(trade: &T3Trade, mid: f64, bid: f64, ask: f64) -> (bool, bool) {
    match trade.direction {
        T3Direction::Up => {
            let tp = bid >= T3_TAKE_PROFIT_UP;
            let reversal = trade.peak_favorable - mid;
            let stopped = reversal > T3_REVERSAL_STOP && mid < trade.entry_price;
            (tp, stopped)
        }
        T3Direction::Down => {
            let tp = ask <= T3_TAKE_PROFIT_DOWN && ask > 0.0;
            let reversal = mid - trade.peak_favorable;
            let stopped = reversal > T3_REVERSAL_STOP && mid > trade.entry_price;
            (tp, stopped)
        }
    }
}

fn t3_update_peaks(trade: &mut T3Trade, mid: f64, now: DateTime<Utc>) {
    match trade.direction {
        T3Direction::Up => {
            if mid > trade.peak_favorable { trade.peak_favorable = mid; }
            if mid < trade.peak_adverse { trade.peak_adverse = mid; }
            if mid >= T3_MIN_CONFIDENCE_UP { trade.time_at_extreme_ms += (now - trade.last_price_check).num_milliseconds(); }
        }
        T3Direction::Down => {
            if mid < trade.peak_favorable { trade.peak_favorable = mid; }
            if mid > trade.peak_adverse { trade.peak_adverse = mid; }
            if mid <= T3_MIN_CONFIDENCE_DOWN { trade.time_at_extreme_ms += (now - trade.last_price_check).num_milliseconds(); }
        }
    }
    trade.last_price_check = now;
}

fn t3_close(trade: &mut T3Trade, snap: &mut T3Snapshot, mid: f64, bid: f64, ask: f64, now: DateTime<Utc>) {
    let (tp, stopped) = t3_check_exit(trade, mid, bid, ask);
    let exit_price = match trade.direction {
        T3Direction::Up if tp => bid,
        T3Direction::Up => bid.max(0.0),
        T3Direction::Down if tp => ask,
        T3Direction::Down => ask.min(1.0),
    };
    trade.exit_price = exit_price;
    trade.exit_time = Some(now);

    if tp {
        trade.exit_reason = T3Status::TakeProfit;
        trade.virtual_pnl = match trade.direction {
            T3Direction::Up => exit_price - trade.entry_price,
            T3Direction::Down => trade.entry_price - exit_price,
        };
        snap.exit_reason = "TAKE_PROFIT".into();
    } else {
        trade.exit_reason = T3Status::StoppedOut;
        trade.virtual_pnl = match trade.direction {
            T3Direction::Up => exit_price - trade.entry_price,
            T3Direction::Down => trade.entry_price - exit_price,
        };
        snap.exit_reason = "STOPPED_OUT".into();
    }
    snap.virtual_pnl = trade.virtual_pnl;
    snap.exit_price = exit_price;
    snap.trade_active = false;
}

fn t3_settle(trade: &mut T3Trade, snap: &mut T3Snapshot, actual_outcome: &str, final_poly: f64) {
    trade.exit_price = final_poly;
    trade.exit_time = Some(Utc::now());
    trade.exit_reason = T3Status::Settled;
    let predicted_up = snap.prediction == "UP";
    trade.virtual_pnl = if actual_outcome.eq_ignore_ascii_case("up") {
        if predicted_up { 1.0 - snap.entry_price } else { -(1.0 - snap.entry_price) }
    } else if actual_outcome.eq_ignore_ascii_case("down") {
        if !predicted_up { snap.entry_price - 0.0 } else { -snap.entry_price }
    } else {
        if predicted_up { final_poly - snap.entry_price } else { snap.entry_price - final_poly }
    };
    snap.virtual_pnl = trade.virtual_pnl;
    snap.exit_reason = "SETTLED".into();
    snap.trade_active = false;
}

fn t3_update_wisdom(w: &mut Wisdom3State, trade: &T3Trade, snap: &T3Snapshot) {
    w.trades_entered += 1;
    match trade.exit_reason {
        T3Status::TakeProfit => w.trades_tp += 1,
        T3Status::StoppedOut => w.trades_stopped += 1,
        T3Status::Settled => w.trades_settled += 1,
        _ => {}
    }
    w.cumulative_pnl += trade.virtual_pnl;
    if w.trades_entered == 1 || trade.virtual_pnl > w.best_trade_pnl { w.best_trade_pnl = trade.virtual_pnl; }
    if w.trades_entered == 1 || trade.virtual_pnl < w.worst_trade_pnl { w.worst_trade_pnl = trade.virtual_pnl; }
    if w.trades_entered > 0 { w.avg_pnl_per_trade = w.cumulative_pnl / w.trades_entered as f64; }
}
