//! Performance instrumentation for the hot path.
//!
//! Each operation in the tick/book pipeline records cumulative time and call
//! count. Access via `GET /api/perf` — returns JSON with per-operation stats.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Per-operation latency accumulator: (total_nanos, call_count)
pub struct PerfSlot {
    pub total_ns: AtomicU64, // cumulative nanoseconds
    pub count: AtomicU64,    // number of invocations
    pub label: &'static str,
}

/// Guard returned by `start()`. Drops into the corresponding slot.
pub struct PerfGuard<'a> {
    slot: &'a PerfSlot,
    started: Instant,
}

impl Drop for PerfGuard<'_> {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_nanos() as u64;
        self.slot.total_ns.fetch_add(elapsed, Ordering::Relaxed);
        self.slot.count.fetch_add(1, Ordering::Relaxed);
    }
}

impl PerfSlot {
    pub const fn new(label: &'static str) -> Self {
        Self {
            total_ns: AtomicU64::new(0),
            count: AtomicU64::new(0),
            label,
        }
    }

    pub fn start(&self) -> PerfGuard<'_> {
        PerfGuard {
            slot: self,
            started: Instant::now(),
        }
    }

    pub fn avg_us(&self) -> f64 {
        let n = self.count.load(Ordering::Relaxed);
        if n == 0 {
            return 0.0;
        }
        self.total_ns.load(Ordering::Relaxed) as f64 / n as f64 / 1000.0
    }
}

// ─── Slots ────────────────────────────────────────────────────────────────────

pub static TICK_CONSUMER: PerfSlot = PerfSlot::new("tick_consumer");
pub static RECORD_SAMPLE: PerfSlot = PerfSlot::new("record_price_sample");
pub static BUILD_TICK: PerfSlot = PerfSlot::new("build_binance_tick");
pub static SM_PUSH: PerfSlot = PerfSlot::new("session_manager_push");
pub static CONSUMER_LOOP: PerfSlot = PerfSlot::new("consumer_iter");
pub static BROADCAST_SEND: PerfSlot = PerfSlot::new("broadcast_send");

// ─── Dump all counters as a JSON string ───────────────────────────────────────

pub fn dump_json() -> String {
    let slots: &[&PerfSlot] = &[
        &TICK_CONSUMER,
        &RECORD_SAMPLE,
        &BUILD_TICK,
        &SM_PUSH,
        &CONSUMER_LOOP,
        &BROADCAST_SEND,
    ];

    let parts: Vec<String> = slots
        .iter()
        .map(|s| {
            let n = s.count.load(Ordering::Relaxed);
            format!(
                r#"  "{}": {{ "calls": {}, "total_us": {:.2}, "avg_us": {:.2} }}"#,
                s.label,
                n,
                s.total_ns.load(Ordering::Relaxed) as f64 / 1000.0,
                s.avg_us(),
            )
        })
        .collect();

    format!("{{\n{}\n}}", parts.join(",\n"))
}
