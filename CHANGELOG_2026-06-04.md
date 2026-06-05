# Changelog — 4 Junio 2026

Sesión de trabajo enfocada en resolver los problemas del TUI `zz-monitor`:
congelamiento, conteo regresivo incorrecto, delta BTC stale, y auto-generación de sesiones.

---

## Commits (7 en total)

| Commit | Descripción |
|--------|-------------|
| `081b97b` | fix: prevent TUI freeze — wrap all main-loop HTTP calls with tokio::time::timeout |
| `dfc78a7` | refactor: zero-I/O main loop + dead code removal |
| `eedae47` | feat: visual imbalance bars — red/green horizontal bars for S5 IMB + D10/D20/D30 |
| `dd9225b` | fix: eliminate session-transition freeze — trade tracking out of render loop |
| `c41639f` | fix: countdown aligned to 15-min boundary + BTC delta uses per-session open + D10/D20/D30 show % |
| `b6688b7` | fix: eliminate remaining stutter — conditional trade tracking + reduced timeouts |
| `5ed1719` | fix: auto-generate child sessions — accept 'scheduled' parent status |

---

## 1. Arquitectura — Main Loop Zero-I/O

### Problema
El main loop del monitor hacía llamadas HTTP con `.await` directamente en el hilo de
renderizado. Cada llamada bloqueaba el render, teclado y WebSocket. Con backend
lento o caído, el efecto acumulativo congelaba la TUI durante minutos.

### Solución
- **Poller task independiente**: `run_poller()` hace todas las llamadas HTTP
  (`/api/odiseo/status`, `/api/btc`, `/api/health`, `/api/hft/latest`, `/api/orders`,
  `/api/sessions`) en paralelo con `tokio::join!` cada 500ms. Resultados enviados por
  `mpsc::unbounded_channel`.
- **Main loop**: solo drena 3 canales (`poll_rx`, `ws_rx`, `kb_rx`) con `try_recv()`
  + render + sleep 100ms. **Cero I/O en el path de renderizado**.
- **Trade tracking** (SL/TSL/fills): movido a los handlers de poll updates.
  `track_manual_fills` solo corre cuando `mt_state==1` o `3` (pending/exit).
  `check_sl_trigger` solo con `mt_state==2` y `mt_sl_price>0`.
  **Ya no corre cada 100ms bloqueando 9s por tick.**
- Gemini trigger buy: `tokio::spawn` fire-and-forget.

### Archivos modificados
- `monitor/src/main.rs` — reescritura completa del main loop
- `monitor/src/api.rs` — 3 capas de timeout: `reqwest::timeout(2s)` + `connect_timeout(1s)` + `with_timeout(2s)` via `tokio::time::timeout`
- `monitor/src/commands.rs` — eliminado `place_sl_order` y `trigger_gemini_buy` (dead code)

---

## 2. Timeouts — Prevención de Bloqueos

Todas las llamadas HTTP en api.rs tienen 3 capas de protección:
1. `reqwest::Client::timeout(2s)` — timeout de transporte
2. `reqwest::Client::connect_timeout(1s)` — timeout de conexión TCP/DNS
3. `with_timeout(2s)` — wrapper `tokio::time::timeout` como red de seguridad

| Punto de bloqueo | Antes | Ahora |
|------------------|-------|-------|
| Polling REST (7 endpoints) | Secuencial, 3s cada uno | Paralelo en spawned task |
| Trade tracking en main loop | Cada 100ms, 3×3s = 9s | Solo con posición activa, 1s timeout |
| Comando `/` (Enter) | `.await` directo | `tokio::time::timeout(2s)` |
| Tecla `s` (session start) | `.await` directo | `tokio::time::timeout(2s)` |

---

## 3. Countdown — Alineación a Frontera de 15 Minutos

### Problema
Cuando el backend auto-iniciaba o recuperaba una sesión a mitad de ronda
(ej: 14:33), usaba `now + 15min = 14:48` como `t5_end`. El countdown mostraba
12 min restantes cuando en realidad quedaban 9 (hasta 14:45, la frontera real).

### Solución
`snap_to_next_chunk(now, 15)` en vez de `now + 15min`:
- Si arranca a 14:33 → countdown apunta a 14:45 ✓
- Si arranca a 14:45 → countdown apunta a 15:00 ✓

### Archivos modificados
- `backend_rust/src/main.rs` — recovery (línea 266) y auto-start (línea 316)

---

## 4. BTC Delta — Prioridad Per-Session

### Problema
`btc_open` (del Gamma API `groupItemThreshold`) solo se carga una vez al arrancar
el backend. Al cambiar de ronda Polymarket, el delta BTC seguía comparando contra
el precio de la ronda anterior (stale).

### Solución
`session_open_btc` (se actualiza en el monitor cada vez que `secs_left` salta >60s,
indicando nueva sesión) ahora tiene **prioridad** sobre `btc_open`.

Afecta: market info, aggregate alert, S2 BTC, S3 BTC Δ.

### Archivos modificados
- `monitor/src/ui.rs` — 4 ubicaciones donde se calcula `btc_ref`

---

## 5. Barras Visuales de Imbalance

### S5 IMB (Imbalance combinado)
```
████████████│████████
▼BEAR 65% │ BULL 35% ▲
UP0.82 · DN1.34
```
Barra horizontal roja (osos) / verde (toros) proporcional al imbalance combinado
UP+DN. Porcentajes numéricos + ratios debajo.

### D10 / D20 / D30 (Imbalance por profundidad)
```
IMB D10          IMB D20          IMB D30
████│██████      ████│████        ████████│██
▼BEAR 45%│BULL 55%▲  ─BEAR 50%│BULL 50%─  ▲BEAR 30%│BULL 70%▲
```
Cada tarjeta muestra barra visual + porcentajes BEAR/BULL (en vez del ratio crudo `1.23x`).

### Archivos modificados
- `monitor/src/ui.rs` — funciones `draw_indicators()` y `draw_indicators_row2()`

---

## 6. Auto-Generación de Sesiones

### Problema
Al terminar una sesión hija de 15 min, no se creaba automáticamente la siguiente.
El padre indefinido (contenedor) se crea con `status='scheduled'`, pero
`auto_generate_child()` y `recover_orphaned_parents()` exigían `status='recording'`.

### Solución
Aceptar tanto `'scheduled'` como `'recording'` como estados válidos del padre:
```rust
// Antes
if parent.status != "recording" { return; }
// Ahora
if parent.status != "recording" && parent.status != "scheduled" { return; }
```

### Archivos modificados
- `backend_rust/src/db/scheduler.rs` — `auto_generate_child()` y `recover_orphaned_parents()`

---

## 7. Limpieza de Código Muerto

| Archivo | Acción |
|---------|--------|
| `monitor/src/api.rs` | Eliminado: campos `price`/`size` de OrderPlaced, método `ok_id`, `http_post_json`, campo `live_mode` |
| `monitor/src/commands.rs` | Eliminado: `place_sl_order()`, `trigger_gemini_buy()` |
| `monitor/src/ui.rs` | Eliminado: `VecDeque` import, constantes `BB_BG`/`BB_AMBER_DIM` |
| `backend/strategies/live.rs` | **Borrado** — 0 imports externos |
| `backend/strategies/order_executor.rs` | **Borrado** — 0 imports externos |
| `backend/premium/updater.rs` | **Borrado** — 0 usos en el proyecto |

---

## Estado Final

- Monitor: `cargo check` 0 errores, `cargo test` 117/117 pasan
- Backend: `cargo check` 0 errores
- Dashboard: render loop 100% desacoplado de I/O, timeouts en todas las capas
- Sesiones: auto-generación corregida, countdown alineado a fronteras reales
- UI: barras visuales de imbalance en S5 + D10/D20/D30, delta BTC por sesión

---

## Deploy

```bash
git fetch origin && git reset --hard origin/main && chmod +x deploy.sh && ./deploy.sh
```
