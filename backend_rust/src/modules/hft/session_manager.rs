use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

/// Write column descriptions as # comment lines before the header row.
/// Write column descriptions as # comment lines before the header row.
fn write_column_metadata(w: &mut BufWriter<File>) {
    let _ = writeln!(w, "# build_version={} built@{}", env!("GIT_VERSION"), env!("BUILD_TIME"));
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ============================================================");
    let _ = writeln!(w, "# ZZIGNAL CSV — Odiseo 83 HFT Bot — 49-column Analysis Guide");
    let _ = writeln!(w, "# ============================================================");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# STRATEGY: Odiseo 83 (entry >= 0.83, TP=0.97, SL-hard=0.81)");
    let _ = writeln!(w, "# TRIGGER: clob_trade_up >= 0.83 (UP) | clob_trade_dn >= 0.83 (DOWN)");
    let _ = writeln!(w, "# EXITS: TP=0.97 | SL-micro (vol>30% drop+imb<-0.5) | SL-trend (price>3c drop+vel<0) | SL-hard=0.81");
    let _ = writeln!(w, "# SESSION: 15-min Polymarket | Boundary: no entries first 60s + last 45s (flash exit reason=6)");
    let _ = writeln!(w, "# BUDGET: $20 default | Profit stop 15% | Max SL/session: 4 | Reinvest ON");
    let _ = writeln!(w, "# TIMEZONE: UTC-5 (Lima/Bogota)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# QUICK START:");
    let _ = writeln!(w, "#   1. Find entries: rows WHERE od83_event = IN_UP OR od83_event = IN_DN");
    let _ = writeln!(w, "#   2. Find exits:  rows WHERE od83_event = OUT_UP OR od83_event = OUT_DN");
    let _ = writeln!(w, "#   3. PnL per trade: od83_up_pnl at exit row");
    let _ = writeln!(w, "#   4. Win rate: count(od83_up_pnl > 0 at exit) / total exits");
    let _ = writeln!(w, "#   5. Flash dumps: rows WHERE dump_score >= 2 OR tick_gap_ms > 1000");
    let _ = writeln!(w, "#   6. Reversals: rows WHERE reversal_score >= 2");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# STATUS: 0=idle/blocked  1=WATCHING  2=ACTIVE");
    let _ = writeln!(w, "# EXIT:   0=active  1=TP  2=SL-micro  3=SL-trend  4=SL-hard  5=settle  6=flash_protect");
    let _ = writeln!(w, "# RISK:   dump_score 0-3 | reversal_score 0-3 | tick_gap_ms >2000=frozen");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 0: IDENTIDAD (cols 1-4)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#    1 | time                | timestamp   | Local time UTC-5 with ms");
    let _ = writeln!(w, "#    2 | ts_exchange         | int64       | Binance exchange timestamp (unix ms)");
    let _ = writeln!(w, "#    3 | event               | string      | BOOK_UPDATE | BINANCE_TICK | TRADE");
    let _ = writeln!(w, "#    4 | latencia_ms         | int         | Cross-exchange latency Binance→Poly (ms)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 1: MERCADO BTC (cols 5-10)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#    5 | binance_price       | float       | BTC mid price from Binance WS");
    let _ = writeln!(w, "#    6 | binance_imbalance   | float       | Binance depth imbalance ratio");
    let _ = writeln!(w, "#    7 | binance_vol_24h     | float       | BTC 24h rolling volume");
    let _ = writeln!(w, "#    8 | btc_vol             | float       | Real-time BTC volume (aggTrade field q)");
    let _ = writeln!(w, "#    9 | btc_vel             | float       | BTC price velocity USD/s → SL-trend trigger");
    let _ = writeln!(w, "#   10 | btc_acel            | float       | BTC price acceleration USD/s² → momentum filter");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 2: ORDER BOOK (cols 11-17)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   11 | bid                 | float       | Polymarket CLOB best bid");
    let _ = writeln!(w, "#   12 | ask                 | float       | Polymarket CLOB best ask");
    let _ = writeln!(w, "#   13 | mid                 | float       | Polymarket mid = (bid+ask)/2 → PnL calc");
    let _ = writeln!(w, "#   14 | spread              | float       | ask - bid → spread_health filter");
    let _ = writeln!(w, "#   15 | bid_vol             | float       | Total bid volume → min_volume filter + SL-micro");
    let _ = writeln!(w, "#   16 | ask_vol             | float       | Total ask volume → min_volume filter + SL-micro");
    let _ = writeln!(w, "#   17 | imbalance           | float       | bid_vol/(bid_vol+ask_vol) → SL-micro check");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 3: RIESGO (cols 18-23)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   18 | spoof               | int         | 1 = >50% vol drop without trade");
    let _ = writeln!(w, "#   19 | tick_gap_ms         | int         | ms since last tick (>2000 = frozen_market filter)");
    let _ = writeln!(w, "#   20 | ask_wall            | int         | 1 = ask_vol > 3x bid_vol → ask_wall filter");
    let _ = writeln!(w, "#   21 | dump_score          | int         | 0=safe 1=warn 2=critical 3=dead → flash_dump filter");
    let _ = writeln!(w, "#   22 | reversal_score      | int         | 0=safe 1=alert 2=danger 3=exit → reversal_risk filter");
    let _ = writeln!(w, "#   23 | secs_left           | int         | Seconds until session close → boundary protection");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 4: MICRO (col 24)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   24 | poly_liquidity_delta | float       | Δask_vol vs prev tick → dump filter candidate");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 5: ★★ TRIGGER CLOB (cols 25-29)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   25 | clob_trade_up       | float       | Last CLOB trade price UP ★ ENTRY TRIGGER >=0.83");
    let _ = writeln!(w, "#   26 | clob_trade_dn       | float       | Last CLOB trade price DOWN ★ ENTRY TRIGGER >=0.83");
    let _ = writeln!(w, "#   27 | clob_trade_up_vol   | float       | Volume of last CLOB trade UP");
    let _ = writeln!(w, "#   28 | clob_trade_dn_vol   | float       | Volume of last CLOB trade DOWN");
    let _ = writeln!(w, "#   29 | od83_filters        | int         | Filter bitmask: 0=all_off 255=all_on (8 bits=8 filters)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 6: ★ ODISEO UP (cols 30-36)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   30 | od83_up             | int         | 0=idle 1=WATCH 2=ACTIVE");
    let _ = writeln!(w, "#   31 | od83_up_entry       | float       | UP entry fill price");
    let _ = writeln!(w, "#   32 | od83_up_sz          | float       | UP contracts bought (budget/entry_price)");
    let _ = writeln!(w, "#   33 | od83_up_pnl         | float       | UP paper PnL = (mid-entry)*size");
    let _ = writeln!(w, "#   34 | od83_up_exit        | float       | UP exit price (0=still active)");
    let _ = writeln!(w, "#   35 | od83_up_r           | int         | UP exit reason 0-6");
    let _ = writeln!(w, "#   36 | od83_up_bal         | float       | UP balance = budget + cumulative PnL");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 7: ★ ODISEO DOWN (cols 37-43)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   37 | od83_dn             | int         | DOWN status (same codes as UP)");
    let _ = writeln!(w, "#   38 | od83_dn_entry       | float       | DOWN entry fill price");
    let _ = writeln!(w, "#   39 | od83_dn_sz          | float       | DOWN contracts sold");
    let _ = writeln!(w, "#   40 | od83_dn_pnl         | float       | DOWN paper PnL");
    let _ = writeln!(w, "#   41 | od83_dn_exit        | float       | DOWN exit price");
    let _ = writeln!(w, "#   42 | od83_dn_r           | int         | DOWN exit reason 0-6");
    let _ = writeln!(w, "#   43 | od83_dn_bal         | float       | DOWN balance");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 8: EVENTOS + MODE (cols 44-46)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   44 | od83_event          | string      | IN_UP|IN_DN|OUT_UP|OUT_DN → entry/exit markers");
    let _ = writeln!(w, "#   45 | od83_up_mode        | string      | PAPER | LIVE");
    let _ = writeln!(w, "#   46 | od83_dn_mode        | string      | PAPER | LIVE");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 9: TIMING (cols 47-48)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   47 | od83_up_at          | timestamp   | UP entry timestamp HH:MM:SS.mmm UTC-5");
    let _ = writeln!(w, "#   48 | od83_dn_at          | timestamp   | DOWN entry timestamp HH:MM:SS.mmm UTC-5");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# FASE 10: POSICIÓN (col 49)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "#   49 | mid_from_entry      | float       | Distance from entry: mid - entry_price");
}
/// Uses a single pre-allocated String with write! to avoid per-field allocation.
/// Called outside the writer lock — only the final `write_all` is inside the mutex.
#[inline]
/// Delegates to `CsvRecord::to_csv_line()` (single source of truth in types.rs).
#[inline]
fn fast_format_csv_line(r: &CsvRecord) -> String {
    r.to_csv_line()
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

        let mut writer = BufWriter::with_capacity(52_428_800, file); // 50 MiB

        // Write column metadata
        write_column_metadata(&mut writer);

        // Write column header
        let _ = writeln!(writer, "{}", CsvRecord::csv_header());

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

        let mut writer = BufWriter::with_capacity(52_428_800, file); // 50 MiB

        // Write header only if file is new or empty
        if !file_exists {
            let _ = writeln!(writer, "{}", CsvRecord::csv_header());
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
    /// Data stays in BufWriter(50 MiB) + OS page cache — flush every 100 rows + 15s background.
    pub fn push(&self, record: &CsvRecord) -> bool {
        let sid = record.session_id;
        if sid == 0 {
            warn!("CSV push: sid=0, skipping");
            return false;
        }

        // Pre-format the CSV line outside the lock
        let line = fast_format_csv_line(record);
        if line.is_empty() {
            warn!("CSV push: fast_format_csv_line returned empty for sid={}", sid);
            return false;
        }
        let is_trade = matches!(record.event_type, EventType::Trade);

        let mut writers = self.writers.lock().unwrap();
        let sw = match writers.get_mut(&sid) {
            Some(w) => w,
            None => {
                warn!("CSV push: no writer for sid={} (active: {:?})", sid,
                    writers.keys().collect::<Vec<_>>());
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
            info!("CSV flush: sid={} rows={} ticks={} trades={}",
                sid, sw.row_count, sw.tick_count, sw.trade_count);
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
