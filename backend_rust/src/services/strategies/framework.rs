//! Strategy Framework — reusable RL-powered strategy engine.
//!
//! Each strategy defines:
//!   - Constants: name, code, time window, price thresholds, TP, stop
//!   - Entry rule: should_enter(poly_mid, volatility) -> Option<Direction>
//!   - Exit rule: should_exit(trade, poly_mid) -> Option<ExitReason>
//!
//! The framework handles:
//!   - Session registration per strategy
//!   - Entry/exit tracking with PnL
//!   - CSV fields: {code}_prediction, {code}_entry_price, {code}_active
//!   - Reinforcement Learning: accuracy → adjust thresholds
//!   - Wisdom state JSON export
//!   - Frontend panel data

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::info;

// ─── Strategy Parameters ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StrategyParams {
    pub name:               String,  // "Hydra 85"
    pub code:               String,  // "hydra85"
    pub seconds_before_end: i64,     // 300 for T-5, 180 for T-3
    pub min_up:             f64,     // poly_mid above this → UP
    pub min_down:           f64,     // poly_mid below this → DOWN
    pub take_profit_up:     f64,     // sell target for UP
    pub take_profit_down:   f64,     // buy target for DOWN
    pub reversal_stop:      f64,     // max adverse move
    pub max_volatility:     f64,     // entry filter: max swing in window
    pub volatility_window:  i64,     // seconds before entry to check
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            name: "Unnamed".into(), code: "unnamed".into(),
            seconds_before_end: 300, min_up: 0.85, min_down: 0.15,
            take_profit_up: 0.95, take_profit_down: 0.05,
            reversal_stop: 0.03, max_volatility: 0.03, volatility_window: 120,
        }
    }
}

// ─── Trade State ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction { Up, Down }

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TradeStatus {
    Pending, Active, TakeProfit, StoppedOut, Settled, Filtered, NoSignal,
}

impl TradeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING", Self::Active => "ACTIVE",
            Self::TakeProfit => "TAKE_PROFIT", Self::StoppedOut => "STOPPED_OUT",
            Self::Settled => "SETTLED", Self::Filtered => "FILTERED",
            Self::NoSignal => "NO_SIGNAL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StrategyTrade {
    pub direction:     Direction,
    pub entry_price:   f64,
    pub entry_time:    DateTime<Utc>,
    pub exit_price:    f64,
    pub exit_time:     Option<DateTime<Utc>>,
    pub exit_reason:   TradeStatus,
    pub virtual_pnl:   f64,
    pub peak_favorable: f64,
    pub peak_adverse:   f64,
}

impl StrategyTrade {
    pub fn new(dir: Direction, price: f64, now: DateTime<Utc>) -> Self {
        Self {
            direction: dir, entry_price: price, entry_time: now,
            exit_price: 0.0, exit_time: None, exit_reason: TradeStatus::Active,
            virtual_pnl: 0.0, peak_favorable: price, peak_adverse: price,
        }
    }
}

// ─── Per-Session Snapshot ──────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StrategySession {
    pub session_id:      i32,
    pub scheduled_end:   DateTime<Utc>,
    pub params:          StrategyParams,
    pub captured:        bool,
    pub poly_mid:        f64,
    pub poly_bid:        f64,
    pub poly_ask:        f64,
    pub volatility_2min: f64,
    pub prediction:      String,
    pub entry_price:     f64,
    pub exit_price:      f64,
    pub actual_outcome:  String,
    pub correct:         bool,
    pub virtual_pnl:     f64,
    pub resolved:        bool,
    pub trade:           Option<StrategyTrade>,
    pub trade_active:    bool,
    pub exit_reason:     String,
}

impl StrategySession {
    pub fn new(session_id: i32, scheduled_end: DateTime<Utc>, params: StrategyParams) -> Self {
        Self {
            session_id, scheduled_end, params,
            captured: false, poly_mid: 0.0, poly_bid: 0.0, poly_ask: 0.0,
            volatility_2min: 0.0, prediction: String::new(),
            entry_price: 0.0, exit_price: 0.0, actual_outcome: String::new(),
            correct: false, virtual_pnl: 0.0, resolved: false,
            trade: None, trade_active: false, exit_reason: String::new(),
        }
    }
}

// ─── Wisdom State (cumulative) ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct WisdomState {
    pub version:             String,
    pub strategy_name:       String,
    pub strategy_code:       String,
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
    // ─── RL-adjusted thresholds ────────────────────────────────────────────
    pub rl_min_up:           f64,
    pub rl_min_down:         f64,
    pub rl_reversal_stop:    f64,
}

impl WisdomState {
    pub fn new(name: &str, code: &str) -> Self {
        Self {
            version: "2.0".into(), strategy_name: name.into(), strategy_code: code.into(),
            exported_at: String::new(), total_sessions: 0, predictions_made: 0,
            predictions_up: 0, predictions_down: 0,
            correct_predictions: 0, accuracy: 0.0, cumulative_pnl: 0.0, avg_pnl_per_trade: 0.0,
            filtered_by_volatility: 0, trades_entered: 0, trades_tp: 0, trades_stopped: 0,
            trades_settled: 0, best_trade_pnl: 0.0, worst_trade_pnl: 0.0,
            last_10_results: Vec::with_capacity(10),
            rl_min_up: 0.85, rl_min_down: 0.15, rl_reversal_stop: 0.03,
        }
    }
}

// ─── Strategy Engine ───────────────────────────────────────────────────────

pub struct StrategyEngine {
    pub params:    StrategyParams,
    sessions:      Mutex<HashMap<i32, StrategySession>>,
    pub wisdom:    Mutex<WisdomState>,
}

impl StrategyEngine {
    pub fn new(params: StrategyParams) -> Self {
        let name = params.name.clone();
        let code = params.code.clone();
        let rl_up = params.min_up;
        let rl_down = params.min_down;
        let rl_stop = params.reversal_stop;
        Self {
            params,
            sessions: Mutex::new(HashMap::new()),
            wisdom: Mutex::new({
                let mut w = WisdomState::new(&name, &code);
                w.rl_min_up = rl_up;
                w.rl_min_down = rl_down;
                w.rl_reversal_stop = rl_stop;
                w
            }),
        }
    }

    // ─── Session lifecycle ─────────────────────────────────────────────────

    pub fn on_session_start(&self, session_id: i32, scheduled_end: DateTime<Utc>) {
        self.sessions.lock().unwrap()
            .insert(session_id, StrategySession::new(session_id, scheduled_end, self.params.clone()));
    }

    /// Returns (prediction, entry_price, trade_active) for CSV.
    pub fn get_csv_fields(&self, session_id: i32) -> (String, f64, u8) {
        let ss = self.sessions.lock().unwrap();
        if let Some(s) = ss.get(&session_id) {
            (s.prediction.clone(), s.entry_price, if s.trade_active { 1 } else { 0 })
        } else { (String::new(), 0.0, 0) }
    }

    pub fn seconds_left(&self, session_id: i32) -> i64 {
        let ss = self.sessions.lock().unwrap();
        if let Some(s) = ss.get(&session_id) {
            (s.scheduled_end - Utc::now()).num_seconds()
        } else {
            // No DB session — use Polymarket 15-min round clock (UTC)
            use chrono::Timelike;
            let now = Utc::now();
            let t = now.time();
            let secs_in_chunk = (t.minute() as i64 % 15) * 60 + t.second() as i64;
            900 - secs_in_chunk
        }
    }

    // ─── Tick processing ───────────────────────────────────────────────────

    /// Called on every tick. Returns Some(snapshot) if prediction just captured.
    pub fn on_tick(
        &self, now: DateTime<Utc>, session_id: i32,
        poly_mid: f64, poly_bid: f64, poly_ask: f64,
    ) -> Option<StrategySession> {
        let mut sessions = self.sessions.lock().unwrap();
        let s = match sessions.get_mut(&session_id) {
            Some(v) => v, None => return None,
        };

        // Monitor active trade
        if s.trade_active {
            let mut trade = s.trade.clone().unwrap();
            let (tp, stopped) = self.check_exit(&trade, poly_mid, poly_bid, poly_ask);
            if tp || stopped {
                self.close_trade(&mut trade, s, poly_mid, poly_bid, poly_ask);
                s.trade = Some(trade.clone());
                let cs = s.clone();
                let ct = trade;
                drop(sessions);
                self.accumulate(&ct, &cs);
                return Some(cs);
            }
            // Update peaks
            self.update_peaks(&mut trade, poly_mid);
            s.trade = Some(trade);
            return None;
        }

        // Entry gate
        if s.captured { return None; }
        let t_time = s.scheduled_end - chrono::Duration::seconds(self.params.seconds_before_end);
        if now < t_time { return None; }

        s.captured = true;
        s.poly_mid = poly_mid;
        s.poly_bid = poly_bid;
        s.poly_ask = poly_ask;

        // Volatility filter
        if s.volatility_2min > self.params.max_volatility {
            s.exit_reason = TradeStatus::Filtered.as_str().into();
            self.wisdom.lock().unwrap().filtered_by_volatility += 1;
            return Some(s.clone());
        }

        // Enter trade
        if poly_mid >= self.params.min_up && poly_ask > 0.0 {
            s.prediction = "UP".into();
            s.entry_price = poly_ask;
            s.trade = Some(StrategyTrade::new(Direction::Up, poly_ask, now));
            s.trade_active = true;
            s.exit_reason = TradeStatus::Active.as_str().into();
            info!("[{}] Session #{}: ENTER UP buy@{:.4} tp@{}",
                self.params.name, session_id, poly_ask, self.params.take_profit_up);
        } else if poly_mid <= self.params.min_down && poly_bid > 0.0 {
            s.prediction = "DOWN".into();
            s.entry_price = poly_bid;
            s.trade = Some(StrategyTrade::new(Direction::Down, poly_bid, now));
            s.trade_active = true;
            s.exit_reason = TradeStatus::Active.as_str().into();
            info!("[{}] Session #{}: ENTER DOWN sell@{:.4} tp@{}",
                self.params.name, session_id, poly_bid, self.params.take_profit_down);
        }
        Some(s.clone())
    }

    pub fn track_volatility(&self, session_id: i32, poly_mid: f64) {
        let mut sessions = self.sessions.lock().unwrap();
        let s = match sessions.get_mut(&session_id) {
            Some(v) => v, None => return,
        };
        if s.captured || s.trade_active { return; }
        let window_start = s.scheduled_end
            - chrono::Duration::seconds(self.params.seconds_before_end)
            - chrono::Duration::seconds(self.params.volatility_window);
        let now = Utc::now();
        if now >= window_start && s.poly_mid > 0.0 {
            let seed_side = (s.poly_mid - 0.5).signum();
            let new_side  = (poly_mid - 0.5).signum();
            if seed_side == new_side || new_side == 0.0 {
                let dev = (poly_mid - s.poly_mid).abs();
                if dev > s.volatility_2min { s.volatility_2min = dev; }
            }
        }
        if s.poly_mid == 0.0 { s.poly_mid = poly_mid; }
    }

    // ─── Session close ─────────────────────────────────────────────────────

    pub fn on_session_close(&self, session_id: i32, actual_outcome: &str, final_poly: f64) -> Option<StrategySession> {
        let (cs, resolved) = {
            let mut sessions = self.sessions.lock().unwrap();
            let s = match sessions.get_mut(&session_id) {
                Some(v) => v, None => return None,
            };
            s.exit_price = final_poly;
            s.actual_outcome = actual_outcome.to_string();
            s.resolved = true;

            let trade = if s.trade_active {
                let mut t = s.trade.clone().unwrap();
                if t.exit_reason == TradeStatus::Active {
                    let predicted_up = s.prediction == "UP";
                    t.exit_price = final_poly;
                    t.exit_reason = TradeStatus::Settled;
                    t.virtual_pnl = self.settle_pnl(predicted_up, s.entry_price, actual_outcome, final_poly);
                    s.virtual_pnl = t.virtual_pnl;
                    s.exit_reason = TradeStatus::Settled.as_str().into();
                    s.trade_active = false;
                }
                s.trade = Some(t.clone());
                Some(t)
            } else { None };
            (s.clone(), trade)
        };

        if let Some(ref trade) = resolved {
            self.accumulate(trade, &cs);
        }

        if !cs.prediction.is_empty() {
            let predicted_up = cs.prediction == "UP";
            let mut w = self.wisdom.lock().unwrap();
            w.total_sessions += 1;
            w.predictions_made += 1;
            if predicted_up { w.predictions_up += 1; } else { w.predictions_down += 1; }
            let correct = predicted_up == actual_outcome.eq_ignore_ascii_case("up")
                && !actual_outcome.eq_ignore_ascii_case("tie");
            w.correct_predictions += if correct { 1 } else { 0 };
            w.accuracy = w.correct_predictions as f64 / w.predictions_made as f64;
            w.last_10_results.push(correct);
            if w.last_10_results.len() > 10 { w.last_10_results.remove(0); }
            // ─── RL feedback: adjust entry threshold ───────────────────────
            self.rl_update(correct);
        } else {
            self.wisdom.lock().unwrap().total_sessions += 1;
        }

        info!("[{}] Session #{} RESOLVED: pred={} actual={} exit={} pnl={:.4}",
            self.params.name, session_id, cs.prediction, actual_outcome, cs.exit_reason, cs.virtual_pnl);
        Some(cs)
    }

    // ─── Trade helpers ─────────────────────────────────────────────────────

    fn check_exit(&self, trade: &StrategyTrade, mid: f64, bid: f64, ask: f64) -> (bool, bool) {
        match trade.direction {
            Direction::Up => {
                let tp = bid >= self.params.take_profit_up;
                let reversal = trade.peak_favorable - mid;
                let stopped = reversal > self.params.reversal_stop && mid < trade.entry_price;
                (tp, stopped)
            }
            Direction::Down => {
                let tp = ask <= self.params.take_profit_down && ask > 0.0;
                let reversal = mid - trade.peak_favorable;
                let stopped = reversal > self.params.reversal_stop && mid > trade.entry_price;
                (tp, stopped)
            }
        }
    }

    fn update_peaks(&self, trade: &mut StrategyTrade, mid: f64) {
        match trade.direction {
            Direction::Up => {
                if mid > trade.peak_favorable { trade.peak_favorable = mid; }
                if mid < trade.peak_adverse { trade.peak_adverse = mid; }
            }
            Direction::Down => {
                if mid < trade.peak_favorable { trade.peak_favorable = mid; }
                if mid > trade.peak_adverse { trade.peak_adverse = mid; }
            }
        }
    }

    fn close_trade(&self, trade: &mut StrategyTrade, s: &mut StrategySession, mid: f64, bid: f64, ask: f64) {
        let (tp, _stopped) = self.check_exit(trade, mid, bid, ask);
        let exit_price = match trade.direction {
            Direction::Up if tp => bid,
            Direction::Up => bid.max(0.0),
            Direction::Down if tp => ask,
            Direction::Down => ask.min(1.0),
        };
        trade.exit_price = exit_price;
        if tp {
            trade.exit_reason = TradeStatus::TakeProfit;
            trade.virtual_pnl = match trade.direction {
                Direction::Up => exit_price - trade.entry_price,
                Direction::Down => trade.entry_price - exit_price,
            };
            s.exit_reason = TradeStatus::TakeProfit.as_str().into();
        } else {
            trade.exit_reason = TradeStatus::StoppedOut;
            trade.virtual_pnl = match trade.direction {
                Direction::Up => exit_price - trade.entry_price,
                Direction::Down => trade.entry_price - exit_price,
            };
            s.exit_reason = TradeStatus::StoppedOut.as_str().into();
        }
        s.virtual_pnl = trade.virtual_pnl;
        s.exit_price = exit_price;
        s.trade_active = false;
    }

    fn settle_pnl(&self, predicted_up: bool, entry: f64, outcome: &str, final_poly: f64) -> f64 {
        if outcome.eq_ignore_ascii_case("up") {
            if predicted_up { 1.0 - entry } else { -(1.0 - entry) }
        } else if outcome.eq_ignore_ascii_case("down") {
            if !predicted_up { entry - 0.0 } else { -entry }
        } else {
            if predicted_up { final_poly - entry } else { entry - final_poly }
        }
    }

    // ─── Accumulate trade into wisdom ──────────────────────────────────────

    fn accumulate(&self, trade: &StrategyTrade, _session: &StrategySession) {
        let mut w = self.wisdom.lock().unwrap();
        w.trades_entered += 1;
        match trade.exit_reason {
            TradeStatus::TakeProfit => w.trades_tp += 1,
            TradeStatus::StoppedOut => w.trades_stopped += 1,
            TradeStatus::Settled => w.trades_settled += 1,
            _ => {}
        }
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

    // ─── RL feedback ───────────────────────────────────────────────────────

    fn rl_update(&self, correct: bool) {
        let mut w = self.wisdom.lock().unwrap();
        let _delta = if correct { 0.005 } else { -0.01 }; // reward < punishment

        if correct && w.accuracy > 0.70 && w.predictions_made >= 5 {
            // Tighten thresholds: raise UP bar, lower DOWN bar
            w.rl_min_up = (w.rl_min_up + 0.005).min(0.95);
            w.rl_min_down = (w.rl_min_down - 0.005).max(0.05);
        } else if !correct {
            // Widen thresholds to be more conservative
            w.rl_min_up = (w.rl_min_up - 0.01).max(0.80);
            w.rl_min_down = (w.rl_min_down + 0.01).min(0.20);
        }

        // Adjust reversal stop based on average trade PnL
        if w.trades_stopped > 0 && w.trades_stopped as f64 / w.trades_entered as f64 > 0.5 {
            w.rl_reversal_stop = (w.rl_reversal_stop + 0.005).min(0.05);
        }
        if w.trades_tp > 0 {
            w.rl_reversal_stop = (w.rl_reversal_stop - 0.002).max(0.01);
        }
    }

    // ─── Export ────────────────────────────────────────────────────────────

    pub fn export_json(&self) -> String {
        let mut w = self.wisdom.lock().unwrap().clone();
        w.exported_at = Utc::now().to_rfc3339();
        serde_json::to_string_pretty(&w).unwrap_or_default()
    }

    pub fn accuracy(&self) -> f64 { self.wisdom.lock().unwrap().accuracy }
    pub fn cumulative_pnl(&self) -> f64 { self.wisdom.lock().unwrap().cumulative_pnl }
}
