/// Monte Carlo — Odiseo strategies
/// Uso: cargo run --release --example montecarlo
use std::time::Instant;

// Simple LCG random number generator
struct Rng(u64);
impl Rng {
    fn new() -> Self { Rng(123456789) }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn f64(&mut self) -> f64 { (self.next() as f64) / (u64::MAX as f64) }
    fn range(&mut self, lo: f64, hi: f64) -> f64 { lo + self.f64() * (hi - lo) }
    fn bool(&mut self, p: f64) -> bool { self.f64() < p }
}

const BUDGET: f64 = 100.0;
const SESSIONS: usize = 1_000_000;
const TICKS: usize = 600; // 15-min
const PROFIT_STOP_PCT: f64 = 0.15;
const SL_LIMIT: u32 = 4;

#[derive(Debug)]
struct Resultado {
    name: String,
    entry: f64,
    tp: f64,
    sl: f64,
    avg_pnl: f64,
    win_rate: f64,
    profit_per_win: f64,
    loss_per_sl: f64,
    breakeven_wr: f64,
    entries_per_session: f64,
}

fn simulacion(name: &str, entry: f64, tp: f64, sl: f64) -> Resultado {
    let mut rng = Rng::new();
    let size = (BUDGET / entry) as usize;
    let profit_limit = BUDGET * PROFIT_STOP_PCT;
    let mut pnl_total = 0.0;
    let mut wins = 0u64;
    let mut losses = 0u64;
    let mut total_entries = 0u64;

    for _ in 0..SESSIONS {
        let mut px: f64 = rng.range(0.3, 0.7);
        let mut bias: f64 = if rng.bool(0.5) { 1.0 } else { -1.0 };
        let mut sp = 0.0;
        let mut sl_count = 0u32;
        let mut entered = false;
        let mut ep = 0.0;
        let mut max_px = 0.0;
        let mut session_entries = 0u64;

        for _ in 0..TICKS {
            if sp >= profit_limit || sl_count >= SL_LIMIT {
                break;
            }
            px += bias * rng.range(0.001, 0.005) + rng.range(-0.003, 0.003);
            px = px.clamp(0.01, 0.99);
            if rng.bool(0.02) { bias *= -1.0; }

            if !entered {
                if px >= entry && px <= tp {
                    ep = px; entered = true; max_px = px;
                    session_entries += 1; total_entries += 1;
                    for _ in 0..TICKS {
                        px += bias * rng.range(0.001, 0.005) + rng.range(-0.003, 0.003);
                        px = px.clamp(0.01, 0.99);
                        if rng.bool(0.02) { bias *= -1.0; }
                        if px > max_px { max_px = px; }
                        if px >= tp { sp += (tp - ep) * size as f64; wins += 1; entered = false; break; }
                        else if max_px - px > 0.03 { sp += (px - ep) * size as f64; sl_count += 1; losses += 1; entered = false; break; }
                        else if px <= sl { sp += (sl - ep) * size as f64; sl_count += 1; losses += 1; entered = false; break; }
                    }
                }
            }
        }
        pnl_total += sp;
    }

    let total = wins + losses;
    let wr = if total > 0 {
        (wins as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let ppw = (tp - entry) * size as f64;
    let pls = (sl - entry) * size as f64;
    let be = pls.abs() / (ppw + pls.abs()) * 100.0;

    Resultado {
        name: name.to_string(),
        entry,
        tp,
        sl,
        avg_pnl: pnl_total / SESSIONS as f64,
        win_rate: wr,
        profit_per_win: ppw,
        loss_per_sl: pls,
        breakeven_wr: be,
        entries_per_session: total_entries as f64 / SESSIONS as f64,
    }
}

fn main() {
    let start = Instant::now();
    println!("═══ Monte Carlo — {}K sesiones × estrategia — Budget ${} ═══\n", SESSIONS/1000, BUDGET as u64);

    let estrategias = vec![
        ("Odiseo 83", 0.83, 0.97, 0.81),
        ("Wide 65",   0.65, 0.95, 0.63),
        ("Early 70",  0.70, 0.85, 0.68),
        ("Late 90",   0.90, 0.97, 0.88),
        ("Wide 55",   0.55, 0.85, 0.53),
        ("Rev 20",    0.20, 0.30, 0.15),
    ];

    let mut resultados: Vec<Resultado> = Vec::new();

    for (name, e, tp, sl) in &estrategias {
        let r = simulacion(name, *e, *tp, *sl);
        resultados.push(r);
    }

    // Print table
    println!("{:<15} {:>10} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "Estrategia", "PnL/ses", "WR", "+$/win", "-$/loss", "BE WR", "Ent/ses", "PnL/dia");
    println!("{}", "-".repeat(85));

    for r in &resultados {
        let sesiones_dia = 28.0; // BTC 15-min sessions per day
        let pnl_dia = r.avg_pnl * sesiones_dia * r.entries_per_session;
        println!("{:<15} ${:>9.2}  {:>6.1}%  ${:>6.2}  ${:>6.2}  {:>5.1}%  {:>6.2}  ${:>8.0}",
            r.name, r.avg_pnl, r.win_rate, r.profit_per_win, r.loss_per_sl.abs(),
            r.breakeven_wr, r.entries_per_session, pnl_dia);
    }

    // Combined: 83 + 65
    let r83 = &resultados[0];
    let r65 = &resultados[1];
    let combined_daily = (r83.avg_pnl * r83.entries_per_session + r65.avg_pnl * r65.entries_per_session) * 28.0;
    let combined_weekly = combined_daily * 7.0;
    let combined_monthly = combined_daily * 30.0;

    println!("\n═══ Combinado: Odiseo 83 + Wide 65 — Budget ${} each ═══", BUDGET as u64);
    println!("  Diario:    ${:.0}", combined_daily);
    println!("  Semanal:   ${:.0}", combined_weekly);
    println!("  Mensual:   ${:.0}", combined_monthly);

    let elapsed = start.elapsed();
    println!("\n  {}K sesiones × {} estrategias = {}M simulaciones en {:.2}s",
        SESSIONS/1000, estrategias.len(), SESSIONS * estrategias.len() / 1_000_000, elapsed.as_secs_f64());
}
