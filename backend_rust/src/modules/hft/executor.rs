// ─── Paper Trading Executor ──────────────────────────────────────────────────
// Simula operaciones de trading con $20 USD de capital virtual.
//
// Parámetros por estacionalidad (según duration_min del mercado):
//
//   5-min (Sniper):
//     - Trigger volumen: binance_vol_100ms > 0.4
//     - Desviación micro_price: > $0.50 del binance_price
//     - TP: $0.012 por contrato o 10 segundos
//
//   15-min (Arbitraje):
//     - Trigger volumen: binance_vol_100ms > 0.2
//     - Desviación micro_price: > $0.30 del binance_price
//     - TP: $0.025 por contrato o 30 segundos
//
// Proximidad al Strike:
//   - Si |poly_mid - strike| < $1.50 → ignora filtros de volumen Y desviación
//     Dispara solo por dirección del binance_micro_price (sniper mode).
//
// Ejecución realista:
//   - Compra: poly_ask, Venta: poly_bid
//   - Comisión: $0.001 por contrato por trade
//   - Capital inicial: $20.00 (persiste entre eventos, no se resetea)

use crate::modules::hft::types::CsvRecord;

/// Lado de la simulación
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SimSide {
    Buy,
    Sell,
}

impl SimSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

/// Resultado de la evaluación por fila
#[derive(Debug, Clone)]
pub struct SimResult {
    pub status:          String, // IDLE|OPEN|CLOSED
    pub side:            String, // BUY|SELL (empty if IDLE)
    pub entry_price:     f64,
    pub exit_price:      f64,
    pub pnl_trade:       f64,
    pub current_balance: f64,
}

impl Default for SimResult {
    fn default() -> Self {
        Self {
            status:          "IDLE".to_string(),
            side:            String::new(),
            entry_price:     0.0,
            exit_price:      0.0,
            pnl_trade:       0.0,
            current_balance: 0.0,
        }
    }
}

/// Posición abierta del paper trader
#[derive(Debug, Clone)]
struct Position {
    side:            SimSide,
    entry_price:     f64,
    entry_time_ms:   i64,
    size_contracts:  f64,
    market_duration: i32,
}

/// Parámetros por estacionalidad — ajustados para mayor sensibilidad
struct SeasonParams {
    /// binance_vol_100ms mínimo para disparar (se ignora en proximity)
    vol_trigger:      f64,
    /// Desviación mínima de micro_price vs binance_price (se ignora en proximity)
    micro_deviation:  f64,
    /// Take-profit USD por contrato
    tp_per_contract:  f64,
    /// Timeout de posición en ms
    tp_timeout_ms:    i64,
}

fn params_for(duration_min: i32) -> SeasonParams {
    match duration_min {
        5 => SeasonParams {
            // Sniper 5-min: alta sensibilidad
            vol_trigger:     0.4,
            micro_deviation: 0.50,
            tp_per_contract: 0.012,
            tp_timeout_ms:   10_000,
        },
        _ => SeasonParams {
            // Arbitraje 15-min (default): sensibilidad moderada
            vol_trigger:     0.2,
            micro_deviation: 0.30,
            tp_per_contract: 0.025,
            tp_timeout_ms:   30_000,
        },
    }
}

// ─── Constantes ──────────────────────────────────────────────────────────────

const COMMISSION_PER_CONTRACT: f64 = 0.001;
const INITIAL_CAPITAL: f64 = 20.00;
/// Distancia al strike que activa el modo sniper (ignora vol + deviation)
const STRIKE_PROXIMITY_SNIPER: f64 = 1.50;

/// Paper Trading Executor — evalúa cada fila del CSV y decide si abrir/cerrar posición.
/// El balance persiste entre llamadas (no se resetea).
pub struct PaperExecutor {
    pub balance:         f64,
    position:            Option<Position>,
    pub total_pnl:       f64,
    pub trades_closed:   u32,
}

impl PaperExecutor {
    pub fn new() -> Self {
        Self {
            balance:       INITIAL_CAPITAL,
            position:      None,
            total_pnl:     0.0,
            trades_closed: 0,
        }
    }

    /// Evalúa un CsvRecord contra la lógica de trading.
    /// `strike_price` — precio BTC al inicio del intervalo (price_to_beat).
    /// `market_duration_min` — 5 o 15, define los thresholds.
    pub fn evaluate(
        &mut self,
        rec: &CsvRecord,
        strike_price: Option<f64>,
        market_duration_min: i32,
    ) -> SimResult {
        let now_ms = rec.ts_exchange.parse::<i64>().unwrap_or(0);
        let params = params_for(market_duration_min);

        // ─── Validación de spread ──────────────────────────────────────────
        // Si el spread es negativo o cero (datos corruptos), no operar.
        let spread = rec.poly_ask - rec.poly_bid;
        if rec.poly_ask <= 0.0 || rec.poly_bid <= 0.0 || spread <= 0.0 {
            return self.idle_result();
        }

        // ─── Posición abierta → evaluar cierre ─────────────────────────────
        if let Some(ref pos) = self.position {
            let exit_price = match pos.side {
                SimSide::Buy => rec.poly_bid,  // vender al bid
                SimSide::Sell => rec.poly_ask, // comprar al ask
            };
            if exit_price <= 0.0 {
                return self.idle_result();
            }

            let pnl_per_contract = match pos.side {
                SimSide::Buy => exit_price - pos.entry_price,
                SimSide::Sell => pos.entry_price - exit_price,
            };
            let gross_pnl = pnl_per_contract * pos.size_contracts;

            // Condiciones de cierre (TP o timeout)
            let tp_hit = gross_pnl >= params.tp_per_contract * pos.size_contracts;
            let timed_out = now_ms - pos.entry_time_ms >= params.tp_timeout_ms;

            if tp_hit || timed_out {
                let commission = COMMISSION_PER_CONTRACT * pos.size_contracts;
                let net_pnl = gross_pnl - commission;
                self.balance += net_pnl;
                self.total_pnl += net_pnl;
                self.trades_closed += 1;

                let result = SimResult {
                    status:          "CLOSED".to_string(),
                    side:            pos.side.as_str().to_string(),
                    entry_price:     pos.entry_price,
                    exit_price,
                    pnl_trade:       net_pnl,
                    current_balance: self.balance,
                };
                self.position = None;
                return result;
            }

            // Posición sigue abierta — reportar estado actual
            return SimResult {
                status:          "OPEN".to_string(),
                side:            pos.side.as_str().to_string(),
                entry_price:     pos.entry_price,
                exit_price:      0.0,
                pnl_trade:       0.0,
                current_balance: self.balance,
            };
        }

        // ─── IDLE: evaluar trigger de entrada ─────────────────────────────

        // Proximidad al strike: modo sniper
        // Si |poly_mid - strike| < $1.50 → ignora volumen Y desviación,
        // dispara solo por dirección del micro_price.
        let mut sniper_mode = false;
        if let Some(strike) = strike_price {
            let distance = (rec.poly_mid - strike).abs();
            if distance < STRIKE_PROXIMITY_SNIPER && distance >= 0.0 {
                sniper_mode = true;
            }
        }

        let entry_allowed = if sniper_mode {
            // Sniper: solo necesita dirección definida del micro_price
            rec.binance_micro_price > 0.0 && rec.binance_price > 0.0
                && (rec.binance_micro_price - rec.binance_price).abs() > 0.001
        } else {
            // Normal: necesita desviación mínima + trigger de volumen
            let micro_dev = (rec.binance_micro_price - rec.binance_price).abs();
            let deviation_ok = micro_dev > params.micro_deviation;
            let vol_ok = rec.binance_vol_100ms > params.vol_trigger;
            deviation_ok && vol_ok
        };

        if !entry_allowed {
            return self.idle_result();
        }

        // Determinar dirección: micro_price > binance_price → bullish → BUY
        let side = if rec.binance_micro_price > rec.binance_price {
            SimSide::Buy
        } else {
            SimSide::Sell
        };

        // Precio de entrada: comprar estrictamente al ask, vender al bid
        let entry_price = match side {
            SimSide::Buy => rec.poly_ask,
            SimSide::Sell => rec.poly_bid,
        };

        // Tamaño: 50% del balance, cap 10 contratos
        let max_contracts = (self.balance * 0.5 / entry_price).floor();
        if max_contracts < 1.0 {
            return self.idle_result();
        }
        let size = max_contracts.min(10.0);

        // Comisión de entrada
        let entry_commission = COMMISSION_PER_CONTRACT * size;
        self.balance -= entry_commission;

        self.position = Some(Position {
            side,
            entry_price,
            entry_time_ms: now_ms,
            size_contracts: size,
            market_duration: market_duration_min,
        });

        SimResult {
            status:          "OPEN".to_string(),
            side:            side.as_str().to_string(),
            entry_price,
            exit_price:      0.0,
            pnl_trade:       0.0,
            current_balance: self.balance,
        }
    }

    fn idle_result(&self) -> SimResult {
        SimResult {
            status:          "IDLE".to_string(),
            side:            String::new(),
            entry_price:     0.0,
            exit_price:      0.0,
            pnl_trade:       0.0,
            current_balance: self.balance,
        }
    }
}
