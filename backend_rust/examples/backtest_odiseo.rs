/// Backtest Odiseo 83 con datos reales de sesiones CSV
/// Uso: cargo run --release --example backtest_odiseo -- /path/session_837_hft.csv
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};

const ENTRY: f64 = 0.83;
const TP: f64 = 0.97;
const SL_HARD: f64 = 0.81;
const SL_TREND: f64 = 0.03;
const BUDGET: f64 = 100.0;
const PROFIT_STOP: f64 = 0.15;
const SL_LIMIT: u32 = 4;

fn main() -> anyhow::Result<()> {
    let path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Uso: cargo run --release --example backtest_odiseo -- <csv_file>");
        std::process::exit(1);
    });

    let f = File::open(&path)?;
    let reader = BufReader::new(f);
    let lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();

    // Find header
    let header_idx = lines.iter().position(|l| l.starts_with("ts_local")).unwrap_or(0);
    let header = &lines[header_idx];
    let cols: Vec<&str> = header.split(',').collect();

    // Find poly_mid column
    let mid_col = cols.iter().position(|c| *c == "poly_mid").unwrap_or(10);
    let trade_price_col = cols.iter().position(|c| *c == "trade_price").unwrap_or(15);
    let trade_side_col = cols.iter().position(|c| *c == "trade_side").unwrap_or(14);

    println!("═══ Backtest Odiseo 83 — Datos reales — Budget ${} ═══\n", BUDGET as u64);
    println!("Archivo: {}", path);
    println!("Entradas: {}\n", lines.len() - header_idx - 1);

    let data: Vec<(String, f64)> = lines[header_idx + 1..]
        .iter()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split(',').collect();
            let ts = parts[0].to_string();
            let mid: f64 = parts.get(mid_col).and_then(|v| v.parse().ok()).unwrap_or(0.0);
            if mid > 0.0 && mid < 1.0 {
                Some((ts, mid))
            } else {
                None
            }
        })
        .collect();

    if data.is_empty() {
        println!("No data found in CSV");
        return Ok(());
    }

    let size = (BUDGET / ENTRY) as usize;
    let profit_limit = BUDGET * PROFIT_STOP;
    let mut session_pnl = 0.0;
    let mut sl_count = 0u32;
    let mut entered = false;
    let mut ep = 0.0;
    let mut max_px = 0.0;
    let mut wins = 0u64;
    let mut losses = 0u64;
    let mut total_pnl = 0.0;
    let mut trades: Vec<(String, String, f64, f64, f64)> = Vec::new();

    for (i, (ts, mid)) in data.iter().enumerate() {
        if session_pnl >= profit_limit || sl_count >= SL_LIMIT {
            break; // session stop
        }

        if !entered {
            if *mid >= ENTRY && *mid <= TP {
                ep = *mid;
                entered = true;
                max_px = *mid;
                // println!("  ENTER @{:.4} at {}", ep, ts);
            }
        } else {
            if *mid > max_px {
                max_px = *mid;
            }

            let mut exit_reason = 0u8;
            let mut exit_price = *mid;

            if *mid >= TP {
                exit_reason = 1;
                exit_price = TP;
            } else if max_px - *mid > SL_TREND {
                exit_reason = 3;
            } else if *mid <= SL_HARD {
                exit_reason = 4;
            }

            if exit_reason > 0 {
                let pnl = (exit_price - ep) * size as f64;
                session_pnl += pnl;
                total_pnl += pnl;

                if pnl > 0.0 {
                    wins += 1;
                } else {
                    sl_count += 1;
                    losses += 1;
                }

                let label = match exit_reason {
                    1 => "TP",
                    3 => "SL-T",
                    4 => "SL-H",
                    _ => "??",
                };
                trades.push((ts.clone(), label.to_string(), ep, exit_price, pnl));
                // println!("  EXIT {} @{:.4} pnl={:+.2} at {}", label, exit_price, pnl, ts);
                entered = false;
            }
        }
    }

    // Results
    println!("{:<22} {:>8} {:>8} {:>10}", "Timestamp", "Exit", "Entry", "Pnl");
    println!("{}", "-".repeat(55));
    for (ts, reason, e, x, pnl) in &trades {
        let ts_short = &ts[11..19];
        println!("{:<22} {:>8} {:>8.4} {:>+10.2}", ts_short, reason, e, pnl);
    }

    let total = wins + losses;
    let wr = if total > 0 {
        (wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    println!("\n═══ Resumen ═══");
    println!("  Trades: {} ({}W/{}L)", total, wins, losses);
    println!("  Win Rate: {:.1}%", wr);
    println!("  PnL Total: ${:+.2}", total_pnl);
    println!("  Profit per win: ${:.2}", (TP - ENTRY) * size as f64);
    println!("  Loss per SL:    ${:.2}", (SL_HARD - ENTRY) * size as f64);

    if total_pnl > 0.0 {
        println!("\n  ✅ ESTRATEGIA RENTABLE en esta sesión");
    } else {
        println!("\n  ❌ Estrategia NO rentable en esta sesión");
    }

    Ok(())
}
