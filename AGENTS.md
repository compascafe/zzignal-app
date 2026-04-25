# ZZignal App — Agent Context

> Documentación orientada a agentes de IA. Si eres un modelo de lenguaje leyendo esto, aquí tienes todo el contexto necesario para trabajar en este proyecto.

---

## 1. Arquitectura General

```
┌─────────────────┐      WebSocket     ┌──────────────────────────┐
│  React Frontend │  ◄──────────────►  │  Rust Backend (Axum)     │
│  (Vite + TW)    │      REST API      │  Port 8080               │
└─────────────────┘                    └──────────────────────────┘
                                                │
                    ┌───────────────────────────┼───────────────────┐
                    │                           │                   │
               Polymarket                 Binance/Coinbase    PostgreSQL
               CLOB WS                    /Kraken WS          (optional)
```

- **Frontend**: React 18 + Vite + Tailwind CSS. Conexión WebSocket nativa.
- **Backend**: Rust (Axum) con worker async en hilo separado.
- **Datos en tiempo real**: WebSocket CLOB (Polymarket) + WebSocket de precio BTC (multi-proveedor).
- **Persistencia**: PostgreSQL opcional (candles, btc_ticks, fills).

---

## 2. Stack Tecnológico

### Backend (`backend_rust/`)

| Componente | Crate | Versión | Notas |
|---|---|---|---|
| Async runtime | `tokio` | 1 | `full` features |
| REST + WS API | `axum` | 0.8 | `ws`, `macros` |
| CORS | `tower-http` | 0.6 | `cors` |
| DB | `sqlx` | 0.8 | PostgreSQL, runtime-tokio |
| Polymarket SDK | `polymarket-client-sdk` | 0.4 | `clob`, `gamma`, `rtds` |
| Ethereum signer | `alloy` | 1.8 | `signer-local`, `signers`. NO subir sin MSRV 1.91+ |
| WebSocket client | `tokio-tungstenite` | 0.29 | `rustls-tls-native-roots` |
| HTTP client | `reqwest` | 0.13 | `rustls`, `json`. Sin native-tls |
| Serialización | `serde` + `serde_json` | 1 | |
| Tiempo | `chrono` | 0.4 | `serde` |
| Errores | `anyhow` | 1 | |
| Logging | `tracing` + `tracing-subscriber` | 0.1 / 0.3 | |
| Credenciales | `dotenvy` + `secrecy` | — | |

**MSRV: Rust 1.91** — impuesto por `polymarket-client-sdk`.

### Frontend (`frontend_react/`)

| Componente | Librería |
|---|---|
| Framework | React 18 + Vite |
| Estilos | Tailwind CSS 3 |
| WebSocket | API nativa del browser |
| Charts | SVG custom (no TradingView aún) |

---

## 3. Estructura de Archivos

```
zzignal_app/
├── AGENTS.md                  ← ESTE ARCHIVO
├── .env                       ← credenciales (NO commitear)
├── .env.example               ← plantilla
├── .gitignore
├── CLAUDE.md                  ← doc original del proyecto
├── build.sh                   ← build del backend Rust
├── .github/workflows/deploy.yml  ← GitHub Actions CI/CD
├── backend_rust/
│   ├── Cargo.toml
│   ├── Cargo.lock
│   └── src/
│       ├── main.rs            ← entry point + consumer AppMsg
│       ├── setup.rs           ← binario auxiliar setup inicial
│       └── modules/
│           ├── mod.rs         ← pub mod core; pub mod db;
│           ├── core/
│           │   ├── mod.rs     ← pub mod worker; pub mod api; pub mod state; pub mod credentials; pub mod persistence;
│           │   ├── worker.rs  ← lógica async: CLOB WS, BTC WS, órdenes
│           │   ├── api.rs     ← rutas REST + handler WS /ws (merge con db::api)
│           │   ├── state.rs   ← AppState (RwLock + watch channels)
│           │   ├── credentials.rs  ← gestión segura de credenciales .env
│           │   └── persistence.rs  ← migraciones + CRUD histórico (candles, fills, btc_ticks)
│           └── db/
│               ├── mod.rs     ← pub mod models; pub mod repository; pub mod scheduler; pub mod api;
│               ├── migrations/
│               │   ├── 001_init.sql
│               │   └── 002_orderbook_executions.sql  ← snapshots + scheduled_executions
│               ├── models.rs  ← OrderBookSnapshotData, ScheduledExecution structs
│               ├── repository.rs  ← Queries CRUD para snapshots y executions
│               ├── scheduler.rs   ← Background task: snapshots cada 10s + executions cada 5s
│               └── api.rs         ← REST endpoints /api/db/* del dashboard db
└── frontend_react/
    ├── package.json
    ├── vite.config.js
    ├── postcss.config.js
    └── src/
        ├── main.jsx, App.jsx, index.css
        ├── hooks/useBackend.js    ← WebSocket hook + estado global
        └── components/
            ├── StatusBar.jsx      ← status, BTC price, balance
            ├── CandleChart.jsx    ← velas SVG custom
            ├── OrderBook.jsx      ← book UP/DOWN dual
            ├── MessageLog.jsx     ← log de mensajes WS
            └── DashboardBD.jsx    ← pestaña BD / Scheduler (snapshots + executions)
```

---

## 4. Variables de Entorno (`.env`)

```env
# PostgreSQL (opcional — si falta, arranca sin persistencia)
DATABASE_URL=postgres://user:pass@host:port/db

# Polymarket — OBLIGATORIAS
POLYMARKET_PRIVATE_KEY=0x...          # hex, con o sin 0x
CLOB_API_KEY=uuid
CLOB_API_SECRET=...
CLOB_API_PASSPHRASE=...
```

La dirección de wallet se **deriva** de `POLYMARKET_PRIVATE_KEY` mediante alloy. Nunca se loguea en texto plano.

---

## 5. API REST (puerto 8080)

### Status & Mercado

| Método | Ruta | Respuesta |
|---|---|---|
| GET | `/api/status` | `{"status": "LIVE"}` |
| GET | `/api/market` | Info del mercado BTC 15-min activo |
| GET | `/api/balance` | `{"balance": 123.45}` |
| GET | `/api/btc` | `{"price": 94321.45, "open": 94200.00}` |
| GET | `/api/btc/provider` | `{"provider": "binance"}` |
| POST | `/api/btc/provider` | `{"provider": "coinbase"}` → cambia proveedor |

### Order Book

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/book/up` | Order book outcome UP |
| GET | `/api/book/down` | Order book outcome DOWN |

### Candles

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/candles` | Velas en memoria (live buffer) |
| POST | `/api/candles/interval` | Cambiar intervalo: `{"interval":"1m"}` |

### Órdenes

| Método | Ruta | Body | Descripción |
|---|---|---|---|
| GET | `/api/orders` | — | Órdenes abiertas |
| POST | `/api/orders/limit` | `{"side":"buy","outcome":"up","price":0.5,"size":10}` | Limit order |
| POST | `/api/orders/market` | `{"side":"buy","outcome":"up","amount_usdc":50}` | Market order |
| POST | `/api/orders/scalp` | `{"outcome":"up","price":0.5,"size":10,"target_price":0.55}` | Scalp buy |
| DELETE | `/api/orders/{id}` | — | Cancelar orden |
| DELETE | `/api/orders` | — | Cancelar todo (CancelMarket) |

### Fills & Análisis

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/fills` | Fills recientes (memoria) |
| GET | `/api/analysis/candles` | Candles históricos desde PostgreSQL |
| GET | `/api/analysis/pnl` | P&L calculado desde fills en DB |
| GET | `/api/analysis/fills` | Fills históricos desde PostgreSQL |

### DB / Scheduler (Dashboard BD)

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/db/snapshots` | Lista de snapshots de order book |
| GET | `/api/db/snapshots/latest` | Último snapshot (por side) |
| GET | `/api/db/executions` | Lista de ejecuciones programadas |
| POST | `/api/db/executions` | Crear ejecución programada |
| DELETE | `/api/db/executions/{id}` | Cancelar/eliminar ejecución |

### WebSocket

| Ruta | Protocolo | Descripción |
|---|---|---|
| `/ws` | WebSocket | Snapshot inicial + broadcast en tiempo real + comandos |

---

## 6. Mensajes WebSocket

### Backend → Frontend (`AppMsg` serializado como JSON)

```typescript
type AppMsg =
  | { type: "snapshot"; status: string; btc: number; balance: number }
  | { type: "status"; status: string }
  | { type: "btc_price"; price: number }
  | { type: "balance"; balance: number }
  | { type: "book"; side: "up" | "down"; book: BookSnapshot }
  | { type: "trade"; side: "up" | "down"; price: number }
  | { type: "open_orders"; orders: OpenOrder[] }
  | { type: "recent_fills"; fills: RecentFill[] }
  | { type: "candles"; interval: string; candles: Candle[] }
  | { type: "candle_update"; candle: Candle }      // update de última vela
  | { type: "order_result"; success: boolean; message: string }
  | { type: "btc_provider"; provider: string }       // broadcast sync
```

### Frontend → Backend (`CmdMsg` como JSON WS)

```typescript
type CmdMsg =
  | { type: "limit"; side: "buy" | "sell"; outcome: "up" | "down"; price: number; size: number }
  | { type: "market"; side: "buy" | "sell"; outcome: "up" | "down"; amount_usdc: number }
  | { type: "scalp"; outcome: "up" | "down"; price: number; size: number; target_price: number }
  | { type: "cancel"; order_id: string }
  | { type: "cancel_all" }
  | { type: "set_interval"; interval: "1s" | "1m" | "5m" | "15m" | "1h" }
  | { type: "set_btc_provider"; provider: "binance" | "coinbase" | "kraken" }
```

---

## 7. Tipos de Datos Clave (Rust)

```rust
pub enum OrderSide { Buy, Sell }
pub enum Outcome   { Up, Down }

pub enum BtcPriceProvider {
    Binance,   // wss://stream.binance.com:9443/ws/btcusdt@aggTrade
    Coinbase,  // wss://advanced-trade-ws.coinbase.com (channel: ticker)
    Kraken,    // wss://ws.kraken.com (channel: ticker)
}

pub struct Candle {
    pub open_time: i64,  // Unix ms
    pub open: f64, pub high: f64, pub low: f64, pub close: f64, pub volume: f64,
}

pub struct BookSnapshot { pub bids: Vec<PriceLevel>, pub asks: Vec<PriceLevel> }
pub struct PriceLevel { pub price: f64, pub size: f64 }

pub struct OpenOrder {
    pub id: String, pub outcome: String, pub side: OrderSide,
    pub price: f64, pub size_orig: f64, pub size_matched: f64,
}

pub struct RecentFill {
    pub outcome: String, pub side: OrderSide,
    pub price: f64, pub size: f64,
    pub time: String,    // "14:32:07"
    pub session: String, // "14:30" — inicio de sesión 15-min
}
```

---

## 8. Multi-Proveedor de Precio BTC

### Decisión de diseño
Binance bloquea IPs de EE.UU. Por eso el backend soporta **3 proveedores** seleccionables en caliente:

| Proveedor | WebSocket URL | Volumen real | Requiere suscripción |
|---|---|---|---|
| **Binance** | `wss://stream.binance.com:9443/ws/btcusdt@aggTrade` | ✅ Campo `q` | ❌ No |
| **Coinbase** | `wss://advanced-trade-ws.coinbase.com` | ❌ No | ✅ `{"type":"subscribe","product_ids":["BTC-USD"],"channel":"ticker"}` |
| **Kraken** | `wss://ws.kraken.com` | ❌ No | ✅ `{"event":"subscribe","pair":["BTC/USD"],"subscription":{"name":"ticker"}}` |

### Cambio en caliente
- El frontend envía WS: `{"type":"set_btc_provider","provider":"coinbase"}`
- El backend actualiza `AppState.btc_provider` y notifica al worker vía `tokio::sync::watch::Sender`
- El worker recibe la notificación en `run_btc_price_stream()`, corta la conexión WS actual (con `tokio::select!`) y se reconecta al nuevo proveedor
- Se hace broadcast a todos los clientes conectados: `{"type":"btc_provider","provider":"coinbase"}`

### Velas sintéticas
Como las velas originales de Binance REST/klines también pueden bloquearse, el backend genera **velas OHLCV sintéticas** a partir de cada tick de precio:

```
Tick WS (cada ~1s)
    │
    ├──► Precio BTC ──► UI (StatusBar)
    │
    └──► TickCandleGenerator
            ├──► Actualiza vela actual (high/low/close)
            ├──► Si cambia de período → cierra vela anterior, empieza nueva
            └──► Envía CandleUpdate al UI
```

- **Volumen**: real de Binance (campo `q` del aggTrade), `0.0` para Coinbase/Kraken
- **Intervalos soportados**: 1s, 1m, 5m, 15m, 1h
- **Independiente del proveedor**: funciona con cualquiera de los 3

---

## 9. Arquitectura del Backend

### Flujo de mensajes

```
cargo run
    │
    ├── hilo OS ──► tokio runtime ──► worker::run(tx, creds, cmd_rx, interval_arc, broadcast_tx, provider_rx)
    │                                       │
    │                                       ├── run_cycle()
    │                                       │     ├── auth CLOB (EOA + Proxy wallet)
    │                                       │     ├── discover_btc_market() [Gamma API]
    │                                       │     ├── fetch initial snapshots
    │                                       │     ├── spawn: run_btc_price_stream() [multi-proveedor WS]
    │                                       │     ├── spawn: run_candle_stream() [Binance REST+WS fallback]
    │                                       │     └── run_live() [CLOB WS + cmd loop]
    │                                       │           └── select! { WS msg | CmdMsg | 5s timer }
    │                                       │
    │                                       └── on error: reconnect con backoff exponencial
    │
    ├── task: consume AppMsg (via std::mpsc → tokio::mpsc bridge)
    │     ├── actualiza AppState (RwLock fields)
    │     ├── persiste en PostgreSQL (candles, fills, btc_ticks)
    │     └── broadcast JSON → clientes WS
    │
    └── axum server  http://0.0.0.0:8080
            ├── /api/*  REST endpoints
            └── /ws     WebSocket (snapshot + live broadcast + recibe CmdMsg JSON)
```

### Autenticación Polymarket

El SDK soporta EOA y Proxy Wallet:

```
1. Gamma API: PublicProfileRequest(address=EOA) → obtener proxy_wallet
2. Si proxy_wallet existe:
   Client::new(...).authentication_builder(&signer)
       .credentials(l2_creds)
       .funder(proxy_wallet)
       .signature_type(SignatureType::Proxy)
       .authenticate().await
3. Si no hay proxy_wallet: flujo normal EOA
```

El saldo USDC real está bajo el proxy wallet, no el EOA. Sin `funder` correcto, `balance_allowance()` devuelve 0.

---

## 10. Cómo Correr en Local

### Requisitos
- Rust 1.91+
- Node.js 18+
- PostgreSQL 14+ (opcional)

### 1. Configurar credenciales
```bash
cp .env.example .env
# Editar .env con tus credenciales de Polymarket
```

### 2. Backend
```bash
cd backend_rust
cargo run           # dev
cargo run --release # release
```

### 3. Frontend
```bash
cd frontend_react
npm install
cat > .env << 'EOF'
VITE_WS_URL=ws://localhost:8080/ws
VITE_API_URL=http://localhost:8080
EOF
npm run dev
```

Abre http://localhost:5173

---

## 11. CI/CD — GitHub Actions

Archivo: `.github/workflows/deploy.yml`

- **Trigger**: push a `main`
- **Frontend job**: compila React, sincroniza `dist/` a EC2 via SSH
- **Backend job**: compila Rust release con cache, empaqueta, despliega y reinicia systemd

### Secrets requeridos
- `SERVER_IP` — IP pública EC2
- `SERVER_SSH_KEY` — clave privada SSH
- `ENV_FILE` — contenido de `.env` en base64

### Caching optimizado (abril 2026)
- `~/.cargo/registry` — dependencias del registry
- `backend_rust/target/release/{deps,build,.fingerprint}` — compilación incremental
- `restore-keys` como fallback para reusar cache aunque cambie `Cargo.lock`
- `CARGO_INCREMENTAL=1` habilitado

---

## 12. Estado Actual de Features

### Backend — Funcionando ✅
- [x] Auth CLOB (EOA + Proxy wallet)
- [x] Descubrimiento mercado BTC 15-min (Gamma API)
- [x] Order book en tiempo real (CLOB WS)
- [x] Precio BTC multi-proveedor (Binance/Coinbase/Kraken)
- [x] Velas sintéticas OHLCV desde ticks
- [x] Órdenes: limit, market, scalp
- [x] Cancelación individual y de mercado
- [x] Historial de fills con sesión 15-min
- [x] Reconexión automática + backoff
- [x] API REST completa + WebSocket `/ws`
- [x] PostgreSQL: candles, btc_ticks, fills
- [x] Análisis DB: candles históricos, P&L
- [x] **Módulo DB modular**: snapshots de order book + scheduler de ejecuciones

### Frontend — Parcialmente implementado ⚠️
- [x] Conexión WebSocket con reconexión
- [x] StatusBar: BTC price, balance, status
- [x] CandleChart: SVG custom con toggle show/hide
- [x] OrderBook dual UP/DOWN
- [x] Selector de proveedor BTC (dropdown)
- [x] Órdenes abiertas + fills
- [x] Botón PANIC (cancelar todo)
- [x] **Dashboard BD / Scheduler** (snapshots + ejecuciones programadas)
- [ ] **Panel de trading** (inputs para limit/market/scalp)
- [ ] **Visualización de P&L**
- [ ] TradingView Lightweight Charts (actualmente SVG custom)

---

## 13. Convenciones de Código

### Rust
- Usar `anyhow::Result` para errores en funciones públicas
- Logging con `tracing::{info, warn, error}` — nunca `println!`
- Credenciales: nunca loguear `private_key` o `.env` completos
- WebSocket handlers: usar `tokio::select!` para detectar cambios + mensajes
- Watch channels (`tokio::sync::watch`) para comunicación one-to-many entre tareas

### React
- Hook `useBackend` centraliza toda la lógica WS
- Componentes funcionales, props drilling mínimo
- Tailwind con clases arbitrarias `bg-[#0a0e14]` para dark theme
- WebSocket URL: `import.meta.env.VITE_WS_URL` con fallback a producción

---

## 14. Decisiones de Diseño Recientes

| Fecha | Decisión | Razón |
|---|---|---|
| Abril 2026 | Multi-proveedor BTC | Binance bloquea IPs de US |
| Abril 2026 | Velas sintéticas | No depender de Binance REST/klines |
| Abril 2026 | Toggle chart | Gráfico SVG custom aún no es óptimo |
| Abril 2026 | Cleanup `.gitignore` | Eliminar `target/` y `node_modules/` trackeados |
| Abril 2026 | Backend modular `modules/core/` + `modules/db/` | Escalabilidad: separar lógica principal de módulos auxiliares |
| Abril 2026 | Migración `002_orderbook_executions.sql` | Tablas para snapshots y ejecuciones programadas |
| Abril 2026 | Scheduler async en background | Snapshots cada 10s + ejecuciones pendientes cada 5s |

---

## 15. Comandos Útiles

```bash
# Backend
cd backend_rust && cargo build           # dev
cd backend_rust && cargo run --release   # release
cd backend_rust && cargo check           # verificación rápida
RUST_LOG=polymarket_dashboard=debug cargo run  # logs detallados

# Frontend
cd frontend_react && npm install && npm run dev

# Setup inicial de credenciales
cd backend_rust && cargo run --bin setup
```

---

## 16. Contacto / Contexto

Este archivo fue creado en abril de 2026 tras implementar:
1. Selector de proveedor BTC (multi-proveedor)
2. Velas sintéticas OHLCV
3. Toggle del gráfico
4. Optimización de caching CI/CD
5. Limpieza de artefactos trackeados en git
6. Módulo DB modular con snapshots de order book y scheduler de ejecuciones programadas

Si vas a modificar algo, revisa primero `backend_rust/src/modules/core/worker.rs` (lógica del worker) y `frontend_react/src/hooks/useBackend.js` (estado del frontend).

¡Happy coding! 🤖
