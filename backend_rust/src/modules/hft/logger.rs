use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::modules::hft::types::CsvRecord;

pub struct CsvLogger {
    path:   String,
    buffer: Mutex<Vec<CsvRecord>>,
}

impl CsvLogger {
    pub fn new(path: &str) -> Self {
        Self {
            path:   path.to_string(),
            buffer: Mutex::new(Vec::with_capacity(2048)),
        }
    }

    pub fn push(&self, rec: CsvRecord) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(rec);
        }
    }

    pub fn flush(&self) {
        let rows: Vec<CsvRecord> = {
            let mut buf = match self.buffer.lock() {
                Ok(b) => b,
                Err(_) => return,
            };
            if buf.is_empty() { return; }
            std::mem::take(&mut *buf)
        };

        let file_exists = std::path::Path::new(&self.path).exists();
        let mut file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(f) => BufWriter::new(f),
            Err(e) => {
                tracing::warn!("HFT CSV: no se pudo abrir {}: {}", self.path, e);
                return;
            }
        };

        if !file_exists {
            let _ = writeln!(
                file,
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
                 cp_uncertainty_range,cp_valid_signal,macro_slope,vfi_value,macd_hist,predicted_bias,is_feedback_adjusted,dynamic_rsi,vfi_confidence,db_accuracy_factor"
            );
        }

        for r in &rows {
            let _ = writeln!(
                file,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                r.ts_local,
                r.ts_exchange,
                r.event_type.as_str(),
                r.latencia_ms,
                r.binance_price,
                r.binance_micro_price,
                r.binance_imbalance,
                r.binance_vol_100ms,
                r.binance_vol_24h,
                r.poly_bid,
                r.poly_ask,
                r.poly_mid,
                r.poly_spread,
                r.poly_bid_vol_all,
                r.poly_ask_vol_all,
                r.poly_imbalance,
                r.trades_per_second,
                r.price_velocity,
                r.poly_liquidity_delta,
                r.absorption_ratio,
                r.price_gap_ratio,
                r.spoofing_flag,
                r.tape_speed_flag,
                r.gap_alert_flag,
            );
        }

        if rows.len() > 10 {
            tracing::info!("HFT CSV: {} ({} filas)", self.path, rows.len());
        }
    }
}

/// Flush del CSV cada 60 segundos.
pub async fn csv_flush_loop(logger: Arc<CsvLogger>, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = interval.tick() => { logger.flush(); }
            _ = shutdown.recv() => {
                logger.flush();
                tracing::info!("HFT CSV: flush final y shutdown");
                return;
            }
        }
    }
}
