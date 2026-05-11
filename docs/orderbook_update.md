# Orderbook + Graficos Update — Mayo 10, 2026

## Commits (todos)

```
1e8177e fix: tab bar con Paragraph+Span en vez de Tabs widget para evitar texto invisible
e193c03 feat: tab GRAFICOS - DOM, TAP, Area Acumulada, Histograma + trade tracking
0a4878a fix: timeout 3s en HTTP requests del TUI para evitar cuelgues en cambio de sesion
a229925 layout: depth panel ocupa todo el espacio, header/ping/precios minimos
5208502 fix: hard reset del orderbook cuando cambia de sesion (secs_left salta >60)
ef6d21a fix: sort_unstable + cap 200 niveles para evitar cuelgues en cambio de sesion
8c8cdd0 fix: orderbook sort explicito - asks asc→rev, bids desc
f937461 depth: 17 lineas x 15 niveles por lado + eventos compacto
c34562d fix: depth panel enfocado en zona de accion - ceiling/floor + spread
7566b80 fix: TUI recibe book messages por WebSocket — depth en tiempo real
```

---

## Archivos modificados

### `monitor/src/api.rs`
- **WsMsg**: `side: Option<String>`, `book: Option<BookDepth>` — parsea mensajes `book` del WS
- **Timeout 3s**: `reqwest::Client` con `.timeout(3s)` — evita cuelgues del TUI
- **TradeEntry** struct: `{ ts, side, price, size }` — para TAP (Time & Sales)

### `monitor/src/main.rs`
- **Handler `"book"`**: actualiza `s.book_up` / `s.book_dn` desde CLOB WS
- **Session reset**: `secs_left` salta >60s → limpia `book_up`/`book_dn` a default
- **`prev_secs_left: i32`**: trackea cambios de sesión
- **Trade tracking**: en `apply_hft_state`, detecta nuevos trades y los pushea a `s.trades`
- **`trades: VecDeque<TradeEntry>`**: buffer de 100 trades para TAP
- **Tab switching**: `(tab + 1) % 3` — 3 pestañas

### `monitor/src/ui.rs`
- **Layout reorganizado**: depth panel `Constraint::Min(6)` — ocupa todo el espacio
- **`draw_book_side`**: sort explícito, ceiling/floor yellow, spread, niveles dinámicos
- **Ping bar**: incluye sesión + BTC
- **Price panel**: 1 línea (▲ UP + ▼ DN + BTCvol)
- **Tab GRAFICOS**: 4 paneles (ver abajo)
- **Tab bar**: `Paragraph` con `Span` en vez de `Tabs` widget

### Frontend React (cambios secundarios)
- `App.jsx`: destructured `bookUp`/`bookDown`
- `DepthChart.jsx`: orderbook ladder en tiempo real desde WS (sin HTTP poll)

---

## Tab GRAFICOS — 4 paneles

```
┌──────────────┬──────────────┐
│   DOM        │   TAP        │
│ Depth of     │ Time & Sales │
│ Market       │              │
│ UP bids/asks │ time prc sz  │
│ spread       │ side         │
├──────────────┴──────────────┤
│  Area Acumulada             │
│  bids █ / asks █            │
├─────────────────────────────┤
│  Histograma                 │
│  volumen por nivel precio   │
└─────────────────────────────┘
```

### DOM (Depth of Market)
- UP (verde) y DOWN (rojo) lado a lado
- Asks orden ascendente → reverse, bids descendente
- Spread entre best bid y best ask
- Barras proporcionales al max size

### TAP (Time & Sales)
- Últimos 20 trades del buffer `s.trades`
- Timestamp, precio (bold), tamaño, lado (UP=green, DOWN=red)

### Area Acumulada
- Volumen acumulado de bids (verde) y asks (rojo) por nivel de precio
- Buckets dinámicos según altura disponible

### Histograma
- Volumen por bucket de precio (cyan)
- Buckets dinámicos, barra proporcional

---

## Flujo de datos

```
Polymarket CLOB WS
    │
    ▼
Backend worker.rs → dispatch_ws_msg()
    │
    ├── AppMsg::BookUp/BookDown → state.book_up / book_down
    │       │
    │       └── capture_combined() → LatestHftState.depth_up/dn_*
    │               │
    │               └── broadcast_tx: {"type":"hft_state","data":{...}}
    │
    └── broadcast_tx: {"type":"book","side":"up","book":{"bids":[...],"asks":[...]}}
            │
            ▼
    TUI WebSocket (/ws)
            │
            ├── WsMsg.book → s.book_up/dn  [fuente primaria → DOM + Graficos]
            ├── WsMsg.data → s.hft.depth_* [fallback]
            └── apply_hft_state → s.trades [TAP]
```

---

## Trade tracking

```rust
// En apply_hft_state, cada tick:
if new_hft.clob_trade_up > 0.0 && precio_cambio {
    s.trades.push_front(TradeEntry { ts, "UP", price, size });
}
if new_hft.clob_trade_dn > 0.0 && precio_cambio {
    s.trades.push_front(TradeEntry { ts, "DOWN", price, size });
}
// Cap 100 trades
```

---

## Renderizado del tab bar

Cambiado de `Tabs` widget a `Paragraph` con `Span` directos:
```
DINERO REAL (blanco/rojo) | PAPER MONEY (blanco/verde) | GRAFICOS (blanco/azul)
```
Inactivos: texto gris sin fondo.

---

## Problema pendiente

**El TUI se congela al cambiar de sesión (requiere reiniciar el server).**

- Timeout de 3s en HTTP requests mitiga el síntoma
- Causa raíz en el **backend**: deadlock o bloqueo durante transición de sesión
- Posibles lugares: `backend_rust/src/main.rs` (AppState RwLocks), `backend_rust/src/modules/core/worker.rs` (reconexión CLOB)
- Workaround: `sudo systemctl restart zzignal-app`

---

## Server

- IP Dinero Real: `ip-172-26-3-22` (Tokyo Lightsail, antes era `ip-172-26-10-114`)
- IP Dublin (pet): `63.32.155.242` — SSH da timeout, usar consola Lightsail directa
- Binario TUI: `~/zzignal-app/monitor/target/release/zzignal-monitor`
- Binario backend: `~/zzignal-app/backend_rust/target/release/polymarket-backend`

### Compilar TUI
```bash
cd ~/zzignal-app && git pull
cd monitor && cargo build --release
# Si se cuelga (first build lento), usar debug:
cd monitor && cargo build
pkill -f zzignal-monitor && ./target/release/zzignal-monitor
```

### Compilar backend
```bash
cd ~/zzignal-app/backend_rust && cargo build --release
sudo systemctl restart zzignal-app
```
