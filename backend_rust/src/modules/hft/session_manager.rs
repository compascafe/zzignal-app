use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

/// Pre-format a CsvRecord into a CSV line string.
/// Uses a single pre-allocated String with write! to avoid per-field allocation.
/// Called outside the writer lock — only the final `write_all` is inside the mutex.
#[inline]
fn fast_format_csv_line(r: &CsvRecord) -> String {
    use std::fmt::Write;
    // One allocation, no reallocs for the final string
    let mut out = String::with_capacity(512);
    let _ = write!(
        out,
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},\
         {},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},\
         {},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        r.ts_local, r.ts_exchange, r.event_type.as_str(), r.latencia_ms,
        r.binance_price, r.binance_micro_price, r.binance_imbalance,
        r.binance_vol_100ms, r.binance_vol_24h,
        r.poly_bid, r.poly_ask, r.poly_mid, r.poly_spread,
        r.poly_bid_vol_all, r.poly_ask_vol_all, r.poly_imbalance,
        r.trade_side, r.trade_price, r.trade_size,
        r.is_informed,
        r.imba_status, r.imba_side, r.imba_entry_price, r.imba_exit_price,
        r.imba_trade_pnl, r.imba_balance,
        r.liqb_status, r.liqb_side, r.liqb_entry_price, r.liqb_exit_price,
        r.liqb_trade_pnl, r.liqb_balance,
        r.trades_per_second, r.price_velocity, r.poly_liquidity_delta,
        r.absorption_ratio, r.price_gap_ratio,
        r.spoofing_flag, r.tape_speed_flag, r.gap_alert_flag,
        r.bollinger_sma, r.bollinger_upper, r.bollinger_lower,
        r.mean_reversion_signal, r.technical_confluence,
        r.trend_direction, r.signal_label,
        r.realized_volatility, r.high_volatility_event, r.bollinger_position,
        r.master_signal, r.cp_uncertainty_range, r.cp_valid_signal,
        r.macro_slope, r.vfi_value, r.macd_hist,
        r.predicted_bias, r.is_feedback_adjusted,
        r.dynamic_rsi, r.vfi_confidence, r.db_accuracy_factor,
        r.t5_prediction, r.t5_entry_price, r.t5_correct,
        r.t3_prediction, r.t3_entry_price, r.t3_active,
        r.pnr_active, r.pnr_seconds_left, r.pnr_price,
        r.pnr_return_up, r.pnr_return_down, r.pnr_volatility_1m,
        r.pnr_confidence, r.pnr_trend, r.pnr_spread_pct,
    );
    out
}

struct SessionWriter {
    writer:    BufWriter<File>,
    path:      String,
    tick_count: u64,
    trade_count: u64,
    row_count: u64,
}

/// Manages per-session CSV file isolation with MULTIPLE concurrent writers.
/// Each session gets its own file: `{data_dir}/session_{id:04}_hft.csv`.
/// Sessions are fully isolated — writing to one never closes another.
pub struct SessionManager {
    writers:  Mutex<HashMap<i32, SessionWriter>>,
    data_dir: String,
}

impl SessionManager {
    pub fn new(data_dir: &str) -> Self {
        std::fs::create_dir_all(data_dir).ok();
        Self {
            writers:  Mutex::new(HashMap::new()),
            data_dir: data_dir.to_string(),
        }
    }

    /// Start a new session writer. Does NOT close other sessions' writers.
    pub fn start_session(&self, session_id: i32) -> Result<(), String> {
        let path = format!("{}/session_{:04}_hft.csv", self.data_dir, session_id);
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("SessionManager: cannot create {}: {}", path, e))?;

        let mut writer = BufWriter::with_capacity(10_485_760, file); // 10 MiB — cabe sesión de hasta ~20k filas en RAM

        // Write 51-column header
        let _ = writeln!(
            writer,
            "ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,\
             binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,\
             poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,\
             trade_price,trade_size,is_informed,\
             imba_status,imba_side,imba_entry_price,imba_exit_price,imba_trade_pnl,imba_balance,\
             liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance,\
             trades_per_second,price_velocity,poly_liquidity_delta,absorption_ratio,\
             price_gap_ratio,spoofing_flag,tape_speed_flag,gap_alert_flag,\
             bollinger_sma,bollinger_upper,bollinger_lower,mean_reversion_signal,\
             technical_confluence,trend_direction,signal_label,\
             realized_volatility,high_volatility_event,bollinger_position,master_signal,\
                              cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor,t5_prediction,t5_entry_price,t5_correct,t3_prediction,t3_entry_price,t3_active,pnr_active,pnr_seconds_left,pnr_price,pnr_return_up,pnr_return_down,pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct"
        );

        let mut writers = self.writers.lock().unwrap();
        // If a writer already exists for this session_id, flush+close the old one first
        if let Some(old) = writers.remove(&session_id) {
            info!("SessionManager: replacing existing writer for session #{}", session_id);
            drop(old); // triggers BufWriter flush + file close
        }

        writers.insert(session_id, SessionWriter {
            writer,
            path: path.clone(),
            tick_count: 0,
            trade_count: 0,
            row_count: 0,
        });

        info!("SessionManager: started session #{} → {}", session_id, path);
        Ok(())
    }

    /// Recover a session after crash — opens in APPEND mode, preserves existing data.
    /// Only writes header if the file is empty.
    pub fn recover_session(&self, session_id: i32) -> Result<(), String> {
        let path = format!("{}/session_{:04}_hft.csv", self.data_dir, session_id);
        let file_exists = std::path::Path::new(&path).exists();

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)  // preserve existing data from before crash
            .open(&path)
            .map_err(|e| format!("SessionManager: cannot recover {}: {}", path, e))?;

        let mut writer = BufWriter::with_capacity(10_485_760, file); // 10 MiB

        // Write header only if file is new or empty
        if !file_exists {
            let _ = writeln!(
                writer,
                "ts_local,ts_exchange,event_type,latencia_ms,binance_price,binance_micro_price,\
                 binance_imbalance,binance_vol_100ms,binance_vol_24h,poly_bid,poly_ask,poly_mid,\
                 poly_spread,poly_bid_vol_all,poly_ask_vol_all,poly_imbalance,trade_side,\
                 trade_price,trade_size,is_informed,\
                 imba_status,imba_side,imba_entry_price,imba_exit_price,imba_trade_pnl,imba_balance,\
                 liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance,\
                 trades_per_second,price_velocity,poly_liquidity_delta,absorption_ratio,\
                 price_gap_ratio,spoofing_flag,tape_speed_flag,gap_alert_flag,\
                 bollinger_sma,bollinger_upper,bollinger_lower,mean_reversion_signal,\
                 technical_confluence,trend_direction,signal_label,\
                 realized_volatility,high_volatility_event,bollinger_position,master_signal,\
                     cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor,t5_prediction,t5_entry_price,t5_correct,t3_prediction,t3_entry_price,t3_active,pnr_active,pnr_seconds_left,pnr_price,pnr_return_up,pnr_return_down,pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct"
            );
        }

        let mut writers = self.writers.lock().unwrap();
        writers.insert(session_id, SessionWriter {
            writer,
            path: path.clone(),
            tick_count: 0,
            trade_count: 0,
            row_count: 0,
        });

        info!("SessionManager: RECOVERED session #{} (append mode) → {}", session_id, path);
        Ok(())
    }

    /// Push a CsvRecord to the session specified by record.session_id.
    /// Pre-formats the CSV line OUTSIDE the writer lock to minimise mutex hold time.
    /// Data stays in BufWriter(2 MiB) + OS page cache — flushed only at session end.
    pub fn push(&self, record: &CsvRecord) -> bool {
        let sid = record.session_id;
        if sid == 0 { return false; }

        // Pre-format the CSV line outside the lock — avoids holding the mutex
        // while formatting 60 float/string fields.
        let line = fast_format_csv_line(record);
        let is_trade = matches!(record.event_type, EventType::Trade);

        let mut writers = self.writers.lock().unwrap();
        let sw = match writers.get_mut(&sid) {
            Some(w) => w,
            None => return false,
        };

        // Write pre-formatted line + newline (single write! call = single buffer copy)
        let _ = sw.writer.write_all(line.as_bytes());
        let _ = sw.writer.write_all(b"\n");

        if is_trade {
            sw.trade_count += 1;
        } else {
            sw.tick_count += 1;
        }

        sw.row_count += 1;
        // NO mid-session flush — data stays in BufWriter + OS page cache.
        // Flushed only at stop_session() / Drop / SIGINT.
        true
    }

    /// Flush and close a specific session's writer.
    pub fn stop_session(&self, session_id: i32) -> Result<(), String> {
        let mut writers = self.writers.lock().unwrap();
        if let Some(sw) = writers.remove(&session_id) {
            info!("SessionManager: stopped session #{} ({} ticks, {} trades, {} rows)",
                session_id, sw.tick_count, sw.trade_count, sw.row_count);
            // Explicit flush via drop
            drop(sw);
            Ok(())
        } else {
            warn!("SessionManager: stop_session #{} — no writer found", session_id);
            Ok(())
        }
    }

    /// Flush a specific session's writer without closing.
    pub fn flush(&self, session_id: i32) -> Result<(), String> {
        let mut writers = self.writers.lock().unwrap();
        if let Some(sw) = writers.get_mut(&session_id) {
            sw.writer.flush()
                .map_err(|e| format!("SessionManager flush #{}: {}", session_id, e))?;
        }
        Ok(())
    }

    /// Flush all active writers (called periodically).
    pub fn flush_all(&self) {
        let mut writers = self.writers.lock().unwrap();
        for (&sid, sw) in writers.iter_mut() {
            if let Err(e) = sw.writer.flush() {
                warn!("SessionManager flush_all #{}: {}", sid, e);
            }
        }
    }

    /// List of currently active session IDs.
    pub fn active_ids(&self) -> Vec<i32> {
        self.writers.lock().unwrap().keys().copied().collect()
    }

    pub fn is_idle(&self) -> bool {
        self.writers.lock().unwrap().is_empty()
    }

    pub fn tick_count(&self, session_id: i32) -> u64 {
        self.writers.lock().unwrap()
            .get(&session_id)
            .map(|sw| sw.tick_count)
            .unwrap_or(0)
    }

    pub fn trade_count(&self, session_id: i32) -> u64 {
        self.writers.lock().unwrap()
            .get(&session_id)
            .map(|sw| sw.trade_count)
            .unwrap_or(0)
    }

    pub fn session_path(&self, session_id: i32) -> String {
        format!("{}/session_{:04}_hft.csv", self.data_dir, session_id)
    }

    pub fn current_path(&self, session_id: i32) -> Option<String> {
        self.writers.lock().unwrap()
            .get(&session_id)
            .map(|sw| sw.path.clone())
    }
}

/// Guaranteed flush on drop — prevents data loss on process termination.
impl Drop for SessionManager {
    fn drop(&mut self) {
        if let Ok(mut writers) = self.writers.lock() {
            for (&sid, sw) in writers.iter_mut() {
                let _ = sw.writer.flush();
                info!("SessionManager::drop flushed session #{}", sid);
            }
        }
    }
}
