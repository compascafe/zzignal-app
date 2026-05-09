# ZZIGNAL — Dev Log May 9 2026

---

## Sesión 1 — Fixes críticos de trading

### Bug: Same-tick re-entry (compras raras)
Posición se liquidaba y re-entraba en el mismo tick.
**Fix:** `return;` tras `*pos = OdiseoPosition::default()` en `process()`.

### Bug: `/h10` no ejecutaba
`http_post` en TUI descartaba errores. Poll cada 2s sobrescribía estado.
**Fix:** `http_post` retorna `Result`. Locks `h65_lock` / `odi_lock` evitan overwrite del poll.

### Bug: trade_min_vol = 50 → sin señal de precio
Filtraba casi todos los trades de Polymarket. Ventana de precios vacía → `lt_up=None` → estrategia ciega.
**Fix:** `trade_min_vol` bajó de 50 → 5 contratos.

### Bug: mid=0.5 (stale Polymarket default) envenenaba decisiones
El mid default de Polymarket (0.50) disparaba trailing stops falsos y reseteaba confirmaciones.
**Fix:** Solo usar `mid` si `|mid - 0.5| > 0.01`. Si no, usar `last_px` o `best_bid`.

---

## Sesión 1.5 — Comandos slash mejorados

### `/h5` a `/h100`
Activan Houdini 65 con presupuesto N (múltiplos de 5). Validación estricta.

### `/o5` a `/o100`
Activan Odiseo 83 con presupuesto N.

### `/p` — PANIC completo
- Market sell UP + DOWN
- Cancela todas las órdenes
- Apaga TODAS las variantes una por una (igual que `zz-emergency`)
- No deja estrategia prendida

### `/s5` a `/s100`
Activan **Senna** (Scalper Momentum) con presupuesto N.
Toggle con `/s` solo.

---

## Sesión 2 — Estrategia Senna (Scalper Momentum)

### Diseño
Variante índice 2 en `ODISEO_DEFS`. Código `"scalper"`.

**Entrada:**
- **CLOB momentum:** `px - prices[0] >= 0.015` en 2 ticks
- **BTC big move:** `vel > 10` (UP) o `vel < -10` (DOWN) + CLOB confirmando
- Market buy (Dublin 155ms latencia)
- `confirm_ticks: 1` (entrada inmediata)
- `btc_trend_filter: true` (bloquea solo si BTC va muy en contra)

**Salida — TP/SL dinámico `f(x) = 15 - 0.1x`:**
| Entry (¢) | Profit % | TP | SL |
|---|---|---|---|
| 35 | 11.5% | 0.390 | 0.326 |
| 45 | 10.5% | 0.497 | 0.423 |
| 55 | 9.5% | 0.602 | 0.522 |
| 65 | 8.5% | 0.705 | 0.628 |
| 70 | 8.0% | 0.756 | 0.678 |
| 75 | 7.5% | 0.806 | 0.728 |
| 80 | 7.0% | 0.856 | 0.778 |
| 85 | 6.5% | 0.905 | 0.828 |
| 95 | 5.5% | 1.00 | 0.927 |

- SL = 60% del TP (relación riesgo/beneficio 1:1.67)
- Trail: 0.01 (ultra-tight, captura ganancia rápido)
- Timeout: 60s sin exit → market sell automático
- Last 60s sesión → market sell todo

**Entry price:** usa `best_ask` del orderbook (precio real de fill), no el `px` de señal.
**Signal price:** guardado en `signal_px` para análisis de slippage.

### CSV — Columnas nuevas para Senna
| Columna | Descripción |
|---|---|
| `scalper_up/dn` | Estado (0=idle, 1=watch, 2=active) |
| `scalper_up_entry` | Precio de fill real (best_ask) |
| `scalper_up_pnl` | PnL virtual |
| `scalper_up_exit` | Precio de salida |
| `scalper_up_r` | Razón (1=TP, 4=SL, 5=trail, 6=timeout/flash) |
| `scalper_event` | IN_UP, OUT_UP, IN_DN, OUT_DN |
| `entry_trigger` | 1=CLOB momentum, 2=BTC big move |
| `tp_pct` | % de ganancia objetivo dinámico |
| `entry_slippage` | Diferencia fill - señal |

---

## Sesión 3 — Blind Exit + Timeout + Best Bid

### Bug: Precio congelado sin trades
Sin trades reales, `px` se quedaba en `last_px` = entry price. TP/SL/trail nunca evaluaban.
**Fix:** `best_bid_up/dn` en AppState — guarda mejor bid de cada lado del orderbook. Se actualiza en cada BookUp/BookDown. Siempre fresco.

**Jerarquía de fuentes de precio:**
1. `lt` (promedio trades filtrados)
2. `raw_trade_up/dn` (último trade sin filtro)
3. `mid` vivo (`|mid-0.5|>0.01`)
4. `best_bid_up/dn` (orderbook, siempre fresco)
5. `last_px` (último precio conocido, blind exit)

### Bug: minute boundary mataba posiciones
Cada minuto en `:XX.021` el mid iba a 0 → mataba confirmaciones y posiciones.
**Fix:** `last_px` sobrevive al boundary. Momentum solo se chequea en precio fresco (`px_fresh`).

---

## Sesión 4 — TUI completo

### Cambios en el TUI
- **Commit hash** arriba en letras blancas (build.rs embebe `git rev-parse`)
- **Ping bar** en Dashboard: `WS: 45ms ▁▁▂▁▁▁ API: 120ms` con sparkline
- **Position bar** 3 columnas: H65 | O83 | SENNA con fondo verde/rojo y PnL%
- **Banner** muestra las 3 estrategias: `H65 OFF | O83 OFF | SENNA $10`
- **Eventos** en log: `⚡ SENNA ENTER UP @ 0.500`, `⚡ SENNA EXIT UP @ 0.530 PnL:+0.60`
- **Reinv** eliminado del header
- **Alertas** siempre visibles con estado de cada estrategia

### Latencia
| Servidor | Avg | P50 | P99 |
|---|---|---|---|
| Tokio/LATAM | 48-50ms | 47-49ms | 99ms |
| Dublin | 52-54ms | 53-54ms | 109ms |

Dublin gana en HTTP a Polymarket (~100ms vs ~300ms).

---

## Rama `main-us`
Para US East: solo CLOB, market buy, `btc_trend_filter: false`, BTC provider Coinbase.
Lista para cuando migres a Virginia.

---

## Commits de hoy (orden cronológico inverso)
```
8ae5d38 feat: TP/SL dinamico f(x)=15-0.1x para Senna + CSV columnas
db9d083 feat: Dual Momentum — Senna entra por CLOB O BTC big move
40092c4 fix: best_bid_up/dn siempre fresco + Senna timeout 60s
7d17937 fix: Senna entry_price=best_ask, TP/SL desde fill real
3ad100c fix: Senna timeout 60s — market sell automatico
a7b0c6b feat: Senna real-time — posicion bar + alerts ENTER/EXIT
4f8c3aa fix: Senna market buy — no se queda colgada
11a9002 fix: Senna escribe al CSV
0b29290 feat: Senna Scalper Momentum — variante 3, /s comando
b1b6157 feat: Senna TUI — banner, poll, apply_variant
9d0ef72 sim: Monte Carlo Senna vs H65
fb97962 fix: confirm_ticks 2→1
4f894b9 fix: momentum skip en fallback stale
1e9f9d7 fix: BTC momentum relajado
0a57c3c fix: build.rs re-ejecuta cuando cambia git HEAD
95fd251 deploy: verify TUI commit hash
90d2cf2 feat: ping bar en Dashboard
798da25 fix: entry vuelve a limit buy
a90ce86 feat: token_momentum en CSV
b3a2d70 feat: momentum entry — token subiendo + BTC
9026cfa fix: trail 0.015→0.04 + market sell
69b0706 fix: boundary 20s→60s ultimo minuto
29b84df feat: blind exit + timeout 5min
2dbf555 feat: trailing stop + entry confirm + BTC trend
152fd72 fix: Houdini ahora entra — trade_min_vol 50→1
11197bc refactor: solo 2 variantes — O83 + H65
d510df0 fix: disable_all limpia sesiones
42b6fd2 fix: /hN enciende live_mode + aisla variante
2701aa7 fix: PANIC apaga todas + WS hft_state real-time
095d67c fix: same-tick re-entry + slash commands + PnL highlight
```

---

## Para arrancar en el server

```bash
cd ~/zzignal-app
git pull
zz-deploy

# TUI
cd monitor && rm -rf target && cargo build --release
pkill -f zzignal-monitor && ./target/release/zzignal-monitor

# Activar Senna
/s10   # en el TUI, o:
curl -sX POST localhost:8080/api/odiseo/variant -H "Content-Type: application/json" -d '{"index":2,"enable":true}'
curl -sX POST localhost:8080/api/odiseo/live -H "Content-Type: application/json" -d '{"enable":true}'
curl -sX POST localhost:8080/api/odiseo/budget -H "Content-Type: application/json" -d '{"index":2,"amount":10}'
```
