use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use tracing::{info, warn};

use crate::modules::hft::types::{CsvRecord, EventType};

/// Write column descriptions as # comment lines before the header row.
fn write_column_metadata(w: &mut BufWriter<File>) {
    let _ = writeln!(w, "# build_version={} built@{}", env!("GIT_VERSION"), env!("BUILD_TIME"));
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# ZZIGNAL HFT — Session CSV");
    let _ = writeln!(w, "# Estrategia Principal: Odiseo 83 (entry >= 0.83)");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# Descripcion:");
    let _ = writeln!(w, "#   Trading bot HFT sobre Polymarket BTC 15-min (mercados UP/DOWN).");
    let _ = writeln!(w, "#   Opera basado en el order book CLOB de Polymarket + precio BTC");
    let _ = writeln!(w, "#   multi-proveedor (Binance/Coinbase/Kraken).");
    let _ = writeln!(w, "#   Entra LONG en UP cuando poly_mid >= entry_threshold (0.83).");
    let _ = writeln!(w, "#   Entra SHORT en DOWN cuando poly_bid <= (1.0 - entry_threshold)");
    let _ = writeln!(w, "#   es decir cuando poly_ask <= 0.17 (equivalente).");
    let _ = writeln!(w, "#   13 variantes: Odiseo 83 (principal), Wide 65, Odiseo 86-96.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── PARAMETROS DE LA ESTRATEGIA ODISEO 83 ─────────────────────");
    let _ = writeln!(w, "# entry_threshold    = 0.83   (poly_mid minimo para entrada UP;");
    let _ = writeln!(w, "#                              equivalente DOWN: poly_bid <= 0.17)");
    let _ = writeln!(w, "# tp_price           = 0.97   (take-profit, +16.87% desde 0.83)");
    let _ = writeln!(w, "# sl_hard            = 0.81   (stop-loss duro, -2.41% desde 0.83)");
    let _ = writeln!(w, "# sl_trend_delta     = 0.03   (SL por tendencia: si precio cae >3c");
    let _ = writeln!(w, "#                              desde maximo Y BTC velocity < 0)");
    let _ = writeln!(w, "# sl_micro_drop      = 0.30   (SL microestructura: volumen cae >30%");
    let _ = writeln!(w, "#                              vs tick anterior Y imbalance < -0.5)");
    let _ = writeln!(w, "# profit_stop        = 15%    (cierre de sesion si ganancia >=");
    let _ = writeln!(w, "#                              budget * 0.15)");
    let _ = writeln!(w, "# max_sl_per_session = 4      (maximo 4 stop-losses por sesion)");
    let _ = writeln!(w, "# reinvest           = ON     (ganancias se acumulan al budget;");
    let _ = writeln!(w, "#                              budget NUNCA baja, solo sube con");
    let _ = writeln!(w, "#                              ganancias positivas)");
    let _ = writeln!(w, "# budget_base        = $20    (presupuesto base por variante y");
    let _ = writeln!(w, "#                              direccion UP/DOWN)");
    let _ = writeln!(w, "# budget_range       = $5-$100");
    let _ = writeln!(w, "# re_entry           = SI     (puede re-entrar en misma sesion");
    let _ = writeln!(w, "#                              despues de TP o SL)");
    let _ = writeln!(w, "# only_last_10min    = No     (Odiseo 83 opera toda la sesion;");
    let _ = writeln!(w, "#                              Odiseo 94 y 96 solo ultimos 10min)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── CODIGOS DE ESTADO (columna _active) ───────────────────────");
    let _ = writeln!(w, "# 0 = idle/bloqueado  (profit stop o SL limit alcanzados)");
    let _ = writeln!(w, "# 1 = WATCHING        (expectante, esperando senal de entrada)");
    let _ = writeln!(w, "# 2 = ACTIVE          (posicion abierta, en trade)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── CODIGOS DE SALIDA (columna _exit_reason) ──────────────────");
    let _ = writeln!(w, "# 0 = sin salida      (posicion abierta o idle)");
    let _ = writeln!(w, "# 1 = TP              (take-profit: price >= tp_price)");
    let _ = writeln!(w, "# 2 = SL-micro        (microestructura: vol drop >30% + imb < -0.5)");
    let _ = writeln!(w, "# 3 = SL-trend        (tendencia: price cae > sl_trend_delta desde");
    let _ = writeln!(w, "#                      max + BTC velocity < 0)");
    let _ = writeln!(w, "# 4 = SL-hard         (stop-loss duro: price <= sl_hard)");
    let _ = writeln!(w, "# 5 = Session settle  (cierre forzado al final de sesion 15min)");
    let _ = writeln!(w, "# 6 = flash_protection (liquidacion forzada por proteccion de");
    let _ = writeln!(w, "#                       frontera: primer minuto o ultimos 45s.");
    let _ = writeln!(w, "#                       NO cuenta como SL — es venta a mercado)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── PROTECCION DE FRONTERA (v6) ───────────────────────────");
    let _ = writeln!(w, "# Primer minuto (seconds_left > 840): NO se abren posiciones.");
    let _ = writeln!(w, "# Ultimos 45s (seconds_left <= 45):  NO se abren posiciones.");
    let _ = writeln!(w, "# Posiciones abiertas en frontera: liquidacion forzada a");
    let _ = writeln!(w, "# mercado con exit_reason=6. Motivacion: flash dumps en los");
    let _ = writeln!(w, "# ultimos segundos causaron perdidas >80% en sesiones pasadas.");
    let _ = writeln!(w, "# En sesion #895 el mercado murio a T-32s (0 ticks entre");
    let _ = writeln!(w, "# T-20s y T-10s). 45s asegura salida antes del colapso.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── CALCULO DE PnL ───────────────────────────────────────────");
    let _ = writeln!(w, "# PAPER PnL (columna _pnl):");
    let _ = writeln!(w, "#   ACTIVE:  (poly_mid - entry_price) * size");
    let _ = writeln!(w, "#   SETTLED: (exit_price - entry_price) * size");
    let _ = writeln!(w, "#   session_profit = suma de todos los PnL realizados en sesion");
    let _ = writeln!(w, "# LIVE PnL (columnas odiseo83_up_live_pnl / odiseo83_down_live_pnl):");
    let _ = writeln!(w, "#   PnL acumulado de trades REALES ejecutados en Polymarket");
    let _ = writeln!(w, "#   Solo se actualiza cuando live_mode = ON");
    let _ = writeln!(w, "#   Slippage real = paper_pnl - live_pnl (diferencia)");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── COMO ANALIZAR ESTE CSV ───────────────────────────────────");
    let _ = writeln!(w, "# 1. FILTRAR por sesion:");
    let _ = writeln!(w, "#    Cada sesion Polymarket dura 15 minutos.");
    let _ = writeln!(w, "#    Agrupar filas por ts_local en ventanas de 15min.");
    let _ = writeln!(w, "#    El campo session_id relaciona cada fila con su sesion.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 2. IDENTIFICAR trades:");
    let _ = writeln!(w, "#    Transicion odiseo83_up_active: 1->2 = ENTRADA, 2->0 = SALIDA.");
    let _ = writeln!(w, "#    odiseo83_up_exit_reason != 0 indica motivo de salida.");
    let _ = writeln!(w, "#    odiseo_signal [col 271]: 1=UP_entry 2=DOWN_entry 3=ambas.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 3. METRICAS por sesion:");
    let _ = writeln!(w, "#    - PnL neto UP   = sum(odiseo83_up_pnl) donde exit_reason > 0");
    let _ = writeln!(w, "#    - PnL neto DOWN = sum(odiseo83_down_pnl) donde exit_reason > 0");
    let _ = writeln!(w, "#    - PnL total     = PnL UP + PnL DOWN");
    let _ = writeln!(w, "#    - Win rate      = trades con PnL > 0 / total trades");
    let _ = writeln!(w, "#    - Profit factor = sum(PnL > 0) / |sum(PnL < 0)|");
    let _ = writeln!(w, "#    - Avg PnL       = PnL total / numero de trades");
    let _ = writeln!(w, "#    - Max drawdown  = min(odiseo83_up_balance) - budget inicial");
    let _ = writeln!(w, "#    - TP rate       = exits con exit_reason=1 / total exits");
    let _ = writeln!(w, "#    - SL rate       = exits con exit_reason>=2 / total exits");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 4. CORRELACIONES con condiciones de mercado:");
    let _ = writeln!(w, "#    - poly_imbalance [col 16] alto (>0.3) -> mas TP que SL?");
    let _ = writeln!(w, "#    - price_velocity [col 34] positiva -> entradas UP rentables?");
    let _ = writeln!(w, "#    - binance_price [col 5] tendencia -> correlacion con outcome?");
    let _ = writeln!(w, "#    - spoofing_flag [col 38] = 1 -> precede SL-micro?");
    let _ = writeln!(w, "#    - tape_speed_flag [col 39] = 1 -> precede salida rapida?");
    let _ = writeln!(w, "#    - poly_spread [col 13] alto -> peor fill, menor PnL?");
    let _ = writeln!(w, "#    - gap_alert_flag [col 40] = 1 -> salida preventiva?");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 5. OPTIMIZACION de parametros (backtesting):");
    let _ = writeln!(w, "#    - Simular entry_threshold en [0.80, 0.83, 0.86, 0.90]");
    let _ = writeln!(w, "#      y comparar PnL acumulado.");
    let _ = writeln!(w, "#    - Simular tp_price en [0.95, 0.96, 0.97, 0.98, 0.99]");
    let _ = writeln!(w, "#      y evaluar win rate vs profit factor.");
    let _ = writeln!(w, "#    - Evaluar falsos positivos de sl_micro_drop y sl_trend_delta");
    let _ = writeln!(w, "#      (exits que eran TP si no hubiera SL prematuro).");
    let _ = writeln!(w, "#    - Comparar sesiones con reinvest ON vs OFF.");
    let _ = writeln!(w, "#    - Comparar Odiseo 83 (full 15min) vs Odiseo 94/96");
    let _ = writeln!(w, "#      (solo ultimos 10min) en las mismas sesiones.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 6. TRAZABILIDAD LIVE vs PAPER:");
    let _ = writeln!(w, "#    - odiseo83_up_pnl (paper) vs odiseo83_up_live_pnl (real)");
    let _ = writeln!(w, "#    - La diferencia paper - live = slippage + comisiones");
    let _ = writeln!(w, "#    - live_usdc_balance [col 147] = balance USDC real");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 7. ANALISIS MULTI-VARIANTE:");
    let _ = writeln!(w, "#    - Comparar Odiseo 83 vs Odiseo 90 vs Wide 65 en misma sesion");
    let _ = writeln!(w, "#    - Evaluar si entry mas conservador (0.90+) tiene mejor");
    let _ = writeln!(w, "#      accuracy aunque menos trades.");
    let _ = writeln!(w, "#    - Wide 65 (entry >= 0.65) captura reversiones tempranas.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# 8. ESTRUCTURA DE SESION:");
    let _ = writeln!(w, "#    - Sesiones de 15 minutos (ej: 14:30:00 - 14:45:00 UTC).");
    let _ = writeln!(w, "#    - Cada sesion es independiente: budget se resetea al inicio.");
    let _ = writeln!(w, "#    - Con reinvest ON, el budget de la siguiente sesion =");
    let _ = writeln!(w, "#      budget_anterior + PnL_positivo (solo sube, nunca baja).");
    let _ = writeln!(w, "#    - Las columnas odiseo83_* se resetean al inicio de sesion.");
    let _ = writeln!(w, "#    - Las columnas odiseo*_live_pnl son acumuladas globalmente.");
    let _ = writeln!(w, "#");
    let _ = writeln!(w, "# ─── VARIANTES DE ODISEO (todas con TP=0.97) ──────────────────");
    let _ = writeln!(w, "# Odiseo 83 (PRINCIPAL): entry=0.83 sl=0.81 full 15min");
    let _ = writeln!(w, "# Wide 65:              entry=0.65 sl=0.63 tp=0.95 full 15min");
    let _ = writeln!(w, "# Odiseo 86:            entry=0.86 sl=0.84 full 15min");
    let _ = writeln!(w, "# Odiseo 87:            entry=0.87 sl=0.85 full 15min");
    let _ = writeln!(w, "# Odiseo 88:            entry=0.88 sl=0.86 full 15min");
    let _ = writeln!(w, "# Odiseo 89:            entry=0.89 sl=0.87 full 15min");
    let _ = writeln!(w, "# Odiseo 90:            entry=0.90 sl=0.88 full 15min");
    let _ = writeln!(w, "# Odiseo 91:            entry=0.91 sl=0.89 full 15min");
    let _ = writeln!(w, "# Odiseo 92:            entry=0.92 sl=0.90 full 15min");
    let _ = writeln!(w, "# Odiseo 93:            entry=0.93 sl=0.91 full 15min");
    let _ = writeln!(w, "# Odiseo 94:            entry=0.94 sl=0.92 solo ultimos 10min");
    let _ = writeln!(w, "# Odiseo 95:            entry=0.95 sl=0.93 full 15min");
    let _ = writeln!(w, "# Odiseo 96:            entry=0.96 sl=0.95 tp=0.985 solo 10min");
    let _ = writeln!(w, "# ═══════════════════════════════════════════════════════════════");
    let _ = writeln!(w, "# ─── Column Reference (301 columns) ────────────────────────────");
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
    let _ = writeln!(w, "# ─── Point of No Return (last 5 min) ──────────────────────────");
    let _ = writeln!(w, "# [68] pnr_active           = 1 when seconds_left <= 300");
    let _ = writeln!(w, "# [69] pnr_seconds_left     = Seconds until session close");
    let _ = writeln!(w, "# [70] pnr_price            = poly_mid during PNR window");
    let _ = writeln!(w, "# [71] pnr_return_up        = 1.0 - poly_ask (expected UP return)");
    let _ = writeln!(w, "# [72] pnr_return_down      = poly_bid - 0.0 (expected DOWN return)");
    let _ = writeln!(w, "# [73] pnr_volatility_1m    = Liquidity delta as volatility proxy");
    let _ = writeln!(w, "# [74] pnr_confidence       = |poly_mid - 0.5| * 2 (0-1 scale)");
    let _ = writeln!(w, "# [75] pnr_trend            = +1 UP, -1 DOWN, 0 flat");
    let _ = writeln!(w, "# [76] pnr_spread_pct       = spread / mid ratio");
    let _ = writeln!(w, "# ─── Market Pressure ────────────────────────────────────────────");
    let _ = writeln!(w, "# [126] pressure_bid_floor   = lowest bid price with vol >= 10");
    let _ = writeln!(w, "# [127] pressure_ask_ceiling = highest ask price with vol >= 10");
    let _ = writeln!(w, "# [128] pressure_band        = ask_ceiling - bid_floor");
    let _ = writeln!(w, "# [129] pressure_index       = (mid-floor)/band, 0=DOWN 1=UP");
    let _ = writeln!(w, "# [130] pressure_skew        = (bid_vol-ask_vol)/total in band");
    let _ = writeln!(w, "# ─── Odiseo 83 UP ═══ ESTRATEGIA PRINCIPAL ═══════════════════════");
    let _ = writeln!(w, "# [131] odiseo83_up_active     = 0=idle/bloqueado 1=WATCHING(expectante) 2=ACTIVE(en posicion)");
    let _ = writeln!(w, "# [132] odiseo83_up_entry_price= entry fill price");
    let _ = writeln!(w, "# [133] odiseo83_up_size       = contracts comprados (budget/entry_price)");
    let _ = writeln!(w, "# [134] odiseo83_up_pnl        = PnL no realizado / realizado (paper)");
    let _ = writeln!(w, "# [135] odiseo83_up_exit_price = exit fill (0 mientras activo)");
    let _ = writeln!(w, "# [136] odiseo83_up_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard 5=settle");
    let _ = writeln!(w, "# [137] odiseo83_up_balance    = budget + PnL acumulado (paper)");
    let _ = writeln!(w, "# ─── Odiseo 83 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [138] odiseo83_down_active   = 0=idle/bloqueado 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [139] odiseo83_down_entry_price= entry fill price");
    let _ = writeln!(w, "# [140] odiseo83_down_size     = contracts vendidos (budget/entry_price)");
    let _ = writeln!(w, "# [141] odiseo83_down_pnl      = PnL no realizado / realizado (paper)");
    let _ = writeln!(w, "# [142] odiseo83_down_exit_price= exit fill");
    let _ = writeln!(w, "# [143] odiseo83_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard 5=settle");
    let _ = writeln!(w, "# [144] odiseo83_down_balance  = budget + PnL acumulado (paper)");
    let _ = writeln!(w, "# ─── Odiseo 83 LIVE (trazabilidad $$$ real) ──────────────────────");
    let _ = writeln!(w, "# [145] odiseo83_up_live_pnl   = PnL real acumulado trades UP LIVE");
    let _ = writeln!(w, "# [146] odiseo83_down_live_pnl = PnL real acumulado trades DOWN LIVE");
    let _ = writeln!(w, "# [147] live_usdc_balance      = balance USDC real en Polymarket");
    let _ = writeln!(w, "# ─── Odiseo 86 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [148] odiseo86_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [149] odiseo86_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [150] odiseo86_up_size         = contracts bought");
    let _ = writeln!(w, "# [151] odiseo86_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [152] odiseo86_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [153] odiseo86_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [154] odiseo86_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 86 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [155] odiseo86_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [156] odiseo86_down_entry_price= entry fill");
    let _ = writeln!(w, "# [157] odiseo86_down_size       = contracts sold");
    let _ = writeln!(w, "# [158] odiseo86_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [159] odiseo86_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [160] odiseo86_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [161] odiseo86_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 87 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [162] odiseo87_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [163] odiseo87_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [164] odiseo87_up_size         = contracts bought");
    let _ = writeln!(w, "# [165] odiseo87_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [166] odiseo87_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [167] odiseo87_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [168] odiseo87_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 87 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [169] odiseo87_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [170] odiseo87_down_entry_price= entry fill");
    let _ = writeln!(w, "# [171] odiseo87_down_size       = contracts sold");
    let _ = writeln!(w, "# [172] odiseo87_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [173] odiseo87_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [174] odiseo87_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [175] odiseo87_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 88 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [176] odiseo88_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [177] odiseo88_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [178] odiseo88_up_size         = contracts bought");
    let _ = writeln!(w, "# [179] odiseo88_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [180] odiseo88_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [181] odiseo88_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [182] odiseo88_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 88 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [183] odiseo88_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [184] odiseo88_down_entry_price= entry fill");
    let _ = writeln!(w, "# [185] odiseo88_down_size       = contracts sold");
    let _ = writeln!(w, "# [186] odiseo88_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [187] odiseo88_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [188] odiseo88_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [189] odiseo88_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 89 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [190] odiseo89_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [191] odiseo89_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [192] odiseo89_up_size         = contracts bought");
    let _ = writeln!(w, "# [193] odiseo89_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [194] odiseo89_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [195] odiseo89_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [196] odiseo89_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 89 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [197] odiseo89_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [198] odiseo89_down_entry_price= entry fill");
    let _ = writeln!(w, "# [199] odiseo89_down_size       = contracts sold");
    let _ = writeln!(w, "# [200] odiseo89_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [201] odiseo89_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [202] odiseo89_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [203] odiseo89_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 90 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [204] odiseo90_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [205] odiseo90_up_entry_price  = entry fill (poly_ask)");
    let _ = writeln!(w, "# [206] odiseo90_up_size         = contracts bought ($20/entry)");
    let _ = writeln!(w, "# [207] odiseo90_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [208] odiseo90_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [209] odiseo90_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [210] odiseo90_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 90 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [211] odiseo90_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [212] odiseo90_down_entry_price= entry fill (poly_bid)");
    let _ = writeln!(w, "# [213] odiseo90_down_size       = contracts sold ($20/entry)");
    let _ = writeln!(w, "# [214] odiseo90_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [215] odiseo90_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [216] odiseo90_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [217] odiseo90_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 91 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [218] odiseo91_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [219] odiseo91_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [220] odiseo91_up_size         = contracts bought");
    let _ = writeln!(w, "# [221] odiseo91_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [222] odiseo91_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [223] odiseo91_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [224] odiseo91_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 91 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [225] odiseo91_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [226] odiseo91_down_entry_price= entry fill");
    let _ = writeln!(w, "# [227] odiseo91_down_size       = contracts sold");
    let _ = writeln!(w, "# [228] odiseo91_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [229] odiseo91_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [230] odiseo91_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [231] odiseo91_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 92 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [232] odiseo92_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [233] odiseo92_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [234] odiseo92_up_size         = contracts bought");
    let _ = writeln!(w, "# [235] odiseo92_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [236] odiseo92_up_exit_price   = exit fill (0 if active)");
    let _ = writeln!(w, "# [237] odiseo92_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [238] odiseo92_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 92 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [239] odiseo92_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [240] odiseo92_down_entry_price= entry fill");
    let _ = writeln!(w, "# [241] odiseo92_down_size       = contracts sold");
    let _ = writeln!(w, "# [242] odiseo92_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [243] odiseo92_down_exit_price = exit fill (0 if active)");
    let _ = writeln!(w, "# [244] odiseo92_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [245] odiseo92_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 93 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [246] odiseo93_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [247] odiseo93_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [248] odiseo93_up_size         = contracts bought");
    let _ = writeln!(w, "# [249] odiseo93_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [250] odiseo93_up_exit_price   = exit fill");
    let _ = writeln!(w, "# [251] odiseo93_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [252] odiseo93_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 93 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [253] odiseo93_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [254] odiseo93_down_entry_price= entry fill");
    let _ = writeln!(w, "# [255] odiseo93_down_size       = contracts sold");
    let _ = writeln!(w, "# [256] odiseo93_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [257] odiseo93_down_exit_price = exit fill");
    let _ = writeln!(w, "# [258] odiseo93_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [259] odiseo93_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 95 UP ───────────────────────────────────────────────");
    let _ = writeln!(w, "# [260] odiseo95_up_active       = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [261] odiseo95_up_entry_price  = entry fill");
    let _ = writeln!(w, "# [262] odiseo95_up_size         = contracts bought");
    let _ = writeln!(w, "# [263] odiseo95_up_pnl          = unrealized/realized PnL");
    let _ = writeln!(w, "# [264] odiseo95_up_exit_price   = exit fill");
    let _ = writeln!(w, "# [265] odiseo95_up_exit_reason  = 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [266] odiseo95_up_balance      = $20 + cum PnL");
    let _ = writeln!(w, "# ─── Odiseo 95 DOWN ─────────────────────────────────────────────");
    let _ = writeln!(w, "# [267] odiseo95_down_active     = 0=idle 1=WATCHING 2=ACTIVE");
    let _ = writeln!(w, "# [268] odiseo95_down_entry_price= entry fill");
    let _ = writeln!(w, "# [269] odiseo95_down_size       = contracts sold");
    let _ = writeln!(w, "# [270] odiseo95_down_pnl        = unrealized/realized PnL");
    let _ = writeln!(w, "# [271] odiseo95_down_exit_price = exit fill");
    let _ = writeln!(w, "# [272] odiseo95_down_exit_reason= 0=none 1=TP 2=SL-micro 3=SL-trend 4=SL-hard");
    let _ = writeln!(w, "# [273] odiseo95_down_balance    = $20 + cum PnL");
    let _ = writeln!(w, "# [274] odiseo_signal            = 0=none 1=UP_entry 2=DOWN_entry 3=both");
    let _ = writeln!(w, "# ─── Odiseo 94 (solo ultimos 10 min) ────────────────────────────");
    let _ = writeln!(w, "# [275-281] odiseo94_up_*        = active/entry/size/pnl/exit/reason/bal");
    let _ = writeln!(w, "# [282-288] odiseo94_down_*      = active/entry/size/pnl/exit/reason/bal");
    let _ = writeln!(w, "# ─── Odiseo 96 (solo ultimos 10 min) ────────────────────────────");
    let _ = writeln!(w, "# [289-295] odiseo96_up_*        = active/entry/size/pnl/exit/reason/bal");
    let _ = writeln!(w, "# [296-302] odiseo96_down_*      = active/entry/size/pnl/exit/reason/bal");
    let _ = writeln!(w, "# [303] last_trade_up           = last trade price UP token");
    let _ = writeln!(w, "# [304] last_trade_down         = last trade price DOWN token");
    let _ = writeln!(w, "# ────────────────────────────────────────────────────────────────────");

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
