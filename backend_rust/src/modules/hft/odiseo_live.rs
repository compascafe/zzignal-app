//! Odiseo Live Trading — real order execution + per-variant control
//! 
//! Each Odiseo variant can be independently toggled ON/OFF.
//! Live mode: places real CLOB orders. Paper mode: simulation only.
//! Tracks per-session P&L for display.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::modules::core::worker::{CmdMsg, OrderSide, Outcome as WorkerOutcome};

// ─── Per-variant configuration ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OdiseoVariantConfig {
    pub code:     String,
    pub name:     String,
    pub enabled:  bool,
    pub entry:    f64,
    pub tp:       f64,
    pub sl:       f64,
}

impl OdiseoVariantConfig {
    pub fn new(code: &str, name: &str, entry: f64, tp: f64, sl: f64) -> Self {
        Self { code: code.into(), name: name.into(), enabled: true, entry, tp, sl }
    }
}

// ─── P&L Session Summary ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct OdiseoSessionSummary {
    pub session_id:   i32,
    pub outcome:      String,
    pub btc_delta:    f64,
    pub variants:     Vec<OdiseoVariantResult>,
    pub total_pnl:    f64,
    pub entries:      u64,
    pub wins:         u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OdiseoVariantResult {
    pub code:   String,
    pub name:   String,
    pub up_pnl: f64,
    pub dn_pnl: f64,
    pub total:  f64,
}

// ─── Live Trading Controller ──────────────────────────────────────────────

pub struct OdiseoLiveController {
    /// Per-variant enabled state
    pub variants:     Mutex<Vec<OdiseoVariantConfig>>,
    /// true = real orders, false = paper only
    pub live_mode:    AtomicBool,
    /// Per-session P&L summaries (session_id → summary)
    pub history:      Mutex<HashMap<i32, OdiseoSessionSummary>>,
    /// Cmd channel for placing real orders
    pub cmd_tx:       tokio::sync::mpsc::UnboundedSender<CmdMsg>,
}

impl OdiseoLiveController {
    pub fn new(cmd_tx: tokio::sync::mpsc::UnboundedSender<CmdMsg>) -> Self {
        use crate::modules::hft::odiseo_strategies::ODISEO_DEFS_STATIC;
        let variants = ODISEO_DEFS_STATIC.iter().map(|d| {
            OdiseoVariantConfig::new(d.code, d.name, d.entry_threshold, d.tp_price, d.sl_hard)
        }).collect();
        Self {
            variants: Mutex::new(variants),
            live_mode: AtomicBool::new(false),
            history:  Mutex::new(HashMap::new()),
            cmd_tx,
        }
    }

    /// Toggle a variant on/off
    pub fn set_variant(&self, code: &str, enabled: bool) -> bool {
        let mut vars = self.variants.lock().unwrap();
        if let Some(v) = vars.iter_mut().find(|v| v.code == code) {
            v.enabled = enabled;
            return true;
        }
        false
    }

    /// Toggle live mode
    pub fn set_live_mode(&self, live: bool) {
        self.live_mode.store(live, Ordering::Relaxed);
    }

    /// Check if a variant is enabled
    pub fn is_variant_enabled(&self, code: &str) -> bool {
        self.variants.lock().unwrap()
            .iter().any(|v| v.code == code && v.enabled)
    }

    /// Place a real buy order on Polymarket
    pub fn place_entry_order(&self, is_up: bool, price: f64, size: f64) {
        if !self.live_mode.load(Ordering::Relaxed) { return; }
        if size < 1.0 || price <= 0.0 || price >= 1.0 { return; }
        
        let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
        let cmd = CmdMsg::PlaceLimitOrder {
            side: OrderSide::Buy,
            outcome,
            price,
            size: size as f64,
        };
        let _ = self.cmd_tx.send(cmd);
        tracing::info!("[OdiseoLive] LIMIT BUY {} @{:.4} x{:.0}", if is_up {"UP"} else {"DOWN"}, price, size);
    }

    /// Place a real sell order on Polymarket (TP or SL exit)
    pub fn place_exit_order(&self, is_up: bool, price: f64, size: f64, is_tp: bool) {
        if !self.live_mode.load(Ordering::Relaxed) { return; }
        if size < 1.0 { return; }
        
        let outcome = if is_up { WorkerOutcome::Up } else { WorkerOutcome::Down };
        if is_tp {
            // Limit sell at TP
            let cmd = CmdMsg::PlaceLimitOrder {
                side: OrderSide::Sell,
                outcome,
                price: price.min(0.999),
                size: size as f64,
            };
            let _ = self.cmd_tx.send(cmd);
            tracing::info!("[OdiseoLive] LIMIT SELL {} @{:.4} x{:.0}", if is_up {"UP"} else {"DOWN"}, price, size);
        } else {
            // Market sell for SL
            let cmd = CmdMsg::PlaceMarketOrder {
                side: OrderSide::Sell,
                outcome,
                amount_usdc: (price * size).max(1.0),
            };
            let _ = self.cmd_tx.send(cmd);
            tracing::info!("[OdiseoLive] MARKET SELL {} ~${:.2}", if is_up {"UP"} else {"DOWN"}, price * size);
        }
    }

    /// Record a session P&L summary
    pub fn record_session(&self, summary: OdiseoSessionSummary) {
        self.history.lock().unwrap().insert(summary.session_id, summary);
    }

    /// Get config as JSON
    pub fn get_config(&self) -> serde_json::Value {
        let vars = self.variants.lock().unwrap();
        let live = self.live_mode.load(Ordering::Relaxed);
        json!({
            "live_mode": live,
            "variants": &*vars,
        })
    }

    /// Get P&L history
    pub fn get_history(&self, limit: usize) -> serde_json::Value {
        let history = self.history.lock().unwrap();
        let mut sessions: Vec<_> = history.values().cloned().collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.session_id));
        sessions.truncate(limit);
        json!(sessions)
    }
}
