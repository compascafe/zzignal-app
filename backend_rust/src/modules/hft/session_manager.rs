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
    let _ = writeln!(w, "# ZZIGNAL CSV — Odiseo 83 HFT Bot — LLM Analysis Guide");
    let _ = writeln!(w, "# ============================================================");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# STRATEGY: Odiseo 83 (entry >= 0.83, TP=0.97, SL-hard=0.81)");
    let _ = writeln!(w, "# TRIGGER: clob_trade_up >= 0.83 (UP) | clob_trade_dn >= 0.83 (DOWN)");
    let _ = writeln!(w, "# EXITS: TP=0.97 | SL-micro (vol>30% drop+imb<-0.5) | SL-trend (price>3c drop+vel<0) | SL-hard=0.81");
    let _ = writeln!(w, "# SESSION: 15-min Polymarket | Boundary: no entries first 60s + last 45s (flash exit reason=6)");
    let _ = writeln!(w, "# BUDGET: $20 default | Profit stop 15% | Max SL/session: 4 | Reinvest ON (never decreases)");
    let _ = writeln!(w, "# TIMEZONE: UTC-5 (Lima/Bogota)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# QUICK START FOR LLMs:");
    let _ = writeln!(w, "#   1. Find entries: filter rows WHERE od83_event = IN_UP OR od83_event = IN_DN");
    let _ = writeln!(w, "#   2. Find exits:  filter rows WHERE od83_event = OUT_UP OR od83_event = OUT_DN");
    let _ = writeln!(w, "#   3. PnL per trade: od83_up_pnl at exit row (where od83_event contains OUT)");
    let _ = writeln!(w, "#   4. Win rate: count(exit rows WHERE od83_up_pnl > 0) / count(all exit rows)");
    let _ = writeln!(w, "#   5. Flash dumps: rows WHERE dump_score >= 2 OR tick_gap_ms > 1000");
    let _ = writeln!(w, "#   6. Reversals: rows WHERE reversal_score >= 2");
    let _ = writeln!(w, "#   7. LIVE vs PAPER: compare od83_up_pnl (paper) vs live_up (real fills)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# STATUS CODES (od83_up, od83_dn):");
    let _ = writeln!(w, "#   0 = idle/blocked | 1 = WATCHING | 2 = ACTIVE (in position)");
    let _ = writeln!(w, "# EXIT REASONS (od83_up_r, od83_dn_r):");
    let _ = writeln!(w, "#   0=active 1=TP 2=SL-micro 3=SL-trend 4=SL-hard 5=session_settle 6=flash_protect");
    let _ = writeln!(w, "# RISK SCORES:");
    let _ = writeln!(w, "#   dump_score: 0=safe 1=warning 2=critical 3=dead(bid=0 or tick_gap>2000)");
    let _ = writeln!(w, "#   reversal_score: 0=safe 1=alert 2=danger 3=exit_now");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# COL | NAME               | TYPE        | DESCRIPTION");
    let _ = writeln!(w, "# ----|--------------------|-------------|------------------------------------------");
    let _ = writeln!(w, "#    1 | time                | timestamp   | Local time UTC-5 with ms");
    let _ = writeln!(w, "#    2 | ts_exchange         | int64       | Binance exchange timestamp (unix ms)");
    let _ = writeln!(w, "#    3 | event               | string      | BOOK_UPDATE | TRADE | BINANCE_TICK");
    let _ = writeln!(w, "#    4 | latencia_ms         | int         | Cross-exchange latency ms");
    let _ = writeln!(w, "#    5 | binance_price       | float       | BTC mid price (bid+ask)/2 — Binance websocket");
    let _ = writeln!(w, "#    6 | binance_micro_price | float       | Volume-weighted micro price");
    let _ = writeln!(w, "#    7 | binance_imbalance   | float       | Depth imbalance ratio");
    let _ = writeln!(w, "#    8 | binance_vol_100ms   | float       | BTC volume last 100ms");
    let _ = writeln!(w, "#    9 | binance_vol_24h     | float       | BTC 24h volume");
    let _ = writeln!(w, "#   10 | btc_vol             | float       | REAL-TIME BTC volume from Binance aggTrade (field q)");
    let _ = writeln!(w, "#   11 | bid                 | float       | Polymarket CLOB best bid");
    let _ = writeln!(w, "#   12 | ask                 | float       | Polymarket CLOB best ask");
    let _ = writeln!(w, "#   13 | mid                 | float       | Polymarket mid price (bid+ask)/2");
    let _ = writeln!(w, "#   14 | spread              | float       | ask - bid (market health indicator)");
    let _ = writeln!(w, "#   15 | bid_vol             | float       | Total CLOB bid volume all levels");
    let _ = writeln!(w, "#   16 | ask_vol             | float       | Total CLOB ask volume all levels");
    let _ = writeln!(w, "#   17 | imbalance           | float       | CLOB imbalance = bid_vol/(bid_vol+ask_vol)");
    let _ = writeln!(w, "#   18 | trades_per_second   | float       | Binance trade rate");
    let _ = writeln!(w, "#   19 | btc_vel             | float       | BTC price velocity USD/s");
    let _ = writeln!(w, "#   20 | poly_liquidity_delta | float       | Delta ask volume vs prev tick");
    let _ = writeln!(w, "#   21 | absorption_ratio    | float       | Absorption ratio");
    let _ = writeln!(w, "#   22 | price_gap_ratio     | float       | % divergence Binance vs Polymarket");
    let _ = writeln!(w, "#   23 | spoof               | int         | 1 = >50% vol drop without trade");
    let _ = writeln!(w, "#   24 | tape                | int         | 1 = volume spike detected");
    let _ = writeln!(w, "#   25 | gap                 | int         | 1 = price gap >0.5%");
    let _ = writeln!(w, "#   26 | pnr_active          | int         | 1 when secs_left <= 300");
    let _ = writeln!(w, "#   27 | secs_left           | int         | Seconds until session close (900→0)");
    let _ = writeln!(w, "#   28 | pnr_price           | float       | poly_mid at PNR capture");
    let _ = writeln!(w, "#   29 | pnr_return_up       | float       | 1.0 - ask (expected UP return)");
    let _ = writeln!(w, "#   30 | pnr_return_down     | float       | bid - 0.0 (expected DOWN return)");
    let _ = writeln!(w, "#   31 | pnr_volatility_1m   | float       | Liquidity delta volatility proxy");
    let _ = writeln!(w, "#   32 | pnr_confidence      | float       | |mid-0.5| * 2 (0-1)");
    let _ = writeln!(w, "#   33 | pnr_trend           | int         | +1 UP -1 DOWN 0 flat");
    let _ = writeln!(w, "#   34 | pnr_spread_pct      | float       | spread / mid ratio");
    let _ = writeln!(w, "#   35 | p_bid_lo            | float       | Pressure: lowest bid with vol>=10");
    let _ = writeln!(w, "#   36 | p_ask_hi            | float       | Pressure: highest ask with vol>=10");
    let _ = writeln!(w, "#   37 | p_band              | float       | Pressure: ask_ceiling - bid_floor");
    let _ = writeln!(w, "#   38 | p_index             | float       | Pressure: (mid-floor)/band (0=DOWN 1=UP)");
    let _ = writeln!(w, "#   39 | p_skew              | float       | Pressure: (bid_vol-ask_vol)/total in band");
    let _ = writeln!(w, "#   40 | od83_up             | int         | OD83 UP status: 0=idle/blocked 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "#   41 | od83_up_entry       | float       | OD83 UP entry fill price");
    let _ = writeln!(w, "#   42 | od83_up_sz          | float       | OD83 UP contracts bought");
    let _ = writeln!(w, "#   43 | od83_up_pnl         | float       | OD83 UP paper PnL ((mid-entry)*size)");
    let _ = writeln!(w, "#   44 | od83_up_exit        | float       | OD83 UP exit price (0=still active)");
    let _ = writeln!(w, "#   45 | od83_up_r           | int         | OD83 UP exit reason 0-6");
    let _ = writeln!(w, "#   46 | od83_up_bal         | float       | OD83 UP balance (budget+cumulative PnL)");
    let _ = writeln!(w, "#   47 | od83_dn             | int         | OD83 DOWN status: 0=idle/blocked 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "#   48 | od83_dn_entry       | float       | OD83 DOWN entry fill price");
    let _ = writeln!(w, "#   49 | od83_dn_sz          | float       | OD83 DOWN contracts sold");
    let _ = writeln!(w, "#   50 | od83_dn_pnl         | float       | OD83 DOWN paper PnL");
    let _ = writeln!(w, "#   51 | od83_dn_exit        | float       | OD83 DOWN exit price");
    let _ = writeln!(w, "#   52 | od83_dn_r           | int         | OD83 DOWN exit reason 0-6");
    let _ = writeln!(w, "#   53 | od83_dn_bal         | float       | OD83 DOWN balance (budget+cumulative PnL)");
    let _ = writeln!(w, "#   54 | live_up             | float       | Real UP PnL from Polymarket fills");
    let _ = writeln!(w, "#   55 | live_dn             | float       | Real DOWN PnL from Polymarket fills");
    let _ = writeln!(w, "#   56 | live_bal            | float       | Real USDC balance from Polymarket API");
    let _ = writeln!(w, "#   57 | od_signal           | int         | Odiseo entry signal: 0=none 1=UP_enter 2=DOWN_enter 3=both");
    let _ = writeln!(w, "#   58 | clob_trade_up       | float       | Last CLOB trade price UP token (TRIGGER for Odiseo UP)");
    let _ = writeln!(w, "#   59 | clob_trade_dn       | float       | Last CLOB trade price DOWN token (TRIGGER for Odiseo DOWN)");
    let _ = writeln!(w, "#   60 | clob_trade_up_vol   | float       | Volume of last CLOB trade UP");
    let _ = writeln!(w, "#   61 | clob_trade_dn_vol   | float       | Volume of last CLOB trade DOWN");
    let _ = writeln!(w, "#   62 | clob_trade_up_ts    | timestamp   | Timestamp last CLOB trade UP");
    let _ = writeln!(w, "#   63 | clob_trade_dn_ts    | timestamp   | Timestamp last CLOB trade DOWN");
    let _ = writeln!(w, "#   64 | od83_event          | string      | IN_UP|IN_DN|OUT_UP|OUT_DN|empty — Entry/exit markers for LLMs");
    let _ = writeln!(w, "#   65 | od83_up_mode        | string      | UP trade mode: PAPER or LIVE");
    let _ = writeln!(w, "#   66 | od83_dn_mode        | string      | DOWN trade mode: PAPER or LIVE");
    let _ = writeln!(w, "#   67 | od83_up_at          | timestamp   | UP entry timestamp HH:MM:SS.mmm UTC-5");
    let _ = writeln!(w, "#   68 | od83_dn_at          | timestamp   | DOWN entry timestamp HH:MM:SS.mmm UTC-5");
    let _ = writeln!(w, "#   69 | od83_up_fill_ms     | int         | UP fill latency ms (0=PAPER, >0=LIVE)");
    let _ = writeln!(w, "#   70 | od83_dn_fill_ms     | int         | DOWN fill latency ms (0=PAPER, >0=LIVE)");
    let _ = writeln!(w, "#   71 | tick_gap_ms         | int         | ms since last tick (>2000 = market frozen)");
    let _ = writeln!(w, "#   72 | bid_drain           | float       | % bid volume lost vs 5 ticks ago (<0 = whales leaving)");
    let _ = writeln!(w, "#   73 | ask_wall            | int         | 1 if ask_vol > 3x bid_vol (one-sided imminent)");
    let _ = writeln!(w, "#   74 | dump_score          | int         | Flash dump risk: 0=safe 1=warning 2=critical 3=dead");
    let _ = writeln!(w, "#   75 | btc_delta           | float       | BTC change (USD) since Odiseo 83 entered position");
    let _ = writeln!(w, "#   76 | mid_from_entry      | float       | Distance from entry: mid - entry_price");
    let _ = writeln!(w, "#   77 | adverse_ticks       | int         | Consecutive ticks moving against position");
    let _ = writeln!(w, "#   78 | vol_bleed           | float       | % volume lost in position's market side");
    let _ = writeln!(w, "#   79 | reversal_score      | int         | Reversal risk: 0=safe 1=alert 2=danger 3=exit");
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
