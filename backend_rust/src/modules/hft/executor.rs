// ─── Paper Trading Executor ──────────────────────────────────────────────────
// Simula operaciones de trading con $20 USD de capital virtual.
//
// Parámetros por estacionalidad (según duration_min del mercado):
//
//   5-min (Sniper):
//     - Trigger volumen: binance_vol_100ms > 2.0
//     - TP: $0.012 por contrato o 10 segundos
//     - Compra: poly_ask, Venta: poly_bid
//
//   15-min (Arbitraje):
//     - Trigger volumen: binance_vol_100ms > 1.2
//     - TP: $0.025 por contrato o 30 segundos
//     - Compra: poly_ask, Venta: poly_bid
//
// Común:
//   - Comisión: $0.001 por contrato por trade
//   - Si micro_price se desvía >$1.50 de binance_price → disparar
//   - Si distancia a Strike < $1.00 → ignorar filtro de volumen (agresividad máxima)
//   - Capital inicial: $20.00

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
    market_duration: i32, // 5 o 15
}

/// Parámetros por estacionalidad
struct SeasonParams {
    vol_trigger:      f64, // binance_vol_100ms threshold
    tp_per_contract:  f64, // take-profit USD por contrato
    tp_timeout_ms:    i64, // take-profit timeout en ms
}

fn params_for(duration_min: i32) -> SeasonParams {
    match duration_min {
        5 => SeasonParams {
            // Sniper: agresivo, volumen alto, TP rápido
            vol_trigger:     2.0,
            tp_per_contract: 0.012,
            tp_timeout_ms:   10_000,
        },
        _ => SeasonParams {
            // Arbitraje (15-min default): moderado
            vol_trigger:     1.2,
            tp_per_contract: 0.025,
            tp_timeout_ms:   30_000,
        },
    }
}

// Constantes comunes
const COMMISSION_PER_CONTRACT: f64 = 0.001;
const INITIAL_CAPITAL: f64 = 20.00;
const MICRO_DEVIATION_THRESHOLD: f64 = 1.50;
const STRIKE_PROXIMITY_THRESHOLD: f64 = 1.00;

/// Paper Trading Executor — evalúa cada fila del CSV y decide si abrir/cerrar posición.
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
    /// Devuelve los campos `sim_*` que deben adjuntarse al registro.
    /// El caller debe mutar el CsvRecord con estos valores.
    pub fn evaluate(
        &mut self,
        rec: &CsvRecord,
        strike_price: Option<f64>,
        market_duration_min: i32,
    ) -> SimResult {
        let now_ms = rec.ts_exchange.parse::<i64>().unwrap_or(0);
        let params = params_for(market_duration_min);

        // Si hay posición abierta → evaluar cierre
        if let Some(ref pos) = self.position {
            let exit_price = match pos.side {
                SimSide::Buy => rec.poly_bid,  // vender al bid
                SimSide::Sell => rec.poly_ask, // comprar al ask (cerrar short)
            };
            if exit_price <= 0.0 {
                return self.idle_result();
            }

            let pnl_per_contract = match pos.side {
                SimSide::Buy => exit_price - pos.entry_price,
                SimSide::Sell => pos.entry_price - exit_price,
            };
            let gross_pnl = pnl_per_contract * pos.size_contracts;

            // Comisión de salida
            let commission = COMMISSION_PER_CONTRACT * pos.size_contracts;

            // Condiciones de cierre (TP o timeout)
            let tp_hit = gross_pnl >= params.tp_per_contract * pos.size_contracts;
            let timed_out = now_ms - pos.entry_time_ms >= params.tp_timeout_ms;

            if tp_hit || timed_out {
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

            // Posición sigue abierta
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

        // Necesitamos poly_bid y poly_ask para operar
        if rec.poly_bid <= 0.0 || rec.poly_ask <= 0.0 {
            return self.idle_result();
        }

        // 1. Proximidad al strike: agresividad máxima (ignora filtro de volumen)
        let mut proximity_override = false;
        if let Some(strike) = strike_price {
            let distance_to_strike = (rec.poly_mid - strike).abs();
            if distance_to_strike < STRIKE_PROXIMITY_THRESHOLD {
                proximity_override = true;
            }
        }

        // 2. Desviación del micro_price
        let micro_deviation = (rec.binance_micro_price - rec.binance_price).abs();
        let deviation_trigger = micro_deviation > MICRO_DEVIATION_THRESHOLD;

        // 3. Trigger de volumen (se salta si proximity_override)
        let vol_trigger = proximity_override || rec.binance_vol_100ms > params.vol_trigger;

        // Decisión: entrar si hay desviación + (volumen o proximity)
        if !deviation_trigger || !vol_trigger {
            return self.idle_result();
        }

        // Determinar dirección: si micro_price > binance_price → bullish → BUY (UP)
        //                       si micro_price < binance_price → bearish → SELL (DOWN)
        let side = if rec.binance_micro_price > rec.binance_price {
            SimSide::Buy
        } else {
            SimSide::Sell
        };

        // Precio de entrada: comprar al ask, vender al bid
        let entry_price = match side {
            SimSide::Buy => rec.poly_ask,
            SimSide::Sell => rec.poly_bid,
        };

        // Tamaño: usar 50% del balance
        let max_contracts = (self.balance * 0.5 / entry_price).floor();
        if max_contracts < 1.0 {
            return self.idle_result();
        }
        let size = max_contracts.min(10.0); // cap 10 contratos

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
