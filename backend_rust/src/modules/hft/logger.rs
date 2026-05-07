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
            let _ = writeln!(file, "{}", CsvRecord::csv_header());
        }

        for r in &rows {
            let _ = writeln!(file, "{}", r.to_csv_line());
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
