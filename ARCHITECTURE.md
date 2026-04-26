# ZZignal App — Architecture & Module Design

> **Audiencia**: LLMs y desarrolladores que necesiten entender, mantener o extender este proyecto.
> **Última actualización**: 2026-04-25

---

## 1. Visión del Producto

Cliente de Polymarket para **High Frequency Trading** en mercados de predicción BTC.

| Capa | Licencia | Qué incluye |
|---|---|---|
| **Core** | Open Source (MIT) | Conexión CLOB, auth, APIs REST/WS, UI básica |
| **Premium** | Propietario (pago) | Recolección multi-timeframe, detección de patrones, auto-ejecución, dashboard ejecutivo |

---

## 2. Estructura de Módulos

```
backend_rust/src/modules/
├── core/                        ← OPEN SOURCE
│   ├── mod.rs                   ← pub mod worker, api, state, credentials, persistence
│   ├── worker.rs                ← Conexión CLOB WS + BTC WS + órdenes (1600 líneas, pendiente de split)
│   ├── api.rs                   ← Router REST + WS /ws + merge de routers premium
│   ├── state.rs                 ← AppState: memoria compartida (RwLock, watch channels, broadcast)
│   ├── credentials.rs           ← Auth Polymarket (EOA + Proxy Wallet)
│   └── persistence.rs           ← CRUD candles, fills, btc_ticks en PostgreSQL
│
├── db/                          ← PROPIETARIO (módulo "Database & Scheduler")
│   ├── mod.rs
│   ├── models.rs                ← OrderBookSnapshotData, ScheduledExecution, RecordingSession, SessionSnapshot, SessionTrade
│   ├── repository.rs            ← CRUD dual: PostgreSQL + in-memory fallback
│   ├── scheduler.rs             ← Timers: snapshots 10s, executions 5s, sessions 1s
│   ├── api.rs                   ← Routers: /api/db/* + /api/sessions/*
│   └── migrations/              ← 001..005 (ver CHANGELOG.md)
│
└── mod.rs                       ← pub mod core; pub mod db;
```

---

## 3. Sistema de Feature Flags (para vender módulos)

### Cargo.toml

```toml
[features]
default = []
premium-collector = []             # Módulo 1: Recolección multi-timeframe
premium-patterns = ["premium-collector"]  # Módulo 2: Detección de patrones
premium-executor = ["premium-patterns"]   # Módulo 3: Auto-ejecución
premium-all = ["premium-collector", "premium-patterns", "premium-executor"]
```

### Cómo compilar

```bash
# Solo open source (Core)
cargo build --release

# Con módulo Collector
cargo build --release --features premium-collector

# Con todos los módulos premium
cargo build --release --features premium-all
```

### Cómo funciona el gating

Cada módulo premium expone un router que se mergea condicionalmente en `core/api.rs`:

```rust
// En api.rs
#[cfg(feature = "premium-collector")]
let collector_r = premium::collector::api::router(state.clone());
let app = core.merge(db_r).merge(session_r);
#[cfg(feature = "premium-collector")]
let app = app.merge(collector_r);
```

### Cómo vender módulos individualmente

1. El código premium vive en **este mismo repo** pero detrás de feature flags
2. Al compilar para un cliente, se usan las flags correspondientes a los módulos que compró
3. Cada módulo tiene su propio sistema de licencia en `premium/license.rs`
4. La licencia se valida al iniciar el módulo (runtime check)

**Modelo de distribución**:
- Opción A: El cliente recibe un binario compilado con sus módulos
- Opción B: El cliente recibe acceso al repo privado con feature flags (más flexible)

---

## 4. Estado Actual de Features

### Core (Open Source) — COMPLETO ✅
- [x] Auth CLOB (EOA + Proxy Wallet)
- [x] Ordenes: limit, market, scalp, cancel
- [x] Order book en tiempo real (CLOB WS)
- [x] Precio BTC multi-proveedor (Binance/Coinbase/Kraken)
- [x] Velas sintéticas OHLCV
- [x] API REST completa + WebSocket /ws
- [x] PostgreSQL: candles, btc_ticks, fills
- [x] Frontend React con StatusBar, OrderBook, CandleChart, MessageLog

### DB Module — COMPLETO ✅
- [x] Snapshots de order book (10s)
- [x] Ejecuciones programadas (5s)
- [x] Session Recorder: grabación tick-by-tick
- [x] Sesiones programadas con inicio/parada automática
- [x] Export CSV/JSON
- [x] Dual DB + in-memory fallback
- [x] Frontend SessionsPanel con gráfico SVG

### Módulos Premium — PENDIENTE 🚧
- [ ] `premium/collector` — Recolección multi-timeframe (5m, 15m, 1h, 4h, 1d)
- [ ] `premium/patterns` — Detección de patrones en order book
- [ ] `premium/executor` — Engine de estrategias + auto-ejecución
- [ ] `premium/license` — Sistema de licencias por módulo
- [ ] `ExecutiveDashboard.jsx` — Dashboard ejecutivo para estrategias

---

## 5. API Endpoints

### Core (siempre disponibles)
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/status` | Estado de conexión |
| GET | `/api/market` | Info del mercado BTC |
| GET | `/api/balance` | Saldo USDC |
| GET | `/api/btc` | Precio BTC actual |
| GET/POST | `/api/btc/provider` | Seleccionar proveedor BTC |
| GET | `/api/book/up`, `/api/book/down` | Order book |
| GET/POST | `/api/candles`, `/api/candles/interval` | Velas |
| GET/POST/DELETE | `/api/orders/*` | Órdenes |
| GET | `/api/fills` | Fills recientes |
| GET | `/api/analysis/*` | Análisis histórico |
| WS | `/ws` | WebSocket en tiempo real |

### DB / Scheduler
| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/db/snapshots` | Snapshots de order book |
| POST | `/api/db/snapshots/test` | Forzar snapshot |
| GET/POST/DELETE | `/api/db/executions/*` | Ejecuciones programadas |
| GET | `/api/sessions` | Listar sesiones de grabación |
| GET | `/api/sessions/active` | Sesión grabando actualmente |
| POST | `/api/sessions/start` | Crear nueva sesión |
| POST | `/api/sessions/{id}/stop` | Detener sesión |
| DELETE | `/api/sessions/{id}` | Eliminar sesión |
| GET | `/api/sessions/{id}/snapshots` | Datos de sesión |
| GET | `/api/sessions/{id}/trades` | Trades de sesión |
| GET | `/api/sessions/{id}/export` | Exportar CSV/JSON |

---

## 6. Flujo de Datos — Session Recorder

```
1. Frontend → POST /api/sessions/start → crea sesión "scheduled"
2. Scheduler (cada 1s) → process_sessions()
   ├── get_sessions_to_start() → inicia 5s antes de scheduled_start
   │   ├── recording_session = Some(id)  ← flag en memoria (ANTES del DB update)
   │   └── start_session_recording() → status='recording', strike_price=BTC
   └── get_sessions_to_stop() → detiene en scheduled_end
       ├── stop_session() → status='completed', final_price=BTC, outcome
       └── recording_session = None
3. Cada book update (CLOB WS) → capture_book()
   ├── Verifica recording_session
   └── insert_session_snapshot() → DB o memoria
4. Cada batch de fills (5s poll) → capture_fills()
   ├── Verifica recording_session
   └── insert_session_trade() → ON CONFLICT DO NOTHING (dedup)
```

---

## 7. Decisiones de Diseño Clave

| Fecha | Decisión | Razón |
|---|---|---|
| Abr 2026 | Arquitectura modular `core/` + `db/` + `premium/` | Separar open source de propietario |
| Abr 2026 | Repository dual (DB + memoria) | Funciona sin PostgreSQL, simplifica desarrollo local |
| Abr 2026 | `&AppState` en vez de `Option<&PgPool>` en repository | Permite acceso a DB y buffers de memoria en una sola firma |
| Abr 2026 | Migración 005 idempotente (`DO $$ IF NOT EXISTS`) | Repara schemas rotos sin destruir datos existentes |
| Abr 2026 | `ON CONFLICT DO NOTHING` en session_trades | Evita duplicación de fills (se enviaban cada 5s sin dedup) |
| Abr 2026 | Feature flags en Cargo.toml | Cada módulo premium se compila condicionalmente |
| Abr 2026 | recording_session flag se setea ANTES del DB update | Evita perder ~1s de datos al inicio de sesión |

---

## 8. Bugs Conocidos Corregidos

| Bug | Severidad | Fix |
|---|---|---|
| Fill duplication (180x por sesión) | CRITICAL | `ON CONFLICT DO NOTHING` + unique index |
| Migration 004 falla en DB frescos | CRITICAL | Migration 005 idempotente repara todo |
| ~1s data loss al iniciar sesión | MAJOR | Flag se setea antes del DB update |
| Stale BTC price en batch de fills | MAJOR | Se lee BTC price por cada fill individual |
| Deadlock in-memory (lock ordering) | MODERATE | Locks se liberan antes de adquirir el siguiente |
| `warn!` en vez de `error!` | LOW | Pendiente de revisar |

---

## 9. Pendientes / Siguientes Pasos

1. **Implementar `premium/collector`** — Agregaciones multi-timeframe del order book
2. **Optimizar Core** — Split worker.rs (1600 líneas), reducir lock contention
3. **Implementar `premium/patterns`** — Detección: walls, spread anomaly, imbalance
4. **Implementar `premium/executor`** — Rule engine + auto-ejecución
5. **Frontend ExecutiveDashboard** — UI para estrategias
6. **Exportar Parquet** — Para datasets de HFT
7. **Migrar `recording_session` a `Vec<i32>`** — Soporte multi-sesión concurrente

---

## 10. Cómo Seguir Implementando (Guía para LLMs)

1. Leer este archivo primero
2. Leer `AGENTS.md` para detalles de stack y convenciones
3. Leer `SESSION_LOG.md` para el historial de cambios de la sesión actual
4. Los módulos premium van en `backend_rust/src/modules/premium/`
5. Cada módulo premium tiene: `models.rs`, `repository.rs`, `scheduler.rs`, `api.rs`
6. Usar feature flags de Cargo.toml para compilación condicional
7. El frontend premium va en `frontend_react/src/components/premium/`
8. Siempre correr `cargo check` y `npm run build` después de cambios
9. Documentar cambios en `ARCHITECTURE.md` y crear CHANGELOG entries
