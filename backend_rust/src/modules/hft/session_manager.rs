use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

/// Manages per-session CSV file isolation.
/// Each session gets its own file: `{data_dir}/session_{id:04}_hft.csv`.
/// Between sessions the writer is None (IDLE state).
pub struct SessionManager {
    active_session_id: Mutex<Option<i32>>,
    writer:            Mutex<Option<BufWriter<File>>>,
    file_path:         Mutex<Option<String>>,
    tick_count:        Mutex<u64>,
    trade_count:       Mutex<u64>,
    data_dir:          String,
}

impl SessionManager {
    pub fn new(data_dir: &str) -> Self {
        std::fs::create_dir_all(data_dir).ok();
        Self {
            active_session_id: Mutex::new(None),
            writer:            Mutex::new(None),
            file_path:         Mutex::new(None),
            tick_count:        Mutex::new(0),
            trade_count:       Mutex::new(0),
            data_dir:          data_dir.to_string(),
        }
    }

    /// Start a new session. Closes any previous writer, creates a new empty CSV file.
    /// The file is created with `truncate(true)` — no residual data from prior sessions.
    pub fn start_session(&self, session_id: i32) -> Result<(), String> {
        // 1. Close previous session (flush + close file)
        self.close_writer("start_session")?;

        // 2. Reset counters to zero — no metadata carry-over
        *self.tick_count.lock().unwrap() = 0;
        *self.trade_count.lock().unwrap() = 0;

        // 3. Create new file — truncate ensures it's empty
        let path = format!("{}/session_{:04}_hft.csv", self.data_dir, session_id);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("SessionManager: cannot create {}: {}", path, e))?;

        let mut writer = BufWriter::with_capacity(65536, file);

        // Write 32-column header
        let _ = writeln!(
            writer,
            "ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,\
             binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,\
             poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,\
             trade_price,trade_size,is_informed,\
             imba_status,imba_side,imba_entry_price,imba_exit_price,imba_trade_pnl,imba_balance,\
             liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance"
        );

        *self.writer.lock().unwrap() = Some(writer);
        *self.file_path.lock().unwrap() = Some(path.clone());
        *self.active_session_id.lock().unwrap() = Some(session_id);

        info!("SessionManager: started session #{} → {}", session_id, path);
        Ok(())
    }

    /// Push a CsvRecord to the current session's file.
    /// Returns true if the record was written, false if no session is active (IDLE).
    pub fn push(&self, record: &CsvRecord) -> bool {
        let mut writer_guard = self.writer.lock().unwrap();
        match writer_guard.as_mut() {
            Some(w) => {
                let _ = writeln!(
                    w,
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    record.ts_local,
                    record.ts_exchange,
                    record.event_type.as_str(),
                    record.latencia_ms,
                    record.binance_price,
                    record.binance_micro_price,
                    record.binance_imbalance,
                    record.binance_vol_100ms,
                    record.binance_vol_24h,
                    record.poly_bid,
                    record.poly_ask,
                    record.poly_mid,
                    record.poly_spread,
                    record.poly_bid_vol_all,
                    record.poly_ask_vol_all,
                    record.poly_imbalance,
                    record.trade_side,
                    record.trade_price,
                    record.trade_size,
                    record.is_informed,
                    record.imba_status,
                    record.imba_side,
                    record.imba_entry_price,
                    record.imba_exit_price,
                    record.imba_trade_pnl,
                    record.imba_balance,
                    record.liqb_status,
                    record.liqb_side,
                    record.liqb_entry_price,
                    record.liqb_exit_price,
                    record.liqb_trade_pnl,
                    record.liqb_balance,
                );
                drop(writer_guard);

                // Increment per-session counters
                match record.event_type {
                    EventType::Trade => *self.trade_count.lock().unwrap() += 1,
                    _ => *self.tick_count.lock().unwrap() += 1,
                }
                true
            }
            None => false,
        }
    }

    /// Flush and close the current session file. Called on session stop.
    /// This is the strict stop: file is flushed, closed, handle dropped.
    pub fn stop_session(&self) -> Result<(), String> {
        let id = self.active_id();
        self.close_writer("stop_session")?;
        if let Some(sid) = id {
            info!("SessionManager: stopped session #{}", sid);
        }
        Ok(())
    }

    /// Flush the current writer without closing (periodic safety flush).
    pub fn flush(&self) -> Result<(), String> {
        let mut w = self.writer.lock().unwrap();
        if let Some(ref mut writer) = *w {
            writer
                .flush()
                .map_err(|e| format!("SessionManager flush error: {}", e))?;
        }
        Ok(())
    }

    pub fn active_id(&self) -> Option<i32> {
        *self.active_session_id.lock().unwrap()
    }

    pub fn is_idle(&self) -> bool {
        self.active_id().is_none()
    }

    pub fn get_tick_count(&self) -> u64 {
        *self.tick_count.lock().unwrap()
    }
    pub fn get_trade_count(&self) -> u64 {
        *self.trade_count.lock().unwrap()
    }

    /// Path to the current session file (for export fallback).
    pub fn current_path(&self) -> Option<String> {
        self.file_path.lock().unwrap().clone()
    }

    /// Path to a specific session's file (may not exist if not yet created or already cleaned).
    pub fn session_path(&self, session_id: i32) -> String {
        format!("{}/session_{:04}_hft.csv", self.data_dir, session_id)
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    fn close_writer(&self, caller: &str) -> Result<(), String> {
        let mut writer_guard = self.writer.lock().unwrap();
        if let Some(ref mut w) = *writer_guard {
            if let Err(e) = w.flush() {
                warn!("SessionManager::{} flush error: {}", caller, e);
            }
        }
        *writer_guard = None; // drop BufWriter → close file
        *self.file_path.lock().unwrap() = None;
        *self.active_session_id.lock().unwrap() = None;
        Ok(())
    }
}
