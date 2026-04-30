// ─── Strategy Manager — Multi-estrategia Paper Trading ($20 c/u) ───────────────
//
// Ejecuta DOS estrategias shadow independientes con capital virtual de $20 cada una.
// Ambas comparten el Safety Layer (spread, data integrity, execution).
//
// Trigger A — Imbalance Divergence:
//   BUY  si binance_imbalance > 0.85 Y poly_imbalance < 0.20
//   SELL si binance_imbalance < 0.15 Y poly_imbalance > 5.0
//   TP $0.020/contrato, timeout 20s
//
// Trigger B — Liquidity Grabbing (Cancelaciones):
//   BUY  si poly_ask_vol_all ↓ >40% en 200ms sin TRADE significativo
//   SELL si poly_bid_vol_all ↓ >40% en 200ms sin TRADE significativo
//   TP $0.010/contrato, timeout 8s
//
// Safety Layer (ambas estrategias):
//   – poly_spread > MAX_SPREAD → abort
//   – poly_ask <= 0 || poly_bid <= 0 → abort
//   – poly_ask <= poly_bid → abort
//   – Compra al ask, venta al bid
//   – $0.001 comisión/contrato

use std::collections::VecDeque;
use crate::modules::hft::types::CsvRecord;
use crate::modules::hft::types::EventType;
use tracing;

// ─── Constantes ──────────────────────────────────────────────────────────────

const INITIAL_CAPITAL: f64 = 20.00;
const COMMISSION_PER_CONTRACT: f64 = 0.001;
const MAX_SPREAD: f64 = 0.05;

// Trigger A: Imbalance Divergence
const IMB_TRIGGER_BINANCE_BUY: f32 = 0.85;
const IMB_TRIGGER_POLY_BUY: f64 = 0.20;
const IMB_TRIGGER_BINANCE_SELL: f32 = 0.15;
const IMB_TRIGGER_POLY_SELL: f64 = 5.0;
const IMB_TP_PER_CONTRACT: f64 = 0.020;
const IMB_TIMEOUT_MS: i64 = 20_000;

// Trigger B: Liquidity Grabbing
const LIQ_WINDOW_MS: i64 = 200;
const LIQ_DROP_PCT: f64 = 0.40;
const LIQ_TP_PER_CONTRACT: f64 = 0.010;
const LIQ_TIMEOUT_MS: i64 = 8_000;

// ─── Tipos ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimSide {
    Buy,
    Sell,
}

impl SimSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone)]
struct Position {
    side:            SimSide,
    entry_price:     f64,
    entry_time_ms:   i64,
    size_contracts:  f64,
}

#[derive(Debug, Clone, Default)]
pub struct StratState {
    pub status:          String,
    pub side:            String,
    pub entry_price:     f64,
    pub exit_price:      f64,
    pub trade_pnl:       f64,
    pub balance:         f64,
}

#[derive(Debug, Clone, Default)]
pub struct StrategyResult {
    pub imba: StratState,
    pub liqb: StratState,
}

// ─── Per-Strategy Instance (mutable state) ───────────────────────────────────

struct StrategyInstance {
    balance:  f64,
    position: Option<Position>,
    pnl:      f64,
    trades:   u32,
}

impl StrategyInstance {
    fn new() -> Self {
        Self { balance: INITIAL_CAPITAL, position: None, pnl: 0.0, trades: 0 }
    }
}

// ─── VolHistory — 200ms sliding window ──────────────────────────────────────

struct VolHistory {
    window: VecDeque<(i64, f64, f64)>, // (ts_ms, ask_vol, bid_vol)
    win_ms: i64,
}

impl VolHistory {
    fn new(window_ms: i64) -> Self {
        Self { window: VecDeque::with_capacity(128), win_ms: window_ms }
    }

    fn push(&mut self, now_ms: i64, ask_vol: f64, bid_vol: f64) {
        self.window.push_back((now_ms, ask_vol, bid_vol));
        let cutoff = now_ms - self.win_ms;
        while self.window.front().map_or(false, |(ts, _, _)| *ts < cutoff) {
            self.window.pop_front();
        }
    }

    fn max_ask_vol(&self) -> (f64, f64) {
        self.window.iter()
            .map(|(_, a, b)| (*a, *b))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0.0, 0.0))
    }

    fn max_bid_vol(&self) -> (f64, f64) {
        self.window.iter()
            .map(|(_, a, b)| (*b, *a))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0.0, 0.0))
    }

    fn len(&self) -> usize {
        self.window.len()
    }
}

// ─── StrategyManager ─────────────────────────────────────────────────────────

pub struct StrategyManager {
    imba:   StrategyInstance,
    liqb:   StrategyInstance,
    vol_history: VolHistory,
    last_trade_ts: i64,
}

impl StrategyManager {
    pub fn new() -> Self {
        Self {
            imba:          StrategyInstance::new(),
            liqb:          StrategyInstance::new(),
            vol_history:   VolHistory::new(LIQ_WINDOW_MS),
            last_trade_ts: 0,
        }
    }

    pub fn evaluate(&mut self, rec: &CsvRecord) -> StrategyResult {
        let now_ms = rec.ts_exchange.parse::<i64>().unwrap_or(0);

        // Update volume history for Trigger B
        self.vol_history.push(now_ms, rec.poly_ask_vol_all, rec.poly_bid_vol_all);

        if rec.event_type == EventType::Trade {
            self.last_trade_ts = now_ms;
        }

        // Safety Layer — shared
        let safe = safety_check(rec);

        let imba_state: StratState;
        let liqb_state: StratState;

        if safe {
            imba_state = process_strategy(
                &mut self.imba, rec, now_ms,
                IMB_TP_PER_CONTRACT, IMB_TIMEOUT_MS,
                |rec| imbalance_trigger(rec),
            );

            // Trigger B needs external state (vol_history, last_trade_ts)
            liqb_state = if self.vol_history.len() >= 3 {
                let trade_in_window = now_ms - self.last_trade_ts <= LIQ_WINDOW_MS;
                let (max_ask, _) = self.vol_history.max_ask_vol();
                let (max_bid, _) = self.vol_history.max_bid_vol();
                process_strategy(
                    &mut self.liqb, rec, now_ms,
                    LIQ_TP_PER_CONTRACT, LIQ_TIMEOUT_MS,
                    |rec| liquidity_trigger(rec, trade_in_window, max_ask, max_bid),
                )
            } else {
                close_only(&mut self.liqb, rec, now_ms, LIQ_TP_PER_CONTRACT, LIQ_TIMEOUT_MS)
            };
        } else {
            imba_state = close_only(&mut self.imba, rec, now_ms, IMB_TP_PER_CONTRACT, IMB_TIMEOUT_MS);
            liqb_state = close_only(&mut self.liqb, rec, now_ms, LIQ_TP_PER_CONTRACT, LIQ_TIMEOUT_MS);
        }

        StrategyResult { imba: imba_state, liqb: liqb_state }
    }
}

// ─── Safety Layer ────────────────────────────────────────────────────────────

fn safety_check(rec: &CsvRecord) -> bool {
    if rec.poly_ask <= 0.0 || rec.poly_bid <= 0.0 {
        return false;
    }
    if rec.poly_ask <= rec.poly_bid {
        return false;
    }
    let spread = rec.poly_ask - rec.poly_bid;
    spread <= MAX_SPREAD
}

// ─── Strategy processing ─────────────────────────────────────────────────────

fn close_only(
    inst: &mut StrategyInstance,
    rec: &CsvRecord,
    now_ms: i64,
    tp_per_contract: f64,
    timeout_ms: i64,
) -> StratState {
    evaluate_close(inst, rec, now_ms, tp_per_contract, timeout_ms)
}

fn process_strategy<F>(
    inst: &mut StrategyInstance,
    rec: &CsvRecord,
    now_ms: i64,
    tp_per_contract: f64,
    timeout_ms: i64,
    trigger_fn: F,
) -> StratState
where
    F: Fn(&CsvRecord) -> Option<(SimSide, String)>,
{
    if inst.position.is_some() {
        return evaluate_close(inst, rec, now_ms, tp_per_contract, timeout_ms);
    }

    let (side, _reason) = match trigger_fn(rec) {
        Some(s) => s,
        None => return StratState {
            status: "IDLE".into(),
            balance: inst.balance,
            ..Default::default()
        },
    };

    let entry_price = match side {
        SimSide::Buy => rec.poly_ask,
        SimSide::Sell => rec.poly_bid,
    };

    let max_contracts = (inst.balance * 0.5 / entry_price).floor();
    if max_contracts < 1.0 {
        return StratState { status: "IDLE".into(), balance: inst.balance, ..Default::default() };
    }
    let size = max_contracts.min(10.0);

    let entry_commission = COMMISSION_PER_CONTRACT * size;
    inst.balance -= entry_commission;

    inst.position = Some(Position { side, entry_price, entry_time_ms: now_ms, size_contracts: size });

    StratState {
        status:  "OPEN".into(),
        side:    side.as_str().into(),
        entry_price,
        exit_price: 0.0,
        trade_pnl:  0.0,
        balance:    inst.balance,
    }
}

fn evaluate_close(
    inst: &mut StrategyInstance,
    rec: &CsvRecord,
    now_ms: i64,
    tp_per_contract: f64,
    timeout_ms: i64,
) -> StratState {
    let pos = match &inst.position {
        Some(p) => p.clone(),
        None => return StratState { status: "IDLE".into(), balance: inst.balance, ..Default::default() },
    };

    let exit_price = match pos.side {
        SimSide::Buy => rec.poly_bid,
        SimSide::Sell => rec.poly_ask,
    };
    if exit_price <= 0.0 {
        return StratState {
            status: "OPEN".into(),
            side: pos.side.as_str().into(),
            entry_price: pos.entry_price,
            exit_price: 0.0,
            trade_pnl: 0.0,
            balance: inst.balance,
        };
    }

    let pnl_per_contract = match pos.side {
        SimSide::Buy => exit_price - pos.entry_price,
        SimSide::Sell => pos.entry_price - exit_price,
    };
    let gross_pnl = pnl_per_contract * pos.size_contracts;

    let tp_hit = gross_pnl >= tp_per_contract * pos.size_contracts;
    let timed_out = now_ms - pos.entry_time_ms >= timeout_ms;

    if tp_hit || timed_out {
        let commission = COMMISSION_PER_CONTRACT * pos.size_contracts;
        let net_pnl = gross_pnl - commission;
        inst.balance += net_pnl;
        inst.pnl += net_pnl;
        inst.trades += 1;

        if tp_hit {
            tracing::debug!("[PAPER] TP hit: side={:?} pnl={:.4} bal={:.4}", pos.side, net_pnl, inst.balance);
        } else {
            tracing::debug!("[PAPER] Timeout: side={:?} pnl={:.4} bal={:.4}", pos.side, net_pnl, inst.balance);
        }

        let result = StratState {
            status: "CLOSED".into(),
            side: pos.side.as_str().into(),
            entry_price: pos.entry_price,
            exit_price,
            trade_pnl: net_pnl,
            balance: inst.balance,
        };
        inst.position = None;
        return result;
    }

    StratState {
        status: "OPEN".into(),
        side: pos.side.as_str().into(),
        entry_price: pos.entry_price,
        exit_price: 0.0,
        trade_pnl: 0.0,
        balance: inst.balance,
    }
}

// ─── Trigger A: Imbalance Divergence ─────────────────────────────────────────

fn imbalance_trigger(rec: &CsvRecord) -> Option<(SimSide, String)> {
    let bin_imb = rec.binance_imbalance;
    let poly_imb = rec.poly_imbalance;

    if bin_imb > IMB_TRIGGER_BINANCE_BUY && poly_imb < IMB_TRIGGER_POLY_BUY {
        tracing::info!("[IMBALANCE] BUY signal: bin_imb={:.3} poly_imb={:.4}", bin_imb, poly_imb);
        return Some((SimSide::Buy, "imbalance_buy".into()));
    }
    if bin_imb < IMB_TRIGGER_BINANCE_SELL && poly_imb > IMB_TRIGGER_POLY_SELL {
        tracing::info!("[IMBALANCE] SELL signal: bin_imb={:.3} poly_imb={:.4}", bin_imb, poly_imb);
        return Some((SimSide::Sell, "imbalance_sell".into()));
    }
    None
}

// ─── Trigger B: Liquidity Grabbing ───────────────────────────────────────────

fn liquidity_trigger(
    rec: &CsvRecord,
    trade_in_window: bool,
    max_ask: f64,
    max_bid: f64,
) -> Option<(SimSide, String)> {
    let current_ask = rec.poly_ask_vol_all;
    let current_bid = rec.poly_bid_vol_all;

    if !trade_in_window {
        if max_ask > 0.0 {
            let drop_pct = (max_ask - current_ask) / max_ask;
            if drop_pct > LIQ_DROP_PCT {
                tracing::info!(
                    "[LIQUIDITY] BUY: ask_vol drop {:.1}% ({}→{})",
                    drop_pct * 100.0, max_ask, current_ask
                );
                return Some((SimSide::Buy, "liquidity_buy".into()));
            }
        }
        if max_bid > 0.0 {
            let drop_pct = (max_bid - current_bid) / max_bid;
            if drop_pct > LIQ_DROP_PCT {
                tracing::info!(
                    "[LIQUIDITY] SELL: bid_vol drop {:.1}% ({}→{})",
                    drop_pct * 100.0, max_bid, current_bid
                );
                return Some((SimSide::Sell, "liquidity_sell".into()));
            }
        }
    }
    None
}
