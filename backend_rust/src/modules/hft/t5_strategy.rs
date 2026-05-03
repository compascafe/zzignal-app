//! T-5 Certainty Strategy — Wisdom v2
//!
//! Hypothesis: When 5 minutes remain in a 15-min Polymarket session and
//! poly_mid > 0.85 (or < 0.15), the market has already "decided" and will
//! close in that direction unless a violent reversal occurs.
//!
//! Paper-trading simulation (DEMO — never places real orders):
//!   UP:   buy at ask at T-5, target sell at 0.95, reversal stop at mid-0.03
//!   DOWN: sell at bid at T-5, target buy at 0.05, reversal stop at mid+0.03
//!
//! Tracks per-trade PnL, reversal detection, exit reason, and cumulative stats.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::info;

// ─── T-5 Constants ─────────────────────────────────────────────────────────

const T5_SECONDS_BEFORE_END: i64 = 300;  // 5 minutes
const T5_VOLATILITY_WINDOW: i64 = 120;   // 2 minutes before T-5
const T5_MIN_CONFIDENCE_UP: f64 = 0.85;
const T5_MIN_CONFIDENCE_DOWN: f64 = 0.15;
const T5_MAX_REVERSAL: f64 = 0.03;       // max price swing before T-5 (entry gate)
const T5_TAKE_PROFIT_UP: f64 = 0.95;     // sell target for UP trades
const T5_TAKE_PROFIT_DOWN: f64 = 0.05;   // buy target for DOWN trades
const T5_REVERSAL_STOP: f64 = 0.03;      // if price reverses >3% toward 0.5, liquidate

// ─── Trade State ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum TradeDirection { Up, Down }

#[derive(Debug, Clone, Copy, PartialEq)]
enum TradeStatus {
    Pending,      // not yet entered
    Active,       // holding position
    TakeProfit,   // target price hit
    StoppedOut,   // reversal detected → closed at loss
    Settled,      // session ended with position → settled at 0/1
    Filtered,     // volatility gate blocked entry
    NoSignal,     // mid-range at T-5, no trade
}

#[derive(Debug, Clone)]
struct ActiveTrade {
    direction:     TradeDirection,
    entry_price:   f64,
    entry_time:    DateTime<Utc>,
    exit_price:    f64,
    exit_time:     Option<DateTime<Utc>>,
    exit_reason:   TradeStatus,
    virtual_pnl:   f64,
    peak_favorable: f64,
    peak_adverse:   f64,
    reversal_count: u32,
    // ─── Validation indicators ─────────────────────────────────────────────
    time_at_extreme_ms: i64,     // how long price stayed >0.90 (UP) or <0.10 (DOWN)
    btc_moved_same:     bool,    // BTC moved in same direction during trade
    spread_entry:       f64,     // poly_spread at entry
    spread_exit:        f64,     // poly_spread at exit
    volume_entry:       f64,     // poly total volume (bid+ask) at entry
    volume_exit:        f64,     // poly total volume at exit
    reversal_speed:     f64,     // (peak - exit) / duration_ms (only if stopped out)
    entry_quality_score: f64,    // composite 0-1 score
    last_price_check:   DateTime<Utc>, // last monitor tick timestamp
}

impl ActiveTrade {
    fn new_up(entry_price: f64, spread: f64, volume: f64, now: DateTime<Utc>) -> Self {
        Self {
            direction: TradeDirection::Up, entry_price, entry_time: now,
            exit_price: 0.0, exit_time: None, exit_reason: TradeStatus::Active,
            virtual_pnl: 0.0, peak_favorable: entry_price, peak_adverse: entry_price,
            reversal_count: 0,
            time_at_extreme_ms: 0, btc_moved_same: false,
            spread_entry: spread, spread_exit: 0.0,
            volume_entry: volume, volume_exit: 0.0,
            reversal_speed: 0.0, entry_quality_score: 0.0,
            last_price_check: now,
        }
    }

    fn new_down(entry_price: f64, spread: f64, volume: f64, now: DateTime<Utc>) -> Self {
        Self {
            direction: TradeDirection::Down, entry_price, entry_time: now,
            exit_price: 0.0, exit_time: None, exit_reason: TradeStatus::Active,
            virtual_pnl: 0.0, peak_favorable: entry_price, peak_adverse: entry_price,
            reversal_count: 0,
            time_at_extreme_ms: 0, btc_moved_same: false,
            spread_entry: spread, spread_exit: 0.0,
            volume_entry: volume, volume_exit: 0.0,
            reversal_speed: 0.0, entry_quality_score: 0.0,
            last_price_check: now,
        }
    }
}

// ─── Per-Session Snapshot ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct T5Snapshot {
    pub session_id:      i32,
    pub scheduled_end:   DateTime<Utc>,
    pub captured:        bool,
    pub poly_mid:        f64,
    pub poly_bid:        f64,
    pub poly_ask:        f64,
    pub btc_price:       f64,
    pub volatility_2min: f64,
    pub prediction:      String,
    pub entry_price:     f64,
    pub exit_price:      f64,
    pub actual_outcome:  String,
    pub correct:         bool,
    pub virtual_pnl:     f64,
    pub resolved:        bool,
    // ─── Active trading fields ─────────────────────────────────────────────
    pub trade:           Option<ActiveTrade>,
    pub trade_active:    bool,
    pub exit_reason:     String,  // "TAKE_PROFIT" | "STOPPED_OUT" | "SETTLED" | "FILTERED" | ""
}

impl T5Snapshot {
    fn new(session_id: i32, scheduled_end: DateTime<Utc>) -> Self {
        Self {
            session_id, scheduled_end,
            captured: false, poly_mid: 0.0, poly_bid: 0.0, poly_ask: 0.0,
            btc_price: 0.0, volatility_2min: 0.0,
            prediction: String::new(), entry_price: 0.0, exit_price: 0.0,
            actual_outcome: String::new(), correct: false, virtual_pnl: 0.0,
            resolved: false,
            trade: None, trade_active: false, exit_reason: String::new(),
        }
    }
}

// ─── Wisdom2 Cumulative State ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Wisdom2State {
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
    pub last_10_results:     Vec<bool>,
    // ─── Trading stats ─────────────────────────────────────────────────────
    pub trades_entered:      u64,
    pub trades_tp:           u64,
    pub trades_stopped:      u64,
    pub trades_settled:      u64,
    pub reversal_alerts:     u64,
    pub best_trade_pnl:      f64,
    pub worst_trade_pnl:     f64,
}

impl Default for Wisdom2State {
    fn default() -> Self {
        Self {
            version: "2.0".into(), exported_at: String::new(),
            total_sessions: 0, predictions_made: 0,
            predictions_up: 0, predictions_down: 0,
            correct_predictions: 0, accuracy: 0.0,
            cumulative_pnl: 0.0, avg_pnl_per_trade: 0.0,
            filtered_by_volatility: 0, last_10_results: Vec::with_capacity(10),
            trades_entered: 0, trades_tp: 0, trades_stopped: 0, trades_settled: 0,
            reversal_alerts: 0, best_trade_pnl: 0.0, worst_trade_pnl: 0.0,
        }
    }
}

// ─── T5 Strategy Manager ───────────────────────────────────────────────────

pub struct T5Manager {
    snapshots: Mutex<HashMap<i32, T5Snapshot>>,
    wisdom:    Mutex<Wisdom2State>,
}

impl T5Manager {
    pub fn new() -> Self {
        Self {
            snapshots: Mutex::new(HashMap::new()),
            wisdom:    Mutex::new(Wisdom2State::default()),
        }
    }

    /// Called when a new session starts.
    pub fn on_session_start(&self, session_id: i32, scheduled_end: DateTime<Utc>) {
        let mut snap = self.snapshots.lock().unwrap();
        snap.insert(session_id, T5Snapshot::new(session_id, scheduled_end));
    }

    /// Called on every tick. At T-5, captures state and opens a virtual position.
    /// Returns the snapshot if a prediction was just made.
    pub fn on_tick(
        &self,
        now: DateTime<Utc>,
        session_id: i32,
        poly_mid: f64,
        poly_bid: f64,
        poly_ask: f64,
        btc_price: f64,
    ) -> Option<T5Snapshot> {
        let mut snapshots = self.snapshots.lock().unwrap();
        let snap = match snapshots.get_mut(&session_id) {
            Some(s) => s,
            None => return None,
        };

        // ─── If already in a trade, monitor for TP / reversal ──────────────
        if snap.trade_active {
            let mut trade = snap.trade.clone().unwrap();
            let (tp_hit, stopped) = check_exit_conditions(&trade, poly_mid, poly_bid, poly_ask);
            if tp_hit || stopped {
                close_trade(&mut trade, snap, poly_mid, poly_bid, poly_ask, now);
                snap.trade = Some(trade.clone());
                let closed_trade = trade.clone();
                let closed_snap = snap.clone();
                drop(snapshots);
                let mut w = self.wisdom.lock().unwrap();
                update_wisdom_trade(&mut w, &closed_trade, &closed_snap);
                return Some(closed_snap);
            }
            update_peaks(&mut trade, poly_mid);
            snap.trade = Some(trade);
            return None;
        }

        // ─── T-5 entry gate ────────────────────────────────────────────────
        if snap.captured { return None; }
        let t5_time = snap.scheduled_end - chrono::Duration::seconds(T5_SECONDS_BEFORE_END);
        if now < t5_time { return None; }

        snap.captured = true;
        snap.poly_mid = poly_mid;
        snap.poly_bid = poly_bid;
        snap.poly_ask = poly_ask;
        snap.btc_price = btc_price;

        // Volatility filter
        if snap.volatility_2min > T5_MAX_REVERSAL {
            snap.exit_reason = "FILTERED".into();
            info!("[T5] Session #{} T-5: FILTERED (volatility {:.4} > {:.3})",
                session_id, snap.volatility_2min, T5_MAX_REVERSAL);
            return Some(snap.clone());
        }

        // ─── Enter trade ───────────────────────────────────────────────────
        let spread = if poly_ask > 0.0 && poly_bid > 0.0 { poly_ask - poly_bid } else { 0.0 };
        let volume = 0.0; // populated from book update in monitor

        if poly_mid > T5_MIN_CONFIDENCE_UP && poly_ask > 0.0 {
            snap.prediction = "UP".into();
            snap.entry_price = poly_ask;
            snap.trade = Some(ActiveTrade::new_up(poly_ask, spread, volume, now));
            snap.trade_active = true;
            snap.exit_reason = "ACTIVE".into();
            info!("[T5] Session #{} T-5: ENTER UP buy@{:.4} spread={:.4} target@{}",
                session_id, poly_ask, spread, T5_TAKE_PROFIT_UP);
        } else if poly_mid < T5_MIN_CONFIDENCE_DOWN && poly_bid > 0.0 {
            snap.prediction = "DOWN".into();
            snap.entry_price = poly_bid;
            snap.trade = Some(ActiveTrade::new_down(poly_bid, spread, volume, now));
            snap.trade_active = true;
            snap.exit_reason = "ACTIVE".into();
            info!("[T5] Session #{} T-5: ENTER DOWN sell@{:.4} spread={:.4} target@{}",
                session_id, poly_bid, spread, T5_TAKE_PROFIT_DOWN);
        } else {
            info!("[T5] Session #{} T-5: poly_mid={:.4} → no trade (mid-range)",
                session_id, poly_mid);
        }

        Some(snap.clone())
    }

    /// Called on every tick during an active T5 trade to update price monitoring.
    /// Checks: take-profit hit, reversal stop.
    pub fn monitor_active_trade(
        &self,
        session_id: i32,
        poly_mid: f64,
        poly_bid: f64,
        poly_ask: f64,
    ) {
        let now = Utc::now();
        let mut snapshots = self.snapshots.lock().unwrap();
        let snap = match snapshots.get_mut(&session_id) {
            Some(s) => s,
            None => return,
        };
        if !snap.trade_active { return; }

        let mut trade = match snap.trade.clone() {
            Some(t) => t,
            None => return,
        };

        update_peaks(&mut trade, poly_mid);

        let (tp_hit, stopped) = check_exit_conditions(&trade, poly_mid, poly_bid, poly_ask);
        if tp_hit || stopped {
            // Inline close to avoid double mutable borrow of snap
            let exit_price = match trade.direction {
                TradeDirection::Up if tp_hit => poly_bid,
                TradeDirection::Up => poly_bid.max(0.0),
                TradeDirection::Down if tp_hit => poly_ask,
                TradeDirection::Down => poly_ask.min(1.0),
            };
            trade.exit_price = exit_price;
            trade.exit_time = Some(now);
            if tp_hit {
                trade.exit_reason = TradeStatus::TakeProfit;
                trade.virtual_pnl = match trade.direction {
                    TradeDirection::Up => exit_price - trade.entry_price,
                    TradeDirection::Down => trade.entry_price - exit_price,
                };
                snap.exit_reason = "TAKE_PROFIT".into();
            } else {
                trade.exit_reason = TradeStatus::StoppedOut;
                trade.virtual_pnl = match trade.direction {
                    TradeDirection::Up => exit_price - trade.entry_price,
                    TradeDirection::Down => trade.entry_price - exit_price,
                };
                snap.exit_reason = "STOPPED_OUT".into();
            }
            snap.virtual_pnl = trade.virtual_pnl;
            snap.exit_price = exit_price;
            snap.trade_active = false;
            snap.trade = Some(trade.clone());
            let closed_trade = trade.clone();
            let closed_snap = snap.clone();
            drop(snapshots);
            let mut w = self.wisdom.lock().unwrap();
            update_wisdom_trade(&mut w, &closed_trade, &closed_snap);
            return;
        }
        snap.trade = Some(trade.clone());
    }

    /// Track price for volatility measurement (2-min window before T-5).
    pub fn track_volatility(&self, session_id: i32, poly_mid: f64) {
        let mut snapshots = self.snapshots.lock().unwrap();
        let snap = match snapshots.get_mut(&session_id) {
            Some(s) => s,
            None => return,
        };
        if snap.captured || snap.trade_active { return; }

        let window_start = snap.scheduled_end
            - chrono::Duration::seconds(T5_SECONDS_BEFORE_END)
            - chrono::Duration::seconds(T5_VOLATILITY_WINDOW);
        let now = Utc::now();
        if now >= window_start && snap.poly_mid > 0.0 {
            let deviation = (poly_mid - snap.poly_mid).abs();
            if deviation > snap.volatility_2min {
                snap.volatility_2min = deviation;
            }
        }
        if snap.poly_mid == 0.0 {
            snap.poly_mid = poly_mid;
        }
    }

    /// Called at session close. Settles any open trade at final outcome price.
    pub fn on_session_close(
        &self,
        session_id: i32,
        actual_outcome: &str,
        final_poly: f64,
    ) -> Option<T5Snapshot> {
        // Work on clone to avoid borrow conflicts
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
                if trade.exit_reason == TradeStatus::Active {
                    let predicted_up = snap.prediction == "UP";
                    trade.exit_price = final_poly;
                    trade.exit_reason = TradeStatus::Settled;
                    let actual_up = actual_outcome.eq_ignore_ascii_case("up");
                    let actual_down = actual_outcome.eq_ignore_ascii_case("down");
                    trade.virtual_pnl = if actual_up {
                        if predicted_up { 1.0 - snap.entry_price } else { -(1.0 - snap.entry_price) }
                    } else if actual_down {
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

        // Update wisdom
        if let Some(ref trade) = resolved_trade {
            let mut w = self.wisdom.lock().unwrap();
            update_wisdom_trade(&mut w, trade, &cs);
        }

        if cs.prediction.is_empty() && !cs.trade_active {
            self.wisdom.lock().unwrap().total_sessions += 1;
            return Some(cs);
        }

        let predicted_up = cs.prediction == "UP";
        let actual_up = actual_outcome.eq_ignore_ascii_case("up");
        cs.correct = !actual_outcome.eq_ignore_ascii_case("tie") && predicted_up == actual_up;

        let mut w = self.wisdom.lock().unwrap();
        w.total_sessions += 1;
        if !cs.prediction.is_empty() {
            w.predictions_made += 1;
            if predicted_up { w.predictions_up += 1; } else { w.predictions_down += 1; }
            if cs.correct { w.correct_predictions += 1; }
            w.accuracy = w.correct_predictions as f64 / w.predictions_made as f64;
        }
        w.last_10_results.push(cs.correct);
        if w.last_10_results.len() > 10 { w.last_10_results.remove(0); }

        info!("[T5] Session #{} RESOLVED: pred={} actual={} correct={} exit={} pnl={:.4} | acc={:.1}% cum_pnl={:.4}",
            session_id, cs.prediction, actual_outcome, cs.correct, cs.exit_reason,
            cs.virtual_pnl, self.accuracy(), self.cumulative_pnl());
        Some(cs)
    }

    pub fn accuracy(&self) -> f64 { self.wisdom.lock().unwrap().accuracy }
    pub fn cumulative_pnl(&self) -> f64 { self.wisdom.lock().unwrap().cumulative_pnl }

    pub fn export_wisdom2(&self) -> String {
        let mut w = self.wisdom.lock().unwrap().clone();
        w.exported_at = Utc::now().to_rfc3339();
        serde_json::to_string_pretty(&w).unwrap_or_default()
    }

    pub fn get_prediction(&self, session_id: i32) -> (String, f64, bool) {
        let snap = self.snapshots.lock().unwrap();
        if let Some(s) = snap.get(&session_id) {
            (s.prediction.clone(), s.entry_price, s.trade_active)
        } else {
            (String::new(), 0.0, false)
        }
    }
}

// ─── Trade Logic Helpers ───────────────────────────────────────────────────

/// Check if take-profit hit or reversal stop triggered.
fn check_exit_conditions(
    trade: &ActiveTrade,
    poly_mid: f64,
    poly_bid: f64,
    poly_ask: f64,
) -> (bool, bool) {
    match trade.direction {
        TradeDirection::Up => {
            // TP: can sell at >= 0.95 (bid is our exit price)
            let tp = poly_bid >= T5_TAKE_PROFIT_UP;
            // Stop: price reversed toward 0.5 by more than REVERSAL_STOP from mid at entry
            let reversal = trade.peak_favorable - poly_mid;
            let stopped = reversal > T5_REVERSAL_STOP && poly_mid < trade.entry_price;
            (tp, stopped)
        }
        TradeDirection::Down => {
            // TP: can buy at <= 0.05 (ask is our exit price)
            let tp = poly_ask <= T5_TAKE_PROFIT_DOWN && poly_ask > 0.0;
            // Stop: price reversed upward toward 0.5 by more than REVERSAL_STOP
            let reversal = poly_mid - trade.peak_favorable;
            let stopped = reversal > T5_REVERSAL_STOP && poly_mid > trade.entry_price;
            (tp, stopped)
        }
    }
}

fn update_peaks(trade: &mut ActiveTrade, poly_mid: f64) {
    match trade.direction {
        TradeDirection::Up => {
            if poly_mid > trade.peak_favorable { trade.peak_favorable = poly_mid; }
            if poly_mid < trade.peak_adverse { trade.peak_adverse = poly_mid; }
            if trade.peak_favorable - poly_mid > T5_REVERSAL_STOP * 0.7 {
                trade.reversal_count += 1;
            }
        }
        TradeDirection::Down => {
            if poly_mid < trade.peak_favorable { trade.peak_favorable = poly_mid; }
            if poly_mid > trade.peak_adverse { trade.peak_adverse = poly_mid; }
            if poly_mid - trade.peak_favorable > T5_REVERSAL_STOP * 0.7 {
                trade.reversal_count += 1;
            }
        }
    }
}

fn close_trade(
    trade: &mut ActiveTrade,
    snap: &mut T5Snapshot,
    poly_mid: f64,
    poly_bid: f64,
    poly_ask: f64,
    now: DateTime<Utc>,
) {
    let (tp, stopped) = check_exit_conditions(trade, poly_mid, poly_bid, poly_ask);
    let exit_price = match trade.direction {
        TradeDirection::Up if tp => poly_bid,
        TradeDirection::Up => poly_bid.max(0.0),
        TradeDirection::Down if tp => poly_ask,
        TradeDirection::Down => poly_ask.min(1.0),
    };
    trade.exit_price = exit_price;
    trade.exit_time = Some(now);

    if tp {
        trade.exit_reason = TradeStatus::TakeProfit;
        trade.virtual_pnl = match trade.direction {
            TradeDirection::Up => exit_price - trade.entry_price,
            TradeDirection::Down => trade.entry_price - exit_price,
        };
        snap.exit_reason = "TAKE_PROFIT".into();
        info!("[T5] Session #{} TP HIT: exit@{:.4} pnl={:.4}",
            snap.session_id, exit_price, trade.virtual_pnl);
    } else if stopped {
        trade.exit_reason = TradeStatus::StoppedOut;
        trade.virtual_pnl = match trade.direction {
            TradeDirection::Up => exit_price - trade.entry_price,
            TradeDirection::Down => trade.entry_price - exit_price,
        };
        snap.exit_reason = "STOPPED_OUT".into();
        info!("[T5] Session #{} STOPPED OUT: reversal from {:.4} to {:.4} pnl={:.4}",
            snap.session_id, trade.peak_favorable, exit_price, trade.virtual_pnl);
    }

    snap.virtual_pnl = trade.virtual_pnl;
    snap.exit_price = exit_price;
    snap.trade_active = false;
}

fn close_trade_settle(trade: &mut ActiveTrade, snap: &mut T5Snapshot, actual_outcome: &str, final_poly: f64) {
    trade.exit_price = final_poly;
    trade.exit_time = Some(Utc::now());
    trade.exit_reason = TradeStatus::Settled;

    let predicted_up = snap.prediction == "UP";
    let actual_up = actual_outcome.eq_ignore_ascii_case("up");
    let actual_down = actual_outcome.eq_ignore_ascii_case("down");

    trade.virtual_pnl = if actual_up {
        if predicted_up { 1.0 - snap.entry_price } else { -(1.0 - snap.entry_price) }
    } else if actual_down {
        if !predicted_up { snap.entry_price - 0.0 } else { -snap.entry_price }
    } else {
        // Tie — close at final_poly
        if predicted_up { final_poly - snap.entry_price } else { snap.entry_price - final_poly }
    };

    snap.virtual_pnl = trade.virtual_pnl;
    snap.exit_reason = "SETTLED".into();
    snap.trade_active = false;
    snap.exit_price = final_poly;

    info!("[T5] Session #{} SETTLED at close: outcome={} pnl={:.4}",
        snap.session_id, actual_outcome, trade.virtual_pnl);
}

fn update_wisdom_trade(w: &mut Wisdom2State, trade: &ActiveTrade, snap: &T5Snapshot) {
    w.trades_entered += 1;

    match trade.exit_reason {
        TradeStatus::TakeProfit => w.trades_tp += 1,
        TradeStatus::StoppedOut => w.trades_stopped += 1,
        TradeStatus::Settled => w.trades_settled += 1,
        _ => {}
    }

    w.reversal_alerts += trade.reversal_count as u64;
    w.cumulative_pnl += trade.virtual_pnl;

    if w.trades_entered == 1 || trade.virtual_pnl > w.best_trade_pnl {
        w.best_trade_pnl = trade.virtual_pnl;
    }
    if w.trades_entered == 1 || trade.virtual_pnl < w.worst_trade_pnl {
        w.worst_trade_pnl = trade.virtual_pnl;
    }

    if w.trades_entered > 0 {
        w.avg_pnl_per_trade = w.cumulative_pnl / w.trades_entered as f64;
    }
}
