# Core — Open Source (MIT)

El Core es gratuito y open source. Incluye todo lo necesario para conectarte a Polymarket, ver el order book en tiempo real, y ejecutar órdenes manualmente.

---

## Qué incluye el Core

| Feature | Descripción |
|---|---|
| **Auth CLOB** | Autenticación EOA + Proxy Wallet automática |
| **BTC multi-proveedor** | Precio en tiempo real de Binance, Coinbase o Kraken (seleccionable) |
| **Velas sintéticas** | OHLCV generado desde ticks de precio (1s, 1m, 5m, 15m, 1h) |
| **Order Book** | Libro de órdenes UP y DOWN en tiempo real vía WebSocket |
| **Órdenes** | Limit, market, scalp, cancel individual, cancel market |
| **API REST** | 20+ endpoints para status, book, candles, órdenes, fills |
| **WebSocket /ws** | Snapshot inicial + broadcast en tiempo real + comandos |
| **PostgreSQL** | Persistencia opcional de candles, fills, btc_ticks |
| **Frontend React** | UI completa con gráfico de velas, order book, status bar |

---

## Compilar y ejecutar

```bash
cd backend_rust

# Desarrollo
cargo run

# Producción
cargo build --release
./target/release/polymarket-backend
```

**Sin módulos Premium**: compila `cargo build` sin flags adicionales.

---

## Variables de entorno (.env)

```env
# Obligatorias
POLYMARKET_PRIVATE_KEY=0x...        # clave privada de tu wallet
CLOB_API_KEY=uuid                   # API key de Polymarket CLOB
CLOB_API_SECRET=...                 # API secret
CLOB_API_PASSPHRASE=...             # API passphrase

# Opcional (si no está, el backend corre sin persistencia)
DATABASE_URL=postgres://user:pass@host:5432/db

# Licencias de módulos Premium (solo si los compraste)
ZZIGNAL_LICENSE_KEY=eyJ...          # token de activación
```

---

## API Endpoints del Core

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/api/status` | Estado: "LIVE", "Initializing", etc. |
| GET | `/api/market` | Info del mercado BTC activo |
| GET | `/api/balance` | Saldo USDC |
| GET | `/api/btc` | Precio BTC actual + open |
| GET | `/api/btc/provider` | Proveedor BTC actual |
| POST | `/api/btc/provider` | Cambiar proveedor: `{"provider":"coinbase"}` |
| GET | `/api/book/up` | Order book UP |
| GET | `/api/book/down` | Order book DOWN |
| GET | `/api/candles` | Velas en memoria |
| POST | `/api/candles/interval` | Cambiar intervalo |
| GET | `/api/orders` | Órdenes abiertas |
| POST | `/api/orders/limit` | Limit order |
| POST | `/api/orders/market` | Market order |
| POST | `/api/orders/scalp` | Scalp buy |
| DELETE | `/api/orders/{id}` | Cancelar orden |
| DELETE | `/api/orders` | Cancelar todo |
| GET | `/api/fills` | Fills recientes |
| GET | `/api/analysis/candles` | Candles históricos (DB) |
| GET | `/api/analysis/pnl` | P&L calculado (DB) |
| GET | `/api/analysis/fills` | Fills históricos (DB) |
| WS | `/ws` | WebSocket en tiempo real |

---

## Estructura de archivos del Core

```
backend_rust/src/modules/core/
├── mod.rs           ← pub mod worker, api, state, credentials, persistence
├── worker.rs        ← Conexión CLOB, BTC WS, órdenes (hilo separado)
├── api.rs           ← Router REST + WS /ws
├── state.rs         ← AppState (RwLock + watch channels + broadcast)
├── credentials.rs   ← Gestión segura de credenciales .env
└── persistence.rs   ← Migraciones + CRUD candles, fills, btc_ticks
```

---

## Frontend (React + Vite + Tailwind)

```
frontend_react/src/
├── App.jsx                    ← Layout principal + tabs
├── hooks/useBackend.js        ← Hook WebSocket + estado global
└── components/
    ├── StatusBar.jsx          ← Status, BTC price, balance
    ├── CandleChart.jsx        ← Velas SVG custom
    ├── OrderBook.jsx          ← Book UP/DOWN dual
    ├── MessageLog.jsx         ← Log de mensajes WS
    ├── DashboardBD.jsx        ← Snapshots + ejecuciones programadas
    └── SessionsPanel.jsx      ← Session Recorder
```

---

## Hacer tu propia build open source

1. Forkea el repo
2. El Core está bajo MIT — puedes usarlo, modificarlo y redistribuirlo
3. Los módulos Premium requieren licencia de pago
4. Si quieres contribuir al Core, abre un PR

---

## Limitaciones del Core (sin módulos Premium)

- No captura multi-timeframe del order book (solo velas de precio BTC)
- No detecta patrones automáticamente
- No ejecuta órdenes automáticamente (bot)
- No tiene dashboard ejecutivo
