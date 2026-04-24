# Polymarket BTC 15-min Dashboard — Guía para LLMs

Proyecto de trading en tiempo real sobre Polymarket (mercados de predicción BTC 15-min).
Arquitectura: **backend Rust** (worker asíncrono + WebSocket server) + **frontend React**.

---

## Stack tecnológico

### Backend (Rust — `backend_rust/`)

| Componente | Crate | Versión |
|---|---|---|
| Async runtime | `tokio` | 1 |
| REST + WebSocket API | `axum` | 0.8 |
| CORS middleware | `tower-http` | 0.6 |
| Base de datos | `sqlx` (PostgreSQL) | 0.8 |
| Polymarket SDK | `polymarket-client-sdk` | 0.4 |
| Ethereum signer | `alloy` | =1.6.3 (pinado) |
| WebSocket client | `tokio-tungstenite` | 0.29 |
| Serialización | `serde` + `serde_json` | 1 |
| HTTP client | `reqwest` | 0.13 (rustls, sin native-tls) |
| Tiempo | `chrono` | 0.4 |
| Credenciales | `dotenvy` + `secrecy` | — |
| Errores | `anyhow` | 1 |
| Logging | `tracing` + `tracing-subscriber` | 0.1/0.3 |

**MSRV: Rust 1.88** — impuesto por `polymarket-client-sdk`. No actualizar `alloy` más allá de `=1.6.3` sin subir el compilador a 1.91+.

### Frontend (React — `frontend_react/`) — pendiente de implementar

| Componente | Librería |
|---|---|
| Framework | React + Vite |
| Estilos | Tailwind CSS |
| WebSocket | API nativa del browser (`/ws`) |
| Charts | TradingView Lightweight Charts o Recharts |

---

## Estructura de archivos

```
dashboard_poly/
├── CLAUDE.md                  ← este archivo
├── .env                       ← credenciales (no commitear)
├── .env.example               ← plantilla de variables de entorno
├── build.sh                   ← build del backend Rust
├── backend_rust/
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── migrations/
│   │   └── 001_init.sql       ← schema PostgreSQL (candles, btc_ticks, fills)
│   └── src/
│       ├── main.rs            ← entry point: axum server + consumer de AppMsg
│       ├── state.rs           ← AppState compartido (RwLock fields + DB pool)
│       ├── api.rs             ← todas las rutas REST + handler WebSocket /ws
│       ├── db.rs              ← migraciones + CRUD + queries de análisis
│       ├── worker.rs          ← lógica asíncrona: CLOB WS, Binance, órdenes
│       ├── credentials.rs     ← carga y gestión de credenciales desde .env
│       └── setup.rs           ← binario auxiliar para configuración inicial
└── frontend_react/            ← pendiente — React + Tailwind
```

---

## Variables de entorno requeridas (`.env`)

```env
POLYMARKET_PRIVATE_KEY=0x...   # Clave privada Ethereum (hex, con o sin 0x)
CLOB_API_KEY=...               # UUID — API key L2 del CLOB
CLOB_API_SECRET=...            # API secret L2
CLOB_API_PASSPHRASE=...        # Passphrase L2
```

La dirección de wallet se **deriva** de la clave privada mediante alloy. Nunca se almacena/loguea en texto plano.

---

## Arquitectura general

```
cargo run
    │
    ├── hilo OS ──► tokio runtime ──► worker::run(broadcast_tx)
    │                                       │
    │                                       ├── run_cycle()
    │                                       │     ├── auth CLOB
    │                                       │     ├── discover_btc_market()
    │                                       │     ├── fetch initial snapshots
    │                                       │     ├── spawn: run_btc_price_stream() [Binance WS]
    │                                       │     ├── spawn: run_candle_stream()    [Binance REST+WS]
    │                                       │     └── run_live()  [CLOB WS + cmd loop]
    │                                       │           └── select! { WS msg | CmdMsg | 5s timer }
    │                                       │
    │                                       └── on error: reconnect con backoff exponencial
    │
    ├── task: consume AppMsg
    │     ├── actualiza AppState (RwLock fields)
    │     ├── persiste en PostgreSQL (candles, fills, btc_ticks)
    │     └── broadcast JSON → clientes WS
    │
    └── axum server  http://0.0.0.0:3000
            ├── /api/*  REST endpoints
            └── /ws     WebSocket (snapshot + live broadcast + recibe CmdMsg JSON)
                    │
                    └── React frontend (browser)
```

## API REST (puerto 3000)

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/status` | Estado de conexión del worker |
| GET | `/api/market` | Info del mercado BTC 15-min activo |
| GET | `/api/balance` | Saldo USDC |
| GET | `/api/btc` | Precio BTC actual y precio de apertura |
| GET | `/api/book/up` | Order book outcome UP |
| GET | `/api/book/down` | Order book outcome DOWN |
| GET | `/api/candles` | Velas en memoria (live buffer) |
| POST | `/api/candles/interval` | Cambiar intervalo: `{"interval":"1m"}` |
| GET | `/api/orders` | Órdenes abiertas |
| POST | `/api/orders/limit` | Colocar limit order |
| POST | `/api/orders/market` | Colocar market order |
| POST | `/api/orders/scalp` | Scalp buy (auto-sell al fill) |
| DELETE | `/api/orders/{id}` | Cancelar orden específica |
| DELETE | `/api/orders` | Cancelar todas (CancelMarket) |
| GET | `/api/fills` | Fills recientes (memoria) |
| GET | `/api/analysis/candles` | Candles históricos desde PostgreSQL |
| GET | `/api/analysis/pnl` | P&L calculado desde fills en DB |
| GET | `/api/analysis/fills` | Fills históricos desde PostgreSQL |
| GET | `/ws` | WebSocket — upgrade para tiempo real |

### Mensajes backend → frontend (`AppMsg` serializado como JSON)

```rust
pub enum AppMsg {
    Status(ConnStatus),
    BookUp(BookSnapshot),
    BookDown(BookSnapshot),
    LastTradeUp(f64),
    LastTradeDown(f64),
    Balance(f64),
    BtcOpen(f64),        // precio BTC al inicio del período 15-min
    BtcPrice(f64),       // precio BTC en tiempo real (Binance aggTrade)
    OrderResult(String), // feedback de orden colocada/cancelada
    OpenOrders(Vec<OpenOrder>),
    RecentFills(Vec<RecentFill>),
    Candles(Vec<Candle>),       // batch inicial
    CandleUpdate(Candle),       // update de la última vela
}
```

### Comandos frontend → backend (`CmdMsg` deserializado desde JSON)

```rust
pub enum CmdMsg {
    PlaceLimitOrder  { side: OrderSide, outcome: Outcome, price: f64, size: f64 },
    PlaceMarketOrder { side: OrderSide, outcome: Outcome, amount_usdc: f64 },
    ScalpBuy { outcome: Outcome, price: f64, size: f64, target_price: f64 },
    CancelOrder  { order_id: String },
    CancelMarket,
}
```

---

## Tipos de datos clave

```rust
pub enum OrderSide { Buy, Sell }
pub enum Outcome   { Up, Down }

pub struct MarketInfo {
    pub title:          String,
    pub token_id_up:    String,          // token address del outcome UP
    pub token_id_down:  Option<String>,  // token address del outcome DOWN
    pub outcome_up:     String,          // label: "Up" / "Yes"
    pub outcome_down:   String,          // label: "Down" / "No"
    pub end_date:       DateTime<Utc>,
    pub active:         bool,
    pub price_to_beat:  Option<f64>,     // BTC price al inicio del 15-min
}

pub struct OpenOrder {
    pub id:           String,
    pub outcome:      String,
    pub side:         OrderSide,
    pub price:        f64,
    pub size_orig:    f64,
    pub size_matched: f64,
}

pub struct RecentFill {
    pub outcome: String,
    pub side:    OrderSide,
    pub price:   f64,
    pub size:    f64,
    pub time:    String,    // "14:32:07" — hora del match
    pub session: String,    // "14:30"    — inicio de la sesión 15-min
}

pub struct Candle {
    pub open_time: i64,  // Unix ms
    pub open: f64, pub high: f64, pub low: f64, pub close: f64, pub volume: f64,
}

pub struct PriceLevel { pub price: f64, pub size: f64 }
pub struct BookSnapshot { pub bids: Vec<PriceLevel>, pub asks: Vec<PriceLevel> }

pub enum CandleInterval {
    OneSecond, OneMinute, FiveMinutes, FifteenMinutes, OneHour
}
```

---

## Worker: fuentes de datos

### Mercado BTC 15-min (Polymarket)

- Descubierto via **Gamma API** (`gamma::Client`): busca el evento activo con slug `btc-15` o similar
- El mercado tiene DOS tokens: `token_id_up` (BTC sube) y `token_id_down` (BTC baja)
- `price_to_beat` = `groupItemThreshold` del mercado en Gamma (precio BTC al inicio del período)
- Fallback si no hay `groupItemThreshold`: Pyth Network REST

### Order book en tiempo real

- **WebSocket CLOB**: `wss://ws-subscriptions-clob.polymarket.com/ws/market`
- Suscripción: `{"assets_ids": [token_up, token_down], "type": "market"}`
- Mensajes `"book"` → actualiza `BookUp`/`BookDown`
- Mensajes `"last_trade_price"` → actualiza `LastTradeUp`/`LastTradeDown`
- Reconexión automática al desconectarse

### BTC/USD precio

- **Binance WebSocket**: `wss://stream.binance.com:9443/ws/btcusdt@aggTrade`
- Campo `"p"` del JSON → `AppMsg::BtcPrice`
- Corre en `tokio::spawn` independiente con backoff exponencial

### Velas BTC/USDT

- **Binance Kline WebSocket**: `wss://stream.binance.com:9443/ws/btcusdt@kline_{interval}`
- Fetch inicial REST: `https://api.binance.com/api/v3/klines`
- Intervalo seleccionable: 1s / 1m / 5m / 15m / 1h

### Balance + órdenes

- Refrescados cada 5 segundos vía timer en el `select!` loop
- `client.balance_allowance()` → `AppMsg::Balance`
- `client.orders()` + `client.trades()` → `AppMsg::OpenOrders` + `AppMsg::RecentFills`
- La sesión 15-min de cada fill: `(match_time.timestamp() / 900) * 900`

---

## Autenticación Polymarket

El SDK soporta dos modos: EOA (Externally Owned Account) y Proxy Wallet.

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

**Por qué importa**: el saldo USDC real está bajo el proxy wallet, no bajo el EOA. Sin funder correcto, `balance_allowance()` devuelve 0.

---

## Colocación de órdenes

### Limit order

```rust
// En handle_limit_order() en worker.rs
let order = client.order_builder()
    .token_id(token_id)
    .side(side)
    .price(price)      // Decimal
    .size(size)        // shares, no USDC
    .order_type(OrderType::Gtc)
    .build()?;
let signed = client.sign_order(order, &signer).await?;
let resp = client.post_order(&signed, None).await?;
```

### Market order

```rust
// Implementado como limit order agresivo (taker) vía Amount::Usdc
let order = client.order_builder()
    .token_id(token_id)
    .side(side)
    .amount(Amount::Usdc(amount_usdc))
    .order_type(OrderType::Market)
    .build()?;
```

### Scalp mode (`ScalpBuy`)

Flujo automático en `handle_scalp_buy()`:

```
1. Colocar BUY limit → obtener order_id
2. Poll client.orders() cada 500ms (max 5min)
   - Si la orden desaparece de open orders → fill completo
   - Si size_matched >= size_orig * 0.995 → fill parcial aceptado
3. Colocar SELL limit a target_price por filled_size shares
```

El campo correcto para tamaño original en `OpenOrderResponse` es **`original_size`** (no `size`).

---

## Errores conocidos y soluciones

### Campo `size` vs `original_size` en `OpenOrderResponse`

- SDK `polymarket-client-sdk 0.4`: el campo correcto es **`original_size`** (tipo `Decimal`)
- `size_matched` también es `Decimal` → `.to_string().parse::<f64>()`

### Proxy wallet vs EOA en Polymarket

- Si el usuario tiene proxy wallet, el saldo USDC está bajo el proxy, no el EOA
- Sin `SignatureType::Proxy` + `funder(proxy_wallet)`, las órdenes pueden fallar o `balance_allowance()` devuelve 0

---

## Comandos útiles

```bash
# Compilar backend (dev)
cd backend_rust && cargo build

# Correr el backend
cd backend_rust && cargo run

# Correr setup inicial de credenciales
cd backend_rust && cargo run --bin setup

# Compilar release
cd backend_rust && cargo build --release

# Ver logs detallados
RUST_LOG=polymarket_dashboard=debug cargo run

# Ver backtrace en panic
RUST_BACKTRACE=1 cargo run
```

---

## Estado actual del proyecto (Abril 2026)

### Backend — Funcionando

- [x] Autenticación CLOB con soporte EOA + Proxy wallet
- [x] Descubrimiento automático del mercado BTC 15-min activo via Gamma API
- [x] Order book en tiempo real (WebSocket CLOB)
- [x] Precio BTC en tiempo real (Binance aggTrade WS)
- [x] Velas OHLCV BTC/USDT con cambio de intervalo en caliente (Binance)
- [x] Colocación de órdenes: limit y market
- [x] Scalp mode: buy a precio X → auto-sell al fill a precio X+Y
- [x] Cancelación de órdenes individuales y de todo el mercado
- [x] Historial de fills con hora y sesión 15-min
- [x] Worker con reconexión automática + backoff exponencial
- [x] **API REST completa** (axum, puerto 3000)
- [x] **WebSocket server** (`/ws`) con snapshot inicial + broadcast
- [x] **PostgreSQL**: candles, btc_ticks, fills — con migraciones automáticas
- [x] **Análisis DB**: query candles históricos, P&L desde fills

### Frontend React — Pendiente

- [ ] Setup inicial (Vite + React + Tailwind)
- [ ] Conexión WebSocket a `ws://localhost:3000/ws`
- [ ] Panel de control: status, BTC price, balance, countdown
- [ ] Chart de velas (TradingView Lightweight Charts)
- [ ] Order book dual UP/DOWN
- [ ] Panel de trading: selector outcome, inputs limit/market/scalp, PANIC
- [ ] Posiciones y historial de fills
- [ ] Panel de análisis: candles históricos + P&L desde DB

### CI/CD + AWS — Pendiente

- [ ] GitHub Actions workflow (branch `dev`)
- [ ] Deploy frontend React → S3 + CloudFront
- [ ] Deploy backend Rust → EC2 (systemd service)
