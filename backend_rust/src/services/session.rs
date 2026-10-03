use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::models::hft::{CsvRecord, EventType};

struct SessionWriter {
    writer: BufWriter<File>,
    tick_count: u64,
    trade_count: u64,
    row_count: u64,
}

/// Manages per-session CSV file isolation with MULTIPLE concurrent writers.
/// Each session gets its own file: `{data_dir}/session_{id:04}_{name}_hft.csv`.
/// Sessions are fully isolated — writing to one never closes another.
pub struct SessionManager {
    writers: Mutex<HashMap<i32, SessionWriter>>,
    data_dir: String,
    names: Mutex<HashMap<i32, String>>,
}

impl SessionManager {
    pub fn new(data_dir: &str) -> Self {
        std::fs::create_dir_all(data_dir).ok();
        Self {
            writers: Mutex::new(HashMap::new()),
            data_dir: data_dir.to_string(),
            names: Mutex::new(HashMap::new()),
        }
    }

    /// Start a new session writer. Does NOT close other sessions' writers.
    pub fn start_session(&self, session_id: i32, name: &str) -> Result<(), String> {
        let path = if name.is_empty() {
            format!("{}/session_{:04}_hft.csv", self.data_dir, session_id)
        } else {
            format!(
                "{}/session_{:04}_{}_hft.csv",
                self.data_dir, session_id, name
            )
        };
        self.names
            .lock()
            .unwrap()
            .insert(session_id, name.to_string());
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("SessionManager: cannot create {}: {}", path, e))?;

        let mut writer = BufWriter::with_capacity(52_428_800, file); // 50 MiB

        // Write column header only (no metadata — fast session start)
        let _ = writeln!(writer, "{}", CsvRecord::csv_header());

        let mut writers = self.writers.lock().unwrap();
        // If a writer already exists for this session_id, flush+close the old one first
        if let Some(old) = writers.remove(&session_id) {
            info!(
                "SessionManager: replacing existing writer for session #{}",
                session_id
            );
            drop(old); // triggers BufWriter flush + file close
        }

        writers.insert(
            session_id,
            SessionWriter {
                writer,
                tick_count: 0,
                trade_count: 0,
                row_count: 0,
            },
        );

        info!("SessionManager: started session #{} → {}", session_id, path);
        Ok(())
    }

    /// Push a CsvRecord to the session specified by record.session_id.
    /// Pre-formats the CSV line OUTSIDE the writer lock to minimise mutex hold time.
    /// Data stays in BufWriter(50 MiB) + OS page cache — flush every 100 rows + 15s background.
    pub fn push(&self, record: &CsvRecord) -> bool {
        let sid = record.session_id;
        if sid == 0 {
            // No recording session active — nothing to persist.
            return false;
        }

        // Pre-format the CSV line outside the lock
        let line = record.to_csv_line();
        if line.is_empty() {
            warn!(
                "CSV push: fast_format_csv_line returned empty for sid={}",
                sid
            );
            return false;
        }
        let is_trade = matches!(record.event_type, EventType::Trade);

        let mut writers = self.writers.lock().unwrap();
        let sw = match writers.get_mut(&sid) {
            Some(w) => w,
            None => {
                warn!(
                    "CSV push: no writer for sid={} (active: {:?})",
                    sid,
                    writers.keys().collect::<Vec<_>>()
                );
                return false;
            }
        };

        let _ = sw.writer.write_all(line.as_bytes());
        let _ = sw.writer.write_all(b"\n");

        if is_trade {
            sw.trade_count += 1;
        } else {
            sw.tick_count += 1;
        }

        sw.row_count += 1;
        if sw.row_count % 100 == 0 {
            let _ = sw.writer.flush();
            info!(
                "CSV flush: sid={} rows={} ticks={} trades={}",
                sid, sw.row_count, sw.tick_count, sw.trade_count
            );
        }
        true
    }

    /// Flush and close a specific session's writer.
    pub fn stop_session(&self, session_id: i32) -> Result<(), String> {
        let sw = {
            let mut writers = self.writers.lock().unwrap();
            writers.remove(&session_id)
        };
        if let Some(sw) = sw {
            info!(
                "SessionManager: stopped session #{} ({} ticks, {} trades, {} rows)",
                session_id, sw.tick_count, sw.trade_count, sw.row_count
            );
            drop(sw); // flush + close outside lock
            Ok(())
        } else {
            warn!(
                "SessionManager: stop_session #{} — no writer found",
                session_id
            );
            Ok(())
        }
    }

    /// Flush a specific session's writer without closing.
    pub fn flush(&self, session_id: i32) -> Result<(), String> {
        let mut writers = self.writers.lock().unwrap();
        if let Some(sw) = writers.get_mut(&session_id) {
            sw.writer
                .flush()
                .map_err(|e| format!("SessionManager flush #{}: {}", session_id, e))?;
        }
        Ok(())
    }

    /// Flush all active writers (called periodically).
    pub fn flush_all(&self) {
        let ids: Vec<i32> = self.writers.lock().unwrap().keys().copied().collect();
        for id in ids {
            let mut writers = self.writers.lock().unwrap();
            if let Some(sw) = writers.get_mut(&id) {
                let _ = sw.writer.flush();
            }
        }
    }

    /// List of currently active session IDs.
    pub fn active_ids(&self) -> Vec<i32> {
        self.writers.lock().unwrap().keys().copied().collect()
    }

    pub fn tick_count(&self, session_id: i32) -> u64 {
        self.writers
            .lock()
            .unwrap()
            .get(&session_id)
            .map(|sw| sw.tick_count)
            .unwrap_or(0)
    }

    pub fn trade_count(&self, session_id: i32) -> u64 {
        self.writers
            .lock()
            .unwrap()
            .get(&session_id)
            .map(|sw| sw.trade_count)
            .unwrap_or(0)
    }

    pub fn session_path(&self, session_id: i32) -> String {
        let name = self
            .names
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        if name.is_empty() {
            format!("{}/session_{:04}_hft.csv", self.data_dir, session_id)
        } else {
            format!(
                "{}/session_{:04}_{}_hft.csv",
                self.data_dir, session_id, name
            )
        }
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
