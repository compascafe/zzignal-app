use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

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

        let mut writer = BufWriter::with_capacity(65536, file);

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
             realized_volatility,high_volatility_event,bollinger_position,master_signal"
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

        let mut writer = BufWriter::with_capacity(65536, file);

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
                 realized_volatility,high_volatility_event,bollinger_position,master_signal"
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
    pub fn push(&self, record: &CsvRecord) -> bool {
        let sid = record.session_id;
        if sid == 0 { return false; }

        let mut writers = self.writers.lock().unwrap();
        let sw = match writers.get_mut(&sid) {
            Some(w) => w,
            None => return false,
        };

        let _ = writeln!(
            sw.writer,
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
            record.trades_per_second,
            record.price_velocity,
            record.poly_liquidity_delta,
            record.absorption_ratio,
            record.price_gap_ratio,
            record.spoofing_flag,
            record.tape_speed_flag,
            record.gap_alert_flag,
            record.bollinger_sma,
            record.bollinger_upper,
            record.bollinger_lower,
            record.mean_reversion_signal,
            record.technical_confluence,
            record.trend_direction,
            record.signal_label,
            record.realized_volatility,
            record.high_volatility_event,
            record.bollinger_position,
            record.master_signal,
        );

        match record.event_type {
            EventType::Trade => sw.trade_count += 1,
            _ => sw.tick_count += 1,
        }

        sw.row_count += 1;
        if sw.row_count % 10 == 0 {
            let _ = sw.writer.flush();
        }
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
