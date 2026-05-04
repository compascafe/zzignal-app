# ZZignal App — Arquitectura v4 (Capas)

> Refactor mayo 2026. Arquitectura en 4 capas con flujo de datos unidireccional.

---

## Visión General

```
┌─────────────────────────────────────────────────────────┐
│                   LAYER 1: DATA                         │
│  poly_orderbook (CLOB depth)  +  binance_feed (BTC)     │
│  ↓↓ datos crudos en memoria, siempre disponibles        │
├─────────────────────────────────────────────────────────┤
│                   LAYER 2: ANALYSIS                     │
│  metrics (mid/spread/imb/velocity)                      │
│  indicators (BB, RSI, MACD, VFI, trend)                 │
│  signals (master_signal, confluence, gap_alert)         │
│  ↓↓ métricas calculadas a partir de los datos           │
├─────────────────────────────────────────────────────────┤
│                   LAYER 3: TRADING                      │
│  engine → fenix, hercules, hydra, pnr, cerbero, imba    │
│  ↓↓ decisiones de entrada/salida (simuladas o reales)   │
├─────────────────────────────────────────────────────────┤
│                   LAYER 4: RECORDING                    │
│  csv_writer → CSV por sesión (125 columnas)             │
│  db_store → PostgreSQL (snapshots, candles, fills)      │
│  rl_feedback → Reinforcement Learning (accuracy → ajuste)│
│  ↓↓ persistencia + aprendizaje                          │
└─────────────────────────────────────────────────────────┘
```

## Estructura de Archivos

```
backend_rust/src/modules/
├── data/                         # LAYER 1 — Raw Market Data
│   ├── mod.rs                    # pub mod poly_orderbook; pub mod binance_feed;
│   ├── poly_orderbook.rs         # Orderbook completo CLOB en memoria (todos los niveles)
│   └── binance_feed.rs           # Precio BTC, volumen, depth de Binance (ring buffer)
│
├── analysis/                     # LAYER 2 — Métricas e Indicadores
│   ├── mod.rs                    # pub mod metrics; pub mod indicators; pub mod signals;
│   ├── metrics.rs                # poly_mid, spread, imbalance, velocity, absorption, gap
│   ├── indicators.rs             # BB(200), RSI(14), MACD(3,10,16), VFI, trend, volatility
│   └── signals.rs                # master_signal, confluence score, gap_alert, fenix_signal
│
├── trading/                      # LAYER 3 — Estrategias y Ejecución
│   ├── mod.rs                    # pub mod engine; pub mod fenix; pub mod hercules; pub mod hydra; pub mod pnr; pub mod cerbero; pub mod imba_liqb;
│   ├── engine.rs                 # TradingEngine: orquesta estrategias + exec (sim/real)
│   ├── fenix.rs                  # Fenix: 5 estrategias range-based, $20 virtual cada una
│   ├── hercules.rs               # Hercules: RL 4-capas + Adaptive Risk Engine + CP feedback
│   ├── hydra.rs                  # Hydra 85 (T-5) + Hydra 90 (T-3) paper trading
│   ├── pnr.rs                    # PNR: Point of No Return — análisis últimos 5 min
│   ├── cerbero.rs                # Cerbero: observación de rangos [70-80], [80-90], [90-98]
│   └── imba_liqb.rs              # Imbalance Divergence + Liquidity Grabbing
│
├── recording/                    # LAYER 4 — Persistencia y Aprendizaje
│   ├── mod.rs                    # pub mod csv_writer; pub mod db_store; pub mod rl_feedback;
│   ├── csv_writer.rs             # Escritura de sesiones CSV (125 columnas, BufWriter 50MB)
│   ├── db_store.rs               # PostgreSQL: candles, fills, snapshots, btc_ticks, sessions
│   └── rl_feedback.rs            # RL Feedback Loop: accuracy → CP widening + parameter tuning
│
├── core/                         # Orquestación (no cambia su rol)
│   ├── mod.rs                    # pub mod worker; pub mod api; pub mod state; pub mod credentials; pub mod persistence;
│   ├── worker.rs                 # WS connections (CLOB + Binance) + message dispatch
│   ├── api.rs                    # REST endpoints + WebSocket /ws handler
│   ├── state.rs                  # AppState: estado central compartido (RwLock + watch channels)
│   ├── credentials.rs            # Polymarket auth (EOA + Proxy wallet)
│   └── persistence.rs            # (legacy) DB queries que no se migraron aún
│
├── db/                           # (legacy) Se migra progresivamente a recording/
│   ├── mod.rs
│   ├── models.rs
│   ├── repository.rs
│   ├── scheduler.rs
│   ├── api.rs
│   └── migrations/
│
├── hft/                          # (legacy) Se descompone en las 4 capas nuevas
│   └── ...
│
└── mod.rs
```

## Flujo de Datos — Tick a Tick

```
1. Binance WebSocket ──► binance_feed.push_tick(price, volume)
                         │
2. CLOB WebSocket ──────► poly_orderbook.push_update(side, bids, asks)
                         │
                         ├──► Layer 1: DATOS CRUDOS EN MEMORIA ◄── permanente
                         │    • poly_orderbook: VecDeque<PolyDepthFrame> (últ. 300 frames/side)
                         │    • binance_feed: PriceRingBuffer<BinanceState> (4096 slots)
                         │
3. Cada tick ───────────► analysis::compute_snapshot(data_layer)
                         │    │
                         │    ├── metrics::calc(): mid, spread, imb, velocity, absorption
                         │    ├── indicators::calc(): BB, RSI, MACD, VFI, trend, vol
                         │    └── signals::evaluate(): master_signal, confluence, gap_alert
                         │
                         ├──► Layer 2: MÉTRICAS CALCULADAS
                         │    • CsvRecord base (columnas 1-61)
                         │
4. Con métricas ────────► trading::engine.on_tick(record, analysis_snapshot)
                         │    │
                         │    ├── fenix::on_tick()    → 5 paper trades
                         │    ├── hercules::evaluate() → RL signal + CP validation
                         │    ├── hydra::on_tick()    → T-5 / T-3 predictions
                         │    ├── pnr::on_tick()      → last 5min analysis
                         │    ├── cerbero::on_tick()  → range observation
                         │    └── imba_liqb::on_tick()→ imbalance + liquidity trades
                         │
                         ├──► Layer 3: DECISIONES TRADING
                         │    • CsvRecord completo (columnas 62-125)
                         │    • Trade events (entry/exit)
                         │
5. Record completo ─────► recording::persist(record, trade_events)
                         │    │
                         │    ├── csv_writer::push()     → session_NNN_hft.csv
                         │    ├── db_store::insert()     → PostgreSQL (hft_snapshots, fills, candles)
                         │    └── rl_feedback::on_trade()→ accuracy tracking → CP widening
                         │
                         └──► Layer 4: PERSISTENCIA + RL
                              • CSV files en disco
                              • PostgreSQL tables
                              • RL feedback loop
```

## AppState — Estado Central

```rust
pub struct AppState {
    // ─── Layer 1: Data ──────────────────────────────────────────────────
    pub poly_orderbook: PolyOrderbook,        // orderbook completo en memoria
    pub binance_feed:   Arc<BinanceFeed>,     // precio + volumen BTC

    // ─── Layer 2: Analysis ──────────────────────────────────────────────
    pub analysis_snap:  RwLock<AnalysisSnapshot>,  // último cómputo

    // ─── Layer 3: Trading ───────────────────────────────────────────────
    pub trading_engine: Arc<TradingEngine>,   // orquesta todas las estrategias

    // ─── Layer 4: Recording ─────────────────────────────────────────────
    pub csv_writer:     Arc<CsvWriter>,       // per-session CSV files
    pub db_store:       Option<PgPool>,       // PostgreSQL (opcional)
    pub rl_feedback:    Arc<RlFeedback>,      // RL feedback loop

    // ─── Core (orquestación) ────────────────────────────────────────────
    pub status:         RwLock<String>,
    pub market:         RwLock<Option<MarketInfo>>,
    pub balance:        RwLock<Option<f64>>,
    pub btc_provider:   RwLock<BtcPriceProvider>,
    pub cmd_tx:         mpsc::UnboundedSender<CmdMsg>,
    pub broadcast_tx:   broadcast::Sender<String>,
    pub interval_arc:   Arc<Mutex<CandleInterval>>,

    // ─── HTTP API state ─────────────────────────────────────────────────
    pub open_orders:    RwLock<Vec<OpenOrder>>,
    pub recent_fills:   RwLock<Vec<RecentFill>>,
    pub candles:        RwLock<Vec<Candle>>,
    pub last_trade_up:  RwLock<Option<f64>>,
    pub last_trade_down:RwLock<Option<f64>>,
    pub btc_open:       RwLock<Option<f64>>,

    // ─── Latency tracking ───────────────────────────────────────────────
    pub latency_binance: RwLock<u64>,
    pub latency_poly:    RwLock<u64>,

    // ─── Legacy (migración progresiva) ──────────────────────────────────
    pub book_up:        RwLock<Option<BookSnapshot>>,
    pub book_down:      RwLock<Option<BookSnapshot>>,
    pub binance_depth:  Arc<RwLock<Option<BinanceDepth>>>,
    pub binance_ring:   Arc<PriceRingBuffer>,
    pub tracking_state: Arc<TrackingState>,
    pub tick_tx:        mpsc::UnboundedSender<BinanceTickEvent>,
    pub mem_hft:        RwLock<Vec<CsvRecord>>,
    pub mem_sessions:   RwLock<Vec<RecordingSession>>,
    pub mem_snapshots:  RwLock<Vec<SessionSnapshot>>,
    pub mem_trades:     RwLock<Vec<SessionTrade>>,
    pub recording_sessions: RwLock<Vec<i32>>,
    pub tick_drain:     Arc<AtomicBool>,
    pub session_manager: Arc<SessionManager>,
    pub strategy_manager: Arc<Mutex<StrategyManager>>,
    pub adaptive_engine: Arc<tokio::sync::Mutex<AdaptiveRiskEngine>>,
    pub macro_ctx:      Arc<RwLock<MacroContext>>,
    pub t5_manager:     Arc<T5Manager>,
    pub t3_manager:     Arc<T3Manager>,
    pub pnr_manager:    Arc<PnrManager>,
    pub insight_manager: Arc<InsightManager>,
    pub fenix_trading:  Arc<FenixTradingManager>,
    pub db:             Option<PgPool>,
}
```

## API Endpoints — Organizados por Capa

### Layer 1: Data
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/depth/latest?side=up&levels=20` | Último snapshot del orderbook |
| GET | `/api/depth/history?side=down&limit=50` | Historial de snapshots |
| GET | `/api/btc` | `{"price": 94321.45}` |
| GET | `/api/btc/provider` | `{"provider": "binance"}` |
| POST | `/api/btc/provider` | Cambiar proveedor BTC |

### Layer 2: Analysis
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/macro` | Macro 24h (RSI, VFI, slope, bias) |
| GET | `/api/candles` | Velas OHLCV sintéticas |

### Layer 3: Trading
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/wisdom` | Hercules engine stats |
| GET | `/api/wisdom2` | Hydra 85 (T-5) |
| GET | `/api/wisdom3` | Hydra 90 (T-3) |
| GET | `/api/wisdom4` | PNR analysis |
| GET | `/api/insights` | Cerbero + Fenix observation |
| GET | `/api/fenix` | Fenix PnL ($20 virtual) |
| GET | `/api/orders` | Órdenes abiertas |
| POST | `/api/orders/limit` | Limit order |
| POST | `/api/orders/market` | Market order |
| DELETE | `/api/orders` | Cancelar todo |

### Layer 4: Recording
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/sessions` | Sesiones grabadas |
| GET | `/api/sessions/active` | Sesión activa |
| GET | `/api/sessions/{id}/export` | Descargar CSV de sesión |
| GET | `/api/sessions/{id}/snapshots` | Snapshots de sesión |
| GET | `/api/analysis/candles` | Candles históricos (DB) |
| GET | `/api/analysis/pnl` | P&L desde fills (DB) |
| GET | `/api/analysis/fills` | Fills históricos (DB) |
| GET | `/api/db/snapshots` | Snapshots de orderbook (DB) |
| GET | `/api/db/executions` | Ejecuciones programadas |

## CSV Output — 125 Columnas

Las columnas se asignan por capa:

| Capa | Columnas | Contenido |
|---|---|---|
| Data | 1-16 | ts, latencia, binance price/vol, poly bid/ask/mid/spread/vol/imb |
| Analysis | 17-61 | trade info, imba/liqb status, velocity, absorption, BB, RSI, MACD, VFI, master_signal, CP |
| Trading | 62-125 | T-5, T-3, PNR, Cerbero, Fenix (trade/entry/pnl/skip/target/exit/signal) |
| Recording | (metadata) | session_id, outcome, btc_start, btc_end, tick_count |

## Convenciones de Código por Capa

### Layer 1 (Data)
- Solo almacena datos crudos, **nunca** computa métricas
- Thread-safe: `RwLock<VecDeque<T>>` o lock-free ring buffer
- Métodos: `push()`, `latest()`, `closest_to_ts()`, `top_n()`
- No depende de ninguna otra capa

### Layer 2 (Analysis)
- Recibe referencias a Layer 1, **nunca** modifica datos crudos
- Produce structs inmutables (`AnalysisSnapshot`)
- Funciones puras siempre que sea posible
- Puede depender de Layer 1, nunca de Layer 3 o 4

### Layer 3 (Trading)
- Recibe `AnalysisSnapshot` + `CsvRecord` parcial
- Toma decisiones (entry/exit) y las registra en el `CsvRecord`
- Estado por sesión (trades abiertos, PnL)
- Puede depender de Layer 1 y 2, nunca de Layer 4

### Layer 4 (Recording)
- Recibe `CsvRecord` completo + eventos de trading
- Escribe a disco/DB de forma asíncrona (nunca bloquea el pipeline)
- RL feedback: lee accuracy histórica → ajusta parámetros en Layer 2/3
- Puede depender de cualquier capa (solo lectura)

## Plan de Migración

1. **Fase 1 (hoy)**: Crear `data/` con `poly_orderbook.rs` y `binance_feed.rs`, integrar en AppState
2. **Fase 2**: Crear `analysis/` extrayendo de `metrics.rs` + `adaptive_risk_engine.rs`
3. **Fase 3**: Crear `trading/` moviendo todas las estrategias
4. **Fase 4**: Crear `recording/` consolidando `session_manager.rs` + `persistence.rs`
5. **Fase 5**: Eliminar `hft/` legacy, limpiar imports, verificar compilación
6. **Fase 6**: Actualizar frontend para nuevos endpoints

---

*Documento generado el 3 Mayo 2026 como parte del refactor v4 de ZZignal.*
