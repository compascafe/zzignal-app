use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

/// Write column descriptions as # comment lines before the header row.
fn write_column_metadata(w: &mut BufWriter<File>) {
    let _ = writeln!(w, "# build_version={} built@{}", env!("GIT_VERSION"), env!("BUILD_TIME"));
    let _ = writeln!(w, "# ─── Column Reference (114 columns) ─────────────────────────────────────");
    let _ = writeln!(w, "# [1]  ts_local            = Local timestamp (ISO 8601)");
    let _ = writeln!(w, "# [2]  ts_exchange         = Binance exchange event_time (unix ms)");
    let _ = writeln!(w, "# [3]  event_type          = BOOK_UPDATE | TRADE | BINANCE_TICK");
    let _ = writeln!(w, "# [4]  latencia_ms         = Cross-exchange latency (poly_ts - binance_ts)");
    let _ = writeln!(w, "# [5]  binance_price       = Binance mid price (bid+ask)/2");
    let _ = writeln!(w, "# [6]  binance_micro_price = Volume-weighted micro price");
    let _ = writeln!(w, "# [7]  binance_imbalance   = Depth imbalance: (bid_vol - ask_vol)/total");
    let _ = writeln!(w, "# [8]  binance_vol_100ms   = Volume in last 100ms");
    let _ = writeln!(w, "# [9]  binance_vol_24h     = 24h BTC volume");
    let _ = writeln!(w, "# [10] poly_bid            = Polymarket best bid");
    let _ = writeln!(w, "# [11] poly_ask            = Polymarket best ask");
    let _ = writeln!(w, "# [12] poly_mid            = Polymarket mid price (one-sided fallback)");
    let _ = writeln!(w, "# [13] poly_spread         = ask - bid spread");
    let _ = writeln!(w, "# [14] poly_bid_vol_all    = Total bid volume (all levels)");
    let _ = writeln!(w, "# [15] poly_ask_vol_all    = Total ask volume (all levels)");
    let _ = writeln!(w, "# [16] poly_imbalance      = Order book imbalance ratio");
    let _ = writeln!(w, "# [17] trade_side          = BUY | SELL (TRADE events only)");
    let _ = writeln!(w, "# [18] trade_price         = Trade price (TRADE events only)");
    let _ = writeln!(w, "# [19] trade_size          = Trade size in contracts");
    let _ = writeln!(w, "# [20] is_informed         = 1 if big move (>$1) or volume spike in 500ms");
    let _ = writeln!(w, "# [21] imba_status         = Imbalance strategy status (IDLE|OPEN|CLOSED)");
    let _ = writeln!(w, "# [22] imba_side           = Imbalance strategy trade side");
    let _ = writeln!(w, "# [23] imba_entry_price    = Imbalance entry price");
    let _ = writeln!(w, "# [24] imba_exit_price     = Imbalance exit price");
    let _ = writeln!(w, "# [25] imba_trade_pnl      = Imbalance PnL");
    let _ = writeln!(w, "# [26] imba_balance        = Imbalance virtual balance");
    let _ = writeln!(w, "# [27] liqb_status         = Liquidity strategy status (IDLE|OPEN|CLOSED)");
    let _ = writeln!(w, "# [28] liqb_side           = Liquidity strategy trade side");
    let _ = writeln!(w, "# [29] liqb_entry_price    = Liquidity entry price");
    let _ = writeln!(w, "# [30] liqb_exit_price     = Liquidity exit price");
    let _ = writeln!(w, "# [31] liqb_trade_pnl      = Liquidity PnL");
    let _ = writeln!(w, "# [32] liqb_balance        = Liquidity virtual balance");
    let _ = writeln!(w, "# [33] trades_per_second   = Binance trade rate (count/s)");
    let _ = writeln!(w, "# [34] price_velocity      = BTC price slope USD/s (1s window)");
    let _ = writeln!(w, "# [35] poly_liquidity_delta = Change in poly ask volume from prev tick");
    let _ = writeln!(w, "# [36] absorption_ratio    = Trade volume / |Δpoly_mid| (absorption)");
    let _ = writeln!(w, "# [37] price_gap_ratio     = % divergence Binance vs Poly since session start");
    let _ = writeln!(w, "# [38] spoofing_flag       = 1 if >50% volume drop without trade");
    let _ = writeln!(w, "# [39] tape_speed_flag     = 1 if volume spike detected");
    let _ = writeln!(w, "# [40] gap_alert_flag      = 1 if price gap > 0.5%");
    let _ = writeln!(w, "# [41] bollinger_sma       = Bollinger Band middle (SMA)");
    let _ = writeln!(w, "# [42] bollinger_upper     = Bollinger upper band (+2σ)");
    let _ = writeln!(w, "# [43] bollinger_lower     = Bollinger lower band (-2σ)");
    let _ = writeln!(w, "# [44] mean_reversion_signal = BB mean-reversion raw signal");
    let _ = writeln!(w, "# [45] technical_confluence = Multi-indicator confluence score");
    let _ = writeln!(w, "# [46] trend_direction     = 1=UP -1=DOWN 0=flat");
    let _ = writeln!(w, "# [47] signal_label        = Signal type label (TECH_CONFLUENCE|etc)");
    let _ = writeln!(w, "# [48] realized_volatility = Annualized realized volatility");
    let _ = writeln!(w, "# [49] high_volatility_event = 1 if BB width > 2σ threshold");
    let _ = writeln!(w, "# [50] bollinger_position  = Price position within BB (0-1)");
    let _ = writeln!(w, "# [51] master_signal       = 0=none 1=BB_BUY 2=BB_SELL 3-4=HUNT 5-6=MOM 7-8=MICRO 9-10=CP-ONLY");
    let _ = writeln!(w, "# [52] cp_uncertainty_range = Conformal Prediction uncertainty (USD)");
    let _ = writeln!(w, "# [53] cp_valid_signal      = Encoded: bit0=cp_valid bit1=hunting bit2=feedback bit3-4=layer");
    let _ = writeln!(w, "# [54] macro_slope          = SMA200 linear regression slope");
    let _ = writeln!(w, "# [55] vfi_value            = Volume Flow Indicator");
    let _ = writeln!(w, "# [56] macd_hist            = MACD(3,10,16) histogram");
    let _ = writeln!(w, "# [57] predicted_bias       = UP | DOWN — Hercules engine bias");
    let _ = writeln!(w, "# [58] is_feedback_adjusted = 1 if CP widened by RL feedback");
    let _ = writeln!(w, "# [59] dynamic_rsi          = Rolling RSI(14) updated each minute");
    let _ = writeln!(w, "# [60] vfi_confidence       = VFI volume strength ratio (0-1)");
    let _ = writeln!(w, "# [61] db_accuracy_factor   = Risk multiplier from DB feedback (1.0=neutral)");
    let _ = writeln!(w, "# ─── Hydra 85 (T-5 @ 0.85) ─────────────────────────────────────────");
    let _ = writeln!(w, "# [62] t5_prediction        = UP | DOWN | empty — prediction at T-300s");
    let _ = writeln!(w, "# [63] t5_entry_price       = Entry price if trade opened");
    let _ = writeln!(w, "# [64] t5_correct           = 1 if prediction matched outcome");
    let _ = writeln!(w, "# ─── Hydra 90 (T-3 @ 0.90) ─────────────────────────────────────────");
    let _ = writeln!(w, "# [65] t3_prediction        = UP | DOWN | empty");
    let _ = writeln!(w, "# [66] t3_entry_price       = Entry price");
    let _ = writeln!(w, "# [67] t3_active            = 1 if trade is open");
    let _ = writeln!(w, "# ─── Point of No Return (last 5 min) ───────────────────────────────");
    let _ = writeln!(w, "# [68] pnr_active           = 1 when seconds_left <= 300");
    let _ = writeln!(w, "# [69] pnr_seconds_left     = Seconds until session close");
    let _ = writeln!(w, "# [70] pnr_price            = poly_mid during PNR window");
    let _ = writeln!(w, "# [71] pnr_return_up        = 1.0 - poly_ask (expected UP return)");
    let _ = writeln!(w, "# [72] pnr_return_down      = poly_bid - 0.0 (expected DOWN return)");
    let _ = writeln!(w, "# [73] pnr_volatility_1m    = Liquidity delta as volatility proxy");
    let _ = writeln!(w, "# [74] pnr_confidence       = |poly_mid - 0.5| * 2 (0-1 scale)");
    let _ = writeln!(w, "# [75] pnr_trend            = +1 UP, -1 DOWN, 0 flat");
    let _ = writeln!(w, "# [76] pnr_spread_pct       = spread / mid ratio");
    let _ = writeln!(w, "# ─── Cerbero & Fenix (Range Insights) ───────────────────────────────");
    let _ = writeln!(w, "# [77] cerbero70_active     = 1 if poly_mid in [0.70, 0.80]");
    let _ = writeln!(w, "# [78] cerbero70_price      = poly_mid at capture (0 if not active)");
    let _ = writeln!(w, "# [79] cerbero70_dir        = 1=UP -1=DOWN 0=N/A");
    let _ = writeln!(w, "# [80] cerbero80_active     = 1 if poly_mid in [0.80, 0.90]");
    let _ = writeln!(w, "# [81] cerbero80_price      = poly_mid at capture");
    let _ = writeln!(w, "# [82] cerbero80_dir        = direction");
    let _ = writeln!(w, "# [83] cerbero90_active     = 1 if poly_mid in [0.90, 0.98]");
    let _ = writeln!(w, "# [84] cerbero90_price      = poly_mid at capture");
    let _ = writeln!(w, "# [85] cerbero90_dir        = direction");
    let _ = writeln!(w, "# [86] fenix35_active       = 1 if poly_mid in [0.35, 0.65]");
    let _ = writeln!(w, "# [87] fenix35_price        = poly_mid at capture");
    let _ = writeln!(w, "# [88] fenix35_dir          = direction (UP if >0.5)");
    let _ = writeln!(w, "# [89] fenix30_active       = 1 if poly_mid in [0.30, 0.50]");
    let _ = writeln!(w, "# [90] fenix30_price        = poly_mid at capture");
    let _ = writeln!(w, "# [91] fenix30_dir          = direction");
    let _ = writeln!(w, "# [92] fenix45_active       = 1 if poly_mid in [0.45, 0.55]");
    let _ = writeln!(w, "# [93] fenix45_price        = poly_mid at capture");
    let _ = writeln!(w, "# [94] fenix45_dir          = direction");
    let _ = writeln!(w, "# ─── Fenix Trading (paper-trading) ──────────────────────────────────");
    let _ = writeln!(w, "# [95] fenix35_trade       = 1=active trade");
    let _ = writeln!(w, "# [96] fenix30_trade       = 1=active trade");
    let _ = writeln!(w, "# [97] fenix45_trade       = 1=active trade");
    let _ = writeln!(w, "# [98] fenix40_trade       = 1=active trade");
    let _ = writeln!(w, "# [99] fenix4550_trade     = 1=active trade");
    let _ = writeln!(w, "# ─── Fenix Skip Reason (diagnostic) ────────────────────────────────");
    let _ = writeln!(w, "# [100] fenix35_skip       = 0=none 1=trend blocked 2=spread blocked 3=volume blocked");
    let _ = writeln!(w, "# [101] fenix30_skip       = 0=none 1=trend blocked 2=spread blocked 3=volume blocked");
    let _ = writeln!(w, "# [102] fenix45_skip       = 0=none 1=trend blocked 2=spread blocked 3=volume blocked");
    let _ = writeln!(w, "# [103] fenix40_skip       = 0=none 1=trend blocked 2=spread blocked 3=volume blocked");
    let _ = writeln!(w, "# [104] fenix4550_skip     = 0=none 1=trend blocked 2=spread blocked 3=volume blocked");
    let _ = writeln!(w, "# ─── Fenix Live PnL ─────────────────────────────────────────────────");
    let _ = writeln!(w, "# [105] fenix35_entry      = entry price");
    let _ = writeln!(w, "# [106] fenix35_pnl        = live PnL");
    let _ = writeln!(w, "# [107] fenix30_entry      = entry price");
    let _ = writeln!(w, "# [108] fenix30_pnl        = live PnL");
    let _ = writeln!(w, "# [109] fenix45_entry      = entry price");
    let _ = writeln!(w, "# [110] fenix45_pnl        = live PnL");
    let _ = writeln!(w, "# [111] fenix40_entry      = entry price");
    let _ = writeln!(w, "# [112] fenix40_pnl        = live PnL");
    let _ = writeln!(w, "# [113] fenix4550_entry    = entry price");
    let _ = writeln!(w, "# [114] fenix4550_pnl      = live PnL");
    let _ = writeln!(w, "# [115] fenix_signal        = 0=none 1=UP 2=DOWN (delta bid/ask + velocity)");
    let _ = writeln!(w, "# ─── Fenix Target + Exit ────────────────────────────────────────────");
    let _ = writeln!(w, "# [116] fenix35_target      = exit target price");
    let _ = writeln!(w, "# [117] fenix30_target      = exit target price");
    let _ = writeln!(w, "# [118] fenix45_target      = exit target price");
    let _ = writeln!(w, "# [119] fenix40_target      = exit target price");
    let _ = writeln!(w, "# [120] fenix4550_target    = exit target price");
    let _ = writeln!(w, "# [121] fenix35_exit        = 1=target hit");
    let _ = writeln!(w, "# [122] fenix30_exit        = 1=target hit");
    let _ = writeln!(w, "# [123] fenix45_exit        = 1=target hit");
    let _ = writeln!(w, "# [124] fenix40_exit        = 1=target hit");
    let _ = writeln!(w, "# [125] fenix4550_exit      = 1=target hit");
    let _ = writeln!(w, "# ─── Market Pressure ─────────────────────────────────────────────────");
    let _ = writeln!(w, "# [126] pressure_bid_floor   = lowest bid price with vol >= 10");
    let _ = writeln!(w, "# [127] pressure_ask_ceiling = highest ask price with vol >= 10");
    let _ = writeln!(w, "# [128] pressure_band        = ask_ceiling - bid_floor");
    let _ = writeln!(w, "# [129] pressure_index       = (mid-floor)/band, 0=DOWN 1=UP");
    let _ = writeln!(w, "# [130] pressure_skew        = (bid_vol-ask_vol)/total in band");
    let _ = writeln!(w, "# ─────────────────────────────────────────────────────────────────────");

}
/// Uses a single pre-allocated String with write! to avoid per-field allocation.
/// Called outside the writer lock — only the final `write_all` is inside the mutex.
#[inline]
/// Formats CSV line dynamically. Uses Vec+join to avoid manual `{}` counting.
#[inline]
fn fast_format_csv_line(r: &CsvRecord) -> String {
    let mut fields: Vec<String> = Vec::with_capacity(130);
    fields.push(r.ts_local.clone());
    fields.push(r.ts_exchange.clone());
    fields.push(r.event_type.as_str().to_string());
    fields.push(r.latencia_ms.to_string());
    fields.push(r.binance_price.to_string());
    fields.push(r.binance_micro_price.to_string());
    fields.push(r.binance_imbalance.to_string());
    fields.push(r.binance_vol_100ms.to_string());
    fields.push(r.binance_vol_24h.to_string());
    fields.push(r.poly_bid.to_string());
    fields.push(r.poly_ask.to_string());
    fields.push(r.poly_mid.to_string());
    fields.push(r.poly_spread.to_string());
    fields.push(r.poly_bid_vol_all.to_string());
    fields.push(r.poly_ask_vol_all.to_string());
    fields.push(r.poly_imbalance.to_string());
    fields.push(r.trade_side.clone());
    fields.push(r.trade_price.to_string());
    fields.push(r.trade_size.to_string());
    fields.push(r.is_informed.to_string());
    fields.push(r.imba_status.clone());
    fields.push(r.imba_side.clone());
    fields.push(r.imba_entry_price.to_string());
    fields.push(r.imba_exit_price.to_string());
    fields.push(r.imba_trade_pnl.to_string());
    fields.push(r.imba_balance.to_string());
    fields.push(r.liqb_status.clone());
    fields.push(r.liqb_side.clone());
    fields.push(r.liqb_entry_price.to_string());
    fields.push(r.liqb_exit_price.to_string());
    fields.push(r.liqb_trade_pnl.to_string());
    fields.push(r.liqb_balance.to_string());
    fields.push(r.trades_per_second.to_string());
    fields.push(r.price_velocity.to_string());
    fields.push(r.poly_liquidity_delta.to_string());
    fields.push(r.absorption_ratio.to_string());
    fields.push(r.price_gap_ratio.to_string());
    fields.push(r.spoofing_flag.to_string());
    fields.push(r.tape_speed_flag.to_string());
    fields.push(r.gap_alert_flag.to_string());
    fields.push(r.bollinger_sma.to_string());
    fields.push(r.bollinger_upper.to_string());
    fields.push(r.bollinger_lower.to_string());
    fields.push(r.mean_reversion_signal.to_string());
    fields.push(r.technical_confluence.to_string());
    fields.push(r.trend_direction.to_string());
    fields.push(r.signal_label.clone());
    fields.push(r.realized_volatility.to_string());
    fields.push(r.high_volatility_event.to_string());
    fields.push(r.bollinger_position.to_string());
    fields.push(r.master_signal.to_string());
    fields.push(r.cp_uncertainty_range.to_string());
    fields.push(r.cp_valid_signal.to_string());
    fields.push(r.macro_slope.to_string());
    fields.push(r.vfi_value.to_string());
    fields.push(r.macd_hist.to_string());
    fields.push(r.predicted_bias.clone());
    fields.push(r.is_feedback_adjusted.to_string());
    fields.push(r.dynamic_rsi.to_string());
    fields.push(r.vfi_confidence.to_string());
    fields.push(r.db_accuracy_factor.to_string());
    fields.push(r.t5_prediction.clone());
    fields.push(r.t5_entry_price.to_string());
    fields.push(r.t5_correct.to_string());
    fields.push(r.t3_prediction.clone());
    fields.push(r.t3_entry_price.to_string());
    fields.push(r.t3_active.to_string());
    fields.push(r.pnr_active.to_string());
    fields.push(r.pnr_seconds_left.to_string());
    fields.push(r.pnr_price.to_string());
    fields.push(r.pnr_return_up.to_string());
    fields.push(r.pnr_return_down.to_string());
    fields.push(r.pnr_volatility_1m.to_string());
    fields.push(r.pnr_confidence.to_string());
    fields.push(r.pnr_trend.to_string());
    fields.push(r.pnr_spread_pct.to_string());
    fields.push(r.cerbero70_active.to_string());
    fields.push(r.cerbero70_price.to_string());
    fields.push(r.cerbero70_dir.to_string());
    fields.push(r.cerbero80_active.to_string());
    fields.push(r.cerbero80_price.to_string());
    fields.push(r.cerbero80_dir.to_string());
    fields.push(r.cerbero90_active.to_string());
    fields.push(r.cerbero90_price.to_string());
    fields.push(r.cerbero90_dir.to_string());
    fields.push(r.fenix35_active.to_string());
    fields.push(r.fenix35_price.to_string());
    fields.push(r.fenix35_dir.to_string());
    fields.push(r.fenix30_active.to_string());
    fields.push(r.fenix30_price.to_string());
    fields.push(r.fenix30_dir.to_string());
    fields.push(r.fenix45_active.to_string());
    fields.push(r.fenix45_price.to_string());
    fields.push(r.fenix45_dir.to_string());
    fields.push(r.fenix35_trade.to_string());
    fields.push(r.fenix30_trade.to_string());
    fields.push(r.fenix45_trade.to_string());
    fields.push(r.fenix40_trade.to_string());
    fields.push(r.fenix4550_trade.to_string());
    fields.push(r.fenix35_skip.to_string());
    fields.push(r.fenix30_skip.to_string());
    fields.push(r.fenix45_skip.to_string());
    fields.push(r.fenix40_skip.to_string());
    fields.push(r.fenix4550_skip.to_string());
    fields.push(r.fenix35_entry.to_string());
    fields.push(r.fenix35_pnl.to_string());
    fields.push(r.fenix30_entry.to_string());
    fields.push(r.fenix30_pnl.to_string());
    fields.push(r.fenix45_entry.to_string());
    fields.push(r.fenix45_pnl.to_string());
    fields.push(r.fenix40_entry.to_string());
    fields.push(r.fenix40_pnl.to_string());
    fields.push(r.fenix4550_entry.to_string());
    fields.push(r.fenix4550_pnl.to_string());
    fields.push(r.fenix35_target.to_string());
    fields.push(r.fenix30_target.to_string());
    fields.push(r.fenix45_target.to_string());
    fields.push(r.fenix40_target.to_string());
    fields.push(r.fenix4550_target.to_string());
    fields.push(r.fenix35_exit.to_string());
    fields.push(r.fenix30_exit.to_string());
    fields.push(r.fenix45_exit.to_string());
    fields.push(r.fenix40_exit.to_string());
    fields.push(r.fenix4550_exit.to_string());
    fields.push(r.fenix_signal.to_string());
    fields.push(r.pressure_bid_floor.to_string());
    fields.push(r.pressure_ask_ceiling.to_string());
    fields.push(r.pressure_band.to_string());
    fields.push(r.pressure_index.to_string());
    fields.push(r.pressure_skew.to_string());
    fields.join(",")
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
                              cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor,t5_prediction,t5_entry_price,t5_correct,t3_prediction,t3_entry_price,t3_active,pnr_active,pnr_seconds_left,pnr_price,pnr_return_up,pnr_return_down,pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct,cerbero70_active,cerbero70_price,cerbero70_dir,cerbero80_active,cerbero80_price,cerbero80_dir,cerbero90_active,cerbero90_price,cerbero90_dir,fenix35_active,fenix35_price,fenix35_dir,fenix30_active,fenix30_price,fenix30_dir,fenix45_active,fenix45_price,fenix45_dir,fenix35_trade,fenix30_trade,fenix45_trade,fenix40_trade,fenix4550_trade,fenix35_skip,fenix30_skip,fenix45_skip,fenix40_skip,fenix4550_skip,fenix35_entry,fenix35_pnl,fenix30_entry,fenix30_pnl,fenix45_entry,fenix45_pnl,fenix40_entry,fenix40_pnl,fenix4550_entry,fenix4550_pnl,fenix35_target,fenix30_target,fenix45_target,fenix40_target,fenix4550_target,fenix35_exit,fenix30_exit,fenix45_exit,fenix40_exit,fenix4550_exit,fenix_signal,pressure_bid_floor,pressure_ask_ceiling,pressure_band,pressure_index,pressure_skew"
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

        let mut writer = BufWriter::with_capacity(52_428_800, file); // 50 MiB

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
                     cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor,t5_prediction,t5_entry_price,t5_correct,t3_prediction,t3_entry_price,t3_active,pnr_active,pnr_seconds_left,pnr_price,pnr_return_up,pnr_return_down,pnr_volatility_1m,pnr_confidence,pnr_trend,pnr_spread_pct,cerbero70_active,cerbero70_price,cerbero70_dir,cerbero80_active,cerbero80_price,cerbero80_dir,cerbero90_active,cerbero90_price,cerbero90_dir,fenix35_active,fenix35_price,fenix35_dir,fenix30_active,fenix30_price,fenix30_dir,fenix45_active,fenix45_price,fenix45_dir,fenix35_trade,fenix30_trade,fenix45_trade,fenix40_trade,fenix4550_trade,fenix35_skip,fenix30_skip,fenix45_skip,fenix40_skip,fenix4550_skip,fenix35_entry,fenix35_pnl,fenix30_entry,fenix30_pnl,fenix45_entry,fenix45_pnl,fenix40_entry,fenix40_pnl,fenix4550_entry,fenix4550_pnl,fenix35_target,fenix30_target,fenix45_target,fenix40_target,fenix4550_target,fenix35_exit,fenix30_exit,fenix45_exit,fenix40_exit,fenix4550_exit,fenix_signal,pressure_bid_floor,pressure_ask_ceiling,pressure_band,pressure_index,pressure_skew"
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
