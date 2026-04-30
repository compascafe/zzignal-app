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
                 liqb_status,liqb_side,liqb_entry_price,liqb_exit_price,liqb_trade_pnl,liqb_balance"
            );
        }

        for r in &rows {
            let _ = writeln!(
                file,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
                r.trade_side,
                r.trade_price,
                r.trade_size,
                r.is_informed,
                r.imba_status,
                r.imba_side,
                r.imba_entry_price,
                r.imba_exit_price,
                r.imba_trade_pnl,
                r.imba_balance,
                r.liqb_status,
                r.liqb_side,
                r.liqb_entry_price,
                r.liqb_exit_price,
                r.liqb_trade_pnl,
                r.liqb_balance,
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
