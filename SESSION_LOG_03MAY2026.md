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

1. Esperar sesión 600 (termina 15:15) → verificar archivo NO esté en 8KB
2. Sesión 601 (15:15-15:30) → confirmar estabilidad
3. Revisar `GET /api/fenix` para ver PnL acumulado
4. Implementar estrategias adicionales que el usuario mencionó
5. Posible: módulo de análisis exhaustivo multi-estrategia

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
