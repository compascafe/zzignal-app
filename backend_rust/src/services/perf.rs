//! Performance instrumentation for the HFT hot path.
//!
//! Each operation in the tick/book pipeline records cumulative time and call count.
//! Access via `GET /api/perf` — returns JSON with per-operation stats.
//!
//! Reset via `POST /api/perf/reset`.

use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::Instant;

/// Per-operation latency accumulator: (total_nanos, call_count)
pub struct PerfSlot {
    pub total_ns: AtomicU64,  // cumulative nanoseconds
    pub count:    AtomicU64,  // number of invocations
    pub label:    &'static str,
}

/// Guard returned by `start()`. Drops into the corresponding slot.
pub struct PerfGuard<'a> {
    slot:    &'a PerfSlot,
    started: Instant,
}

impl<'a> Drop for PerfGuard<'a> {
    fn drop(&mut self) {
        if !ENABLED.load(Ordering::Relaxed) { return; }
        let elapsed = self.started.elapsed().as_nanos() as u64;
        self.slot.total_ns.fetch_add(elapsed, Ordering::Relaxed);
        self.slot.count.fetch_add(1, Ordering::Relaxed);
    }
}

impl PerfSlot {
    pub const fn new(label: &'static str) -> Self {
        Self { total_ns: AtomicU64::new(0), count: AtomicU64::new(0), label }
    }

    pub fn start(&self) -> PerfGuard<'_> {
        PerfGuard { slot: self, started: Instant::now() }
    }

    pub fn avg_us(&self) -> f64 {
        let n = self.count.load(Ordering::Relaxed);
        if n == 0 { return 0.0; }
        self.total_ns.load(Ordering::Relaxed) as f64 / n as f64 / 1000.0
    }

    pub fn reset(&self) {
        self.total_ns.store(0, Ordering::Relaxed);
        self.count.store(0, Ordering::Relaxed);
    }
}

/// Toggle globally — enabled by default.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

// ─── Slots ─────────────────────────────────────────────────────────────────────────

pub static TICK_CONSUMER:     PerfSlot = PerfSlot::new("tick_consumer");
pub static TRACK_PRICE:       PerfSlot = PerfSlot::new("track_price");
pub static RECORD_TRADE:      PerfSlot = PerfSlot::new("record_binance_trade");
pub static RECORD_SAMPLE:     PerfSlot = PerfSlot::new("record_price_sample");
pub static PUSH_BOLLINGER:    PerfSlot = PerfSlot::new("push_bollinger_price");
pub static BUILD_TICK:        PerfSlot = PerfSlot::new("build_binance_tick");
pub static ENGINE_LOCK:       PerfSlot = PerfSlot::new("adaptive_engine_lock");
pub static EVAL_MASTER:       PerfSlot = PerfSlot::new("evaluate_master_signal");
pub static MACRO_CTX_WRITE:   PerfSlot = PerfSlot::new("macro_ctx_write");
pub static UPDATE_RSI:        PerfSlot = PerfSlot::new("update_dynamic_rsi");
pub static CHECK_MOMENTUM:    PerfSlot = PerfSlot::new("check_momentum_trigger");
pub static CSV_PUSH:          PerfSlot = PerfSlot::new("csv_logger_push");
pub static SM_PUSH:           PerfSlot = PerfSlot::new("session_manager_push");
pub static BUILD_BOOK:        PerfSlot = PerfSlot::new("build_book_update");
pub static CAPTURE_COMBINED:  PerfSlot = PerfSlot::new("capture_combined");
pub static DB_INSERT_BOOK:    PerfSlot = PerfSlot::new("capture_book_db");
pub static DB_INSERT_FILL:    PerfSlot = PerfSlot::new("insert_fill");
pub static STRATEGY_EVAL:     PerfSlot = PerfSlot::new("strategy_evaluate");

// ─── Dump all counters as JSON string ────────────────────────────────────────────

pub fn dump_json() -> String {
    let slots: &[&PerfSlot] = &[
        &TICK_CONSUMER, &TRACK_PRICE, &RECORD_TRADE, &RECORD_SAMPLE, &PUSH_BOLLINGER,
        &BUILD_TICK, &ENGINE_LOCK, &EVAL_MASTER, &MACRO_CTX_WRITE,
        &UPDATE_RSI, &CHECK_MOMENTUM, &CSV_PUSH, &SM_PUSH,
        &BUILD_BOOK, &CAPTURE_COMBINED, &DB_INSERT_BOOK, &DB_INSERT_FILL,
        &STRATEGY_EVAL,
    ];

    let parts: Vec<String> = slots.iter().map(|s| {
        let n = s.count.load(Ordering::Relaxed);
        format!(
            r#"  "{}": {{ "calls": {}, "total_us": {:.2}, "avg_us": {:.2} }}"#,
            s.label, n,
            s.total_ns.load(Ordering::Relaxed) as f64 / 1000.0,
            s.avg_us(),
        )
    }).collect();

    format!("{{\n{}\n}}", parts.join(",\n"))
}

pub fn reset_all() {
    let slots: &[&PerfSlot] = &[
        &TICK_CONSUMER, &TRACK_PRICE, &RECORD_TRADE, &RECORD_SAMPLE, &PUSH_BOLLINGER,
        &BUILD_TICK, &ENGINE_LOCK, &EVAL_MASTER, &MACRO_CTX_WRITE,
        &UPDATE_RSI, &CHECK_MOMENTUM, &CSV_PUSH, &SM_PUSH,
        &BUILD_BOOK, &CAPTURE_COMBINED, &DB_INSERT_BOOK, &DB_INSERT_FILL,
        &STRATEGY_EVAL,
    ];
    for s in slots { s.reset(); }
}
