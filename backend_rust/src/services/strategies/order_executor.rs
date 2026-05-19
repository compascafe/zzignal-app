// ─── Abstract OrderExecutor — Plug-in Live Trading Architecture ───────────────
//
// Design: Rust trait abstracts order placement/cancellation behind a common
// interface. VirtualExecutor logs decisions without sending real orders.
// When confidence threshold is reached, swap in a LiveExecutor with API keys.
//
// Layer: sits between the Master Signal engine and the exchange API/CLOB.

use tracing::info;

pub enum ExecSide {
    Buy,
    Sell,
}
pub enum ExecOutcome {
    Up,
    Down,
}

/// Result of an attempted order placement.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub success:   bool,
    pub order_id:  Option<String>,
    pub message:   String,
}

/// Core abstraction: any order executor must implement this trait.
pub trait OrderExecutor: Send + Sync {
    fn place_limit(&mut self, side: ExecSide, outcome: ExecOutcome, price: f64, size: f64) -> ExecResult;
    fn place_market(&mut self, side: ExecSide, outcome: ExecOutcome, amount_usdc: f64) -> ExecResult;
    fn cancel_order(&mut self, order_id: &str) -> ExecResult;
    fn cancel_all(&mut self) -> ExecResult;
}

// ─── Virtual Executor (Paper Trading / Shadow Mode) ────────────────────────────

pub struct VirtualExecutor;

impl OrderExecutor for VirtualExecutor {
    fn place_limit(&mut self, side: ExecSide, _outcome: ExecOutcome, price: f64, size: f64) -> ExecResult {
        let s = match side { ExecSide::Buy => "BUY", ExecSide::Sell => "SELL" };
        info!("[VIRTUAL] LIMIT {} @ ${:.4} x {}", s, price, size);
        ExecResult {
            success: true,
            order_id: Some("virtual-0".into()),
            message: format!("Virtual {} limit executed", s),
        }
    }
    fn place_market(&mut self, side: ExecSide, _outcome: ExecOutcome, amount_usdc: f64) -> ExecResult {
        let s = match side { ExecSide::Buy => "BUY", ExecSide::Sell => "SELL" };
        info!("[VIRTUAL] MARKET {} $${:.2}", s, amount_usdc);
        ExecResult {
            success: true,
            order_id: Some("virtual-0".into()),
            message: format!("Virtual {} market executed", s),
        }
    }
    fn cancel_order(&mut self, order_id: &str) -> ExecResult {
        info!("[VIRTUAL] CANCEL {}", order_id);
        ExecResult { success: true, order_id: None, message: "Virtual cancel".into() }
    }
    fn cancel_all(&mut self) -> ExecResult {
        info!("[VIRTUAL] CANCEL ALL");
        ExecResult { success: true, order_id: None, message: "Virtual cancel all".into() }
    }
}
