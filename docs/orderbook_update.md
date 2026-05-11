# Orderbook Update — Mayo 10, 2026

## Commits

```
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
- **WsMsg** nuevos campos: `side: Option<String>`, `book: Option<BookDepth>` — para parsear mensajes `book` del WebSocket
- **Timeout 3s** en todos los HTTP requests (`reqwest::Client` con `.timeout(3s)`) — previene cuelgues del TUI cuando el backend no responde

### `monitor/src/main.rs`
- **Handler `"book"`** en el drenado de mensajes WS: actualiza `s.book_up` / `s.book_dn` directamente del CLOB WebSocket
- **Session reset**: detecta cuando `secs_left` salta >60s (nueva sesión) y limpia `book_up`/`book_dn` a `default()`
- **Campo `prev_secs_left: i32`** para trackear cambios de sesión

### `monitor/src/ui.rs`
- **Layout reorganizado**: depth panel usa `Constraint::Min(6)` — ocupa todo el espacio disponible. Header, ping, precios y eventos reducidos al mínimo
- **`draw_depth_panel`** → **`draw_book_side`**: función extraída, recibe `book` como fuente primaria con fallback a `hft.depth_up_*`
- **Ordenamiento explícito**: asks por precio ascendente (best first) → reverse para display; bids por precio descendente (best first). Display: worst ask → best ask (ceiling, yellow) → SPREAD → best bid (floor, yellow) → worst bid
- **`sort_unstable_by`** + cap 200 niveles para rendimiento
- **Niveles dinámicos**: `half = (area.height - 3) / 2`, clamp 4-8 por lado para que bids y asks quepan en pantalla
- **Ping bar** ahora incluye sesión + BTC price
- **Price panel** comprimido a 1 línea: ▲ UP + ▼ DN + BTCvol

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
            ├── WsMsg parse → msg.book → s.book_up / s.book_dn  [fuente primaria]
            ├── WsMsg parse → msg.data → s.hft.depth_up/dn_*   [fallback]
            │
            └── draw_book_side() → sort + render bids/asks
```

---

## Problema pendiente

**El TUI se congela al cambiar de sesión si no se reinicia el server.**

- El timeout de 3s en HTTP requests mitiga el síntoma pero no la causa raíz
- La causa está en el **backend**: durante la transición de sesión (descubrir nuevo mercado, reconectar CLOB WS, re-autenticar), algo bloquea las respuestas HTTP
- Posibles causas en el backend:
  - Deadlock en RwLocks de `AppState` (ej. `book_up` write lock mientras otro task hace read)
  - `capture_combined()` haciendo `.await` sobre un lock ya tomado
  - El worker entrando en loop de reconexión y bloqueando el runtime
  - Canal `mpsc` lleno bloqueando al sender
- Se necesita investigar `backend_rust/src/main.rs` y `backend_rust/src/modules/core/worker.rs` para encontrar el deadlock

### Workaround actual
Reiniciar el server resuelve temporalmente:
```bash
sudo systemctl restart zzignal-app
```

---

## Frontend React (cambios adicionales no relacionados con el TUI)

### `frontend_react/src/App.jsx`
- Destructura `bookUp`, `bookDown` de `useBackend()` y los pasa a `DepthChart`

### `frontend_react/src/components/DepthChart.jsx`
- Reescrito: muestra orderbook ladder en tiempo real desde WebSocket (ya no usa HTTP polling)
- Dos paneles lado a lado: UP (verde) y DOWN (naranja)
- Cada panel: asks (rojo), mid/best bid-ask, bids (verde)
- Barras de profundidad proporcionales
