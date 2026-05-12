# ZZignal Monitor — Sesión 11 Mayo 2026

## Estado actual
- **Branch**: main
- **Último commit**: `8d1c3f9` — "ui: card1 shows BTC NOW top, abrio below — remove confusing BEAT label"
- **Tests**: 36/36 pass
- **Backend**: Binance por defecto (`state.rs:214`)

---

## Índice de features implementadas

### 1. Gemini — Trading automático por trigger
| Comando | Significado |
|---|---|
| `/5g70` | Gemini $5, target 0.70, trigger 0.65 (target−0.05) |
| `/8g72` | Gemini $8, target 0.72, trigger 0.67 |
| `/7g70e82` | Gemini $7, target 0.70, trigger 0.65, exit @0.82 |
| `/c` | Cancela Gemini pendiente + órdenes si lanzadas |
| `/co` | Market sell posición + cancela todo + PANIC estrategias |

**State machine**: IDLE → WAITING → TRIGGERED → ACTIVE → DONE
- Trigger: WS hft_state (primario) + poll HFT 500ms (fallback)
- Primer tick UP o DN ≤ trigger dispara BUY a target
- Exit automático colocado DESPUÉS del fill (no antes)
- Gemini card en footer (solo visible cuando activo)

### 2. Provider BTC — Binance default
| Comando | Efecto |
|---|---|
| `/provider binance` | Cambia a Binance (más rápido, ~0.5ms) |
| `/provider coinbase` | Cambia a Coinbase |
| `/provider kraken` | Cambia a Kraken |

- Backend arranca con Binance por defecto
- Monitor muestra provider en card 1 BTC y en WS handler + poll 5s

### 3. Layout dashboard — Rediseño completo

```
┌─ BTC ─────┐ ┌─ BTC Δ ────┐ ┌─ TIMER ───┐ ┌─ BALANCE ─┐
│ $81,500   │ │ ▲ +$1,234  │ │ 12:34     │ │ $150.25   │
│ abrio $80K│ │   +1.5%    │ │ restantes │ │ ords: 2   │
│ BINANCE   │ │ ▲ UP       │ │           │ │ BALANCE   │
└───────────┘ └────────────┘ └───────────┘ └───────────┘
┌─ UP ──────┐ ┌─ DN ────────┐
│ ▲ UP .5500│ │ ▼ DN .4700  │
│ +5.2% ▶POS│ │ -3.1% —     │
└───────────┘ └─────────────┘
┌─ S1 CLOB ─┐ ┌─ S2 BTC  ──┐ ┌─ S3 VEL ──┐ ┌─ S4 VOL ──┐
│ UP +2.1%  │ │ $81.5K +1% │ │ ▲ $5.2/s │ │ $12,500   │
│ DN -1.8%  │ │ ▲ BULL ✓   │ │ ▲ $1.3/s² │ │ B/A 1.5x  │
│ ▲ UP ✓BTC │ │             │ │           │ │ ▲BUY       │
└───────────┘ └────────────┘ └───────────┘ └───────────┘
┌─ TRADING ──────────────────────────────────────────────┐
│ MANUAL: IDLE  0 posiciones                             │
└────────────────────────────────────────────────────────┘
┌─ ORDERBOOK ───────────────────────────────────────────┐
│ .6500 ██████████                     ██████ .5200     │
│ .6400 ████████                       ███████ .5300    │
│ ...    ...           SPREAD          ...    ...       │
└────────────────────────────────────────────────────────┘
┌─ ORDENES/POSICIONES ──┐ ┌─ TRADE LOG ────────────────┐
│ ✓ BUY UP @.5500 5/7  │ │ 14:32:01 ✓ BUY UP sz=7     │
│ ✗ SELL DN @.4700 0/5 │ │ 14:31:55 ⚡ GEMINI @.65→.70│
│ ── POS STRATEGY ──    │ │ 14:31:30 ▶ MKT SELL DN     │
└───────────────────────┘ └────────────────────────────┘
┌─ ⚡ GEMINI: @0.65→0.70 $5  EXIT @0.80  /c=cancelar ──┐  (solo si activo)
└───────────────────────────────────────────────────────┘
┌─ FOOTER ──────────────────────────────────────────────┐
│ /4up65 BUY  /lup70 SELL  /co cash out  /c cancel  /x mkt │
│ /4up65e70 BUY+exit  /5g70 Gemini  /provider binance  SL:OFF │
│ [/]comando  [Tab]vista  [Esc/q]salir                  │
└───────────────────────────────────────────────────────┘
```

### 4. S1-S4 Indicadores de mercado

| Slot | Contenido | Datos |
|---|---|---|
| **S1 CLOB** | Polymarket momentum | UP △%, DN △%, dirección dominante. ✓BTC si confirmado |
| **S2 BTC** | BTC momentum | Precio + △%, ▲BULL/▼BEAR. ✓CLOB si confirmado |
| **S3 VEL** | Velocidad + aceleración | Computado localmente de BTC history (no del backend) |
| **S4 VOL** | Volumen sesión | Acumulado de clob_trade_up_vol + dn_vol + bid/ask ratio |

**Alerta visual**: S1 + S2 pulsan **verde** sincronizado cuando CLOB+BTC ambos UP. Pulsan **rojo** cuando ambos DOWN. Fondo + borde. (pulse_tick % 10 < 7 = 70% on)

### 5. Orderbook
- Barras con escala **logarítmica**: `ln(size+1) / ln(max+1)` 
- Altura **fija** 13 líneas (no expandible)
- Grid lines eliminadas
- Niveles dinámicos: `(avail-1)/2` cap 9

### 6. Exit orders — Post-fill
- Exit ya NO se coloca junto con el buy
- Se coloca automáticamente después de detectar el fill (`place_exit_after_fill`)
- Detectado en 3 paths: seen fill, gone (was seen), fast fill (>4s)
- `mt_exit_placed_at` para fast-fill detection

### 7. Cash Out (/co) — Arreglado
- Usa `http_post` (no `http_post_result`) para market sell
- Calcula PnL antes de resetear
- **No resetea** si el market sell falla → posición intacta
- PnL se acumula en `mt_pnl_cum`

### 8. Position bar — Limpia
- Solo muestra posición MANUAL (no estrategias Senna/H65/O83)
- Idle → "0 POSICIONES — /4up65 para abrir"
- Con historial → "0 POSICIONES Σ+12.50 3T/2W — /4up65 para abrir"
- Estrategias visibles en panel de Órdenes/Posiciones

### 9. Trade log — Persistente
- `trade_log.clear()` eliminado de `reset_manual()` y `cancel_active()`
- Todas las operaciones persisten durante la sesión
- Panel expandible: `Min(8)` líneas, hasta 30 entradas

### 10. Unit tests — 36/36
| Módulo | Tests |
|---|---|
| commands | 24 (buy, sell, gemini 50 cases, provider, sl, tsl, cancel, alerts, meta) |
| ui::indicator_tests | 12 (clob_mom 3, btc_mom 3, aligned 2, vol_ratio 4) |

---

## Archivos modificados

| Archivo | Cambios principales |
|---|---|
| `monitor/src/main.rs` | State: gemini_*, btc_provider, pulse_tick, btc_history, session_vol_cum, mt_exit_placed_at. check_gemini_trigger(), BTC vel/accel compute |
| `monitor/src/commands.rs` | Parsed::Gemini, Parsed::Provider, try_parse_gemini(), exec_gemini(), trigger_gemini_buy(), place_exit_after_fill(), exec_provider(), /c y /co extendidos |
| `monitor/src/ui.rs` | draw_indicators S1-S4 con pulso, draw_gemini_card, BTC cards redesign, position bar limpia, footer rediseñado, orderbook log scale, calc_* test functions |
| `monitor/src/api.rs` | WsMsg.provider, BtcProviderInfo struct |
| `backend_rust/src/modules/core/state.rs` | Default Coinbase→Binance |

---

## Comandos rápidos

```bash
# Server (tras git pull)
cd ~/zzignal-app
git checkout -- backend_rust/target/.rustc_info.json  # si hay conflicto
git pull

# Build backend
cd backend_rust && cargo build --release

# Build monitor  
cd monitor && cargo build --release

# Correr tests
cd monitor && cargo test    # 36/36

# Iniciar sesión
curl -s -X POST http://localhost:8080/api/sessions/start \
  -H "Content-Type: application/json" \
  -d '{"name":"BTC15","duration_min":15,"depth_levels":50,"indefinite":true}'

# Verificar status
curl -s http://localhost:8080/api/status
curl -s http://localhost:8080/api/book/up | jq '[.bids[0].price, .asks[0].price]'
```

## Trading commands

```
/4up65        BUY UP @0.65 ($4)
/4d65         BUY DOWN @0.65 ($4)
/4up65e70     BUY UP + exit @0.70
/4up65e70s50  BUY UP + exit @0.70 + SL bracket @0.50
/lup70        SELL UP limit @0.70
/ld70         SELL DOWN limit @0.70
/lm           SELL MARKET
/x            SELL MARKET (alias)
/c            CANCEL (Gemini, órdenes, o sugiere /lm si posición activa)
/co           CASH OUT (market sell + PANIC estrategias)
/p            PANIC (liquidar TODO)
/5g70         GEMINI $5 trigger 0.65 target 0.70
/5g70e80      GEMINI $5 + exit @0.80
/provider binance|coinbase|kraken
```
