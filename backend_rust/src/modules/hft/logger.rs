use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;

use crate::modules::hft::types::HftMetrics;

#[derive(Debug, Clone)]
struct CsvRow {
    ts:                     String,
    btc_price:              f64,
    btc_bid_vol_5:          f64,
    btc_ask_vol_5:          f64,
    binance_lag_ms:         i64,
    binance_micro_price_at_t: f64,
    poly_mid_price:         f64,
    poly_imbalance:         f64,
    latency_delta:          f64,
}

pub struct CsvLogger {
    path:   String,
    buffer: Mutex<Vec<CsvRow>>,
}

impl CsvLogger {
    pub fn new(path: &str) -> Self {
        Self {
            path:   path.to_string(),
            buffer: Mutex::new(Vec::with_capacity(1024)),
        }
    }

    pub fn push(&self, metrics: &HftMetrics) {
        let row = CsvRow {
            ts:                      Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            btc_price:               metrics.btc_price_binance,
            btc_bid_vol_5:           metrics.binance_bid_vol_5,
            btc_ask_vol_5:           metrics.binance_ask_vol_5,
            binance_lag_ms:          metrics.binance_lag_ms,
            binance_micro_price_at_t: metrics.binance_micro_price_at_t,
            poly_mid_price:          metrics.poly_mid_price,
            poly_imbalance:          metrics.poly_imbalance,
            latency_delta:           metrics.latency_delta,
        };
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(row);
        }
    }

    pub fn flush(&self) {
        let rows: Vec<CsvRow> = {
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
                "timestamp,btc_price_binance,btc_bid_vol_5,btc_ask_vol_5,binance_lag_ms,binance_micro_price_at_t,poly_mid_price,poly_imbalance,latency_delta"
            );
        }

        for row in &rows {
            let _ = writeln!(
                file,
                "{},{},{},{},{},{},{},{},{}",
                row.ts,
                row.btc_price,
                row.btc_bid_vol_5,
                row.btc_ask_vol_5,
                row.binance_lag_ms,
                row.binance_micro_price_at_t,
                row.poly_mid_price,
                row.poly_imbalance,
                row.latency_delta,
            );
        }

        if rows.len() > 10 {
            tracing::info!("HFT CSV: {} ({:.1} KB)", self.path, rows.len() as f64 * 140.0 / 1024.0);
        }
    }
}

/// Tarea de fondo: flush del CSV cada 60 segundos.
pub async fn csv_flush_loop(logger: Arc<CsvLogger>, mut shutdown: tokio::sync::broadcast::Receiver<()>) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                logger.flush();
            }
            _ = shutdown.recv() => {
                logger.flush();
                tracing::info!("HFT CSV: flush final y shutdown");
                return;
            }
        }
    }
}
