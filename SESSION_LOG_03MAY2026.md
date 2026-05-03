# Sesión 3 Mayo 2026 — ZZignal HFT Dashboard

## Estado actual (15:10 UTC)

### Servidor
- **IP**: `ip-172-26-10-114` (Tokyo)
- **Binario**: v0.2.5 (commit `c1190e7`), compilado y corriendo ✅
- **Sesión activa**: #600 (15:00-15:15 UTC), outcome pendiente
- **Próxima**: #601 (15:15-15:30 UTC)

### Lo que está funcionando ✅

| Fix | Archivo | Estado |
|---|---|---|
| Ring buffer timestamps locales | `binance_depth.rs` | ✅ latencia ~50ms |
| Ring no-clear entre sesiones | `scheduler.rs` | ✅ micro_price poblado |
| CSV escritura: BufWriter 50MB | `session_manager.rs` | ✅ flush cada 100 filas |
| Background flush 15s | `main.rs` | ✅ task async |
| poly_mid one-sided fallback | `metrics.rs` | ✅ 94-100% filas con mid |
| velocity fallback price_history | `metrics.rs` | ✅ |
| is_informed 500ms window | `metrics.rs` | ✅ |
| gap_alert 0.5% threshold | `metrics.rs` | ✅ |
| BB layer sin spread_ok | `adaptive_risk_engine.rs` | ✅ |
| CP-only Layer 5 fallback | `adaptive_risk_engine.rs` | ✅ |
| Warmup 4h (240 velas) | `adaptive_risk_engine.rs` | ✅ |
| Warmup gate 50+ ticks | `adaptive_risk_engine.rs` | ✅ |
| CP threshold 0.15 | `adaptive_risk_engine.rs` | ✅ |
| Fills dedup | `main.rs` | ✅ |
| Hercules / Hydra / PNR modules | varios | ✅ |
| Insight strategies (Cerbero/Fenix) | `insight_strategies.rs` | ✅ |
| Fenix Trading independiente | `fenix_trading.rs` | ✅ confirmación 1/3/5/7/10 ticks |

### Lo que falta verificar ⚠️

1. **Sesión 600**: primer CSV completo con 109 columnas, Fenix PnL en vivo
2. **Sesión 601**: confirmación de estabilidad multi-sesión
3. **master_signal**: debe mostrar >1 señal por sesión (antes solo 1)
4. **Fenix Trading PnL**: ver en frontend `/api/fenix` qué estrategia gana

### Estrategias activas — Resumen

| # | Nombre | Entry | Gate | TP/Stop | CSV |
|---|---|---|---|---|---|
| 1 | Hercules | 4 capas señal | CP 0.15 | — | campos engine |
| 2 | Hydra 85 | T-5min, mid>0.85 | volatilidad 0.03 | TP 0.95, stop 0.03 | t5_prediction |
| 3 | Hydra 90 | T-3min, mid≥0.90 | volatilidad 0.02 | TP 0.95, stop 0.015 | t3_prediction |
| 4 | Hydra No Return | PNR buckets | — | descubre óptimo | pnr_* |
| 5 | Cerbero 70-80 | [0.70-0.80] | observación | — | cerbero70_* |
| 6 | Cerbero 80-90 | [0.80-0.90] | observación | — | cerbero80_* |
| 7 | Cerbero 90-98 | [0.90-0.98] | observación | — | cerbero90_* |
| 8 | Fenix 35-65 | [0.35-0.65] | 1 tick | $20 virtual | fenix35_* |
| 9 | Fenix 30-50 | [0.30-0.50] | 3 ticks | $20 virtual | fenix30_* |
|10 | Fenix 45-55 | [0.45-0.55] | 5 ticks | $20 virtual | fenix45_* |
|11 | Fenix 40-50 | [0.40-0.50] | 7 ticks | $20 virtual | fenix40_* |
|12 | Fenix 45-50 | [0.45-0.50] | 10 ticks | $20 virtual | fenix4550_* |

### CSV: 109 columnas

Columnas clave nuevas:
- `fenix35_entry, fenix35_pnl, fenix30_entry, fenix30_pnl, fenix45_entry, fenix45_pnl, fenix40_entry, fenix40_pnl, fenix4550_entry, fenix4550_pnl`
- Metadata `# Column:` con descripción de las 94+ columnas originales

### API Endpoints

| Endpoint | Qué devuelve |
|---|---|
| `GET /api/wisdom` | Hercules (engine stats) |
| `GET /api/wisdom2` | Hydra 85 |
| `GET /api/wisdom3` | Hydra 90 |
| `GET /api/wisdom4` | Hydra No Return (PNR) |
| `GET /api/insights` | Cerbero + Fenix observation |
| `GET /api/fenix` | Fenix Trading PnL ($20 virtual) |

### Frontend (SystemHealth.jsx)

Paneles en orden:
1. Connection & Performance
2. System Resources
3. **Hercules** — RL engine stats
4. **Hydra 85** — T-5 paper trading
5. **Hydra 90** — T-3 paper trading
6. **Hydra No Return** — PNR analysis
7. **Cerbero & Fenix** — range insights
8. **Fenix Trading** — $20 virtual PnL
9. Macro 24h
10. Sessions

### Archivos modificados hoy

```
backend_rust/src/main.rs
backend_rust/src/modules/core/api.rs
backend_rust/src/modules/core/state.rs
backend_rust/src/modules/db/scheduler.rs
backend_rust/src/modules/hft/adaptive_risk_engine.rs
backend_rust/src/modules/hft/binance_depth.rs
backend_rust/src/modules/hft/metrics.rs
backend_rust/src/modules/hft/mod.rs
backend_rust/src/modules/hft/session_manager.rs
backend_rust/src/modules/hft/types.rs
backend_rust/src/modules/hft/strategy_framework.rs        (NUEVO)
backend_rust/src/modules/hft/t5_strategy.rs               (reescrito)
backend_rust/src/modules/hft/t3_strategy.rs               (reescrito)
backend_rust/src/modules/hft/pnr_strategy.rs              (NUEVO)
backend_rust/src/modules/hft/insight_strategies.rs        (NUEVO)
backend_rust/src/modules/hft/fenix_trading.rs             (NUEVO)
backend_rust/Cargo.toml                                   (v0.2.5)
frontend_react/src/App.jsx
frontend_react/src/components/SystemHealth.jsx
frontend_react/src/components/WisdomCompare.jsx           (NUEVO)
.github/workflows/deploy.yml
```

### Próximos pasos

1. ~~Esperar sesión 600 (termina 15:15) → verificar archivo NO esté en 8KB~~ → sesión 603 confirmada: 2110 ticks, CSV vacío (0 filas de datos)
2. ~~Sesión 601 (15:15-15:30) → confirmar estabilidad~~ → mismo bug, CSV vacío
3. Revisar `GET /api/fenix` para ver PnL acumulado
4. Implementar estrategias adicionales que el usuario mencionó
5. Posible: módulo de análisis exhaustivo multi-estrategia

---

## Fix: CSV vacío (sesión 603) — 17:00 UTC

### Diagnóstico

Sesión 603 (`session_603_hft.csv`) con metadata correcta (2110 ticks, outcome=down) pero **cero filas de datos**. Causa raíz:

```
Parent 378 (indefinido) created → scheduler lo arranca como sesión normal
→ recording_sessions = [378]
→ Child 603 created → recording_sessions = [378, 603]
→ rec.session_id = first() = 378 → TODOS los ticks van a CSV de 378
→ CSV de 603 recibe header + metadata pero NUNCA datos
```

El parent (contenedor) NUNCA debería estar en `recording_sessions` porque no debe grabar datos.

### Archivos modificados

| Archivo | Cambio |
|---|---|
| `backend_rust/src/main.rs:210` | `first()` → `last()` (tick consumer, safety net) |
| `backend_rust/src/main.rs:484` | `first()` → `last()` (strategy on_tick) |
| `backend_rust/src/main.rs:656` | `first()` → `last()` (book/trade record + active_ids) |
| `backend_rust/src/modules/db/repository.rs:359` | `list_recording_session_ids` excluye parents (sesiones con hijos) |
| `backend_rust/src/modules/db/repository.rs:404` | `get_sessions_to_start` excluye parents |

### Query de filtro

```sql
WHERE status = 'recording'  -- o 'scheduled'
  AND (parent_id IS NOT NULL
       OR NOT EXISTS (SELECT 1 FROM recording_sessions WHERE parent_id = recording_sessions.id))
```

Excluye padres contenedores, incluye hijos y sesiones standalone.

### Comandos útiles en servidor

```bash
# Compilar y reiniciar
cd ~/zzignal-app && git pull && cd backend_rust && cargo build --release && sudo systemctl restart zzignal-app

# Ver logs
journalctl -u zzignal-app -f

# Ver archivos de sesión
ls -lh ~/zzignal-app/sessions/

# Ver últimas líneas de una sesión
tail -5 ~/zzignal-app/sessions/session_0601_hft.csv

# Contar líneas de datos
wc -l ~/zzignal-app/sessions/session_0601_hft.csv
```

---

## Fenix v3 — Range-based Trading con 50ms Edge — 20:00-21:00 UTC

### Commits

| Commit | Descripción |
|---|---|
| `fc422db` | Fix CSV vacío: parent 378 se colaba en recording_sessions |
| `83d4ac8` | Fix SQL alias: recording_sessions.id se resolvía contra tabla interna |
| `ab1d696` | Rate-limit gap alert a 1 cada 5s |
| `3c71196` | Fenix filters v1: trend gate, momentum direction, spread filter, fenixXX_skip |
| `dcfa4de` | Fenix volume filter: binance_vol gate, TPS fast entry, imbalance direction bias |
| `9e4efb2` | Spread gate: cambiado de absoluto >0.02 a relativo >5% (spread/mid) |
| `39506bb` | Fenix in_range usa bid/ask real + spread gate 200% |
| `35508ad` | Fenix in_range usa volumen real (bid/ask depth ≥10) + poly_mid |
| `bb688f4` | Fenix delta tracker: bid/ask volume deltas + velocity → UP/DOWN signal |
| `25e3abe` | Volume gate usa poly depth (bid/ask vol) en vez de binance |
| `833c781` | **Fenix v3**: range-based entry/exit con target, fenixXX_target + fenixXX_exit CSV |
| `21277b3` | Per-session balance ($20 start) + cumulative balance |
| `d198d6e` | Fix: FenixStats::new missing fields |
| `87991c2` | Fix: market_active acepta one-sided markets |

### Cómo funciona Fenix v3

Cada estrategia tiene un rango de precios [min, max]. La dirección se determina por:
1. **Delta signal** (delta bid/ask vol + velocity) — prioridad máxima
2. **Volume bias** (imbalance + velocity) — fallback
3. **Momentum** (last 8 poly_mid slope) — fallback final

**Entry**: precio en el 25% inferior del rango para UP, o 25% superior para DOWN. Confirmación con N ticks + volume trigger (TPS > 1.5 acelera).

**Exit**: precio alcanza el 95% del borde opuesto → take profit automático.

**50ms edge**: BTC se mueve en Binance → Fenix lo detecta 50ms antes que Polymarket reprecie → compra al precio viejo → vende al nuevo.

### Estrategias

| # | Estrategia | Rango | Conf Ticks | Entry UP @ | Target UP @ | Entry DOWN @ | Target DOWN @ |
|---|---|---|---|---|---|---|---|
| 1 | Fenix 35-65 | 0.35-0.65 | 1 | ≤0.42 | 0.62 | ≥0.57 | 0.37 |
| 2 | Fenix 30-50 | 0.30-0.50 | 3 | ≤0.35 | 0.47 | ≥0.45 | 0.32 |
| 3 | Fenix 45-55 | 0.45-0.55 | 5 | ≤0.47 | 0.52 | ≥0.53 | 0.47 |
| 4 | Fenix 40-50 | 0.40-0.50 | 7 | ≤0.42 | 0.47 | ≥0.48 | 0.42 |
| 5 | Fenix 45-50 | 0.45-0.50 | 10 | ≤0.46 | 0.48 | ≥0.49 | 0.47 |

### Nuevas columnas CSV (125 total)

| Columna | Descripción |
|---|---|
| `fenix_signal` | 0=none, 1=UP, 2=DOWN (delta bid/ask volume + velocity) |
| `fenixXX_target` | Precio objetivo de salida |
| `fenixXX_exit` | 1 = target alcanzado, 0 = abierto |

### API

```
GET /api/fenix → balance acumulativo + session_balance + session_pnl por estrategia
```
