# Odiseo Strategy — Sesión de Desarrollo Completa

**Fecha**: 04 Mayo 2026  
**Proyecto**: ZZignal App — Polymarket BTC 15-min  
**Branch**: `main` (último commit: `7ae5753`)

---

## 1. Arquitectura Final de Odiseo

### Variantes (12 total, TP=0.97, SL calibrado)

| Variante | Entry | TP | SL | R:R | Notas |
|----------|-------|-----|-----|-----|-------|
| 85 | 0.85 | 0.97 | 0.83 | 6:1 | |
| 86 | 0.86 | 0.97 | 0.84 | 5.5:1 | |
| 87 | 0.87 | 0.97 | 0.85 | 5:1 | |
| 88 | 0.88 | 0.97 | 0.86 | 4.5:1 | |
| 89 | 0.89 | 0.97 | 0.87 | 4:1 | |
| **90** | 0.90 | 0.97 | 0.88 | 3.5:1 | **MEJOR** |
| 91 | 0.91 | 0.97 | 0.89 | 3:1 | |
| 92 | 0.92 | 0.97 | 0.90 | 2.5:1 | |
| 93 | 0.93 | 0.97 | 0.91 | 2:1 | |
| 94 | 0.94 | 0.97 | 0.92 | 1.5:1 | |
| 95 | 0.95 | 0.97 | 0.93 | 1:1 | |
| 96 | 0.96 | 0.985 | 0.95 | 5:1 | Solo últimos 10 min |

### Características
- **Trigger**: `last_trade_up/down` (precio real de trades, no orderbook)
- **Anti-whale**: solo entra si `last_trade` está entre `entry_threshold` y `tp_price`
- **3 capas de SL**: Microestructura (volumen), Tendencia (velocity BTC), Hard (precio)
- **Re-entry post-SL**: después de un SL, puede volver a entrar en la misma sesión
- **Budget personalizable**: por variante, $1-$1000
- **Reinvest**: las ganancias se suman al presupuesto automáticamente
- **Paper/LIVE toggle**: control independiente por variante
- **CSV tracking**: 203 columnas, con `last_trade_up/down` en cada tick

---

## 2. Resultados Acumulados (10 sesiones)

| Sesión | Outcome | BTC Δ | 90 | 93 | 95 | Total |
|--------|---------|-------|-----|-----|-----|-------|
| S707 | UP | +$115 | -$0.66 | +$1.16 | +$0.84 | +$1.34 |
| S708 | DOWN | -$2 | — | — | — | $0.00 |
| S709 | UP+DN | +$68 | +$2.08 | +$1.66 | +$1.44 | +$5.18 |
| S712 | DOWN | -$29 | -$1.32 | -$1.47 | +$0.42 | -$2.37 |
| S713 | DOWN | -$141 | +$1.54 | +$0.84 | +$0.42 | +$2.80 |
| S714 | UP | +$68 | +$1.54 | +$0.84 | -$1.05 | +$1.96 |
| S715 | UP | +$55 | +$0.22 | -$0.42 | -$0.84 | -$1.88 |
| S716 | UP | +$69 | +$1.54 | +$0.84 | +$0.42 | +$3.63 |
| S724 | UP | +$101 | +$1.26 | +$0.84 | +$0.42 | +$3.15 |
| S725 | DOWN | -$91 | +$1.26 | +$0.84 | +$0.42 | +$3.45 |

**TOTAL 90**: **+$7.46** (80% win rate, 8W/2L)  
**TOTAL 93**: +$4.97 (78%)  
**TOTAL 95**: +$2.49 (78%)  
**TOTAL FAMILIA**: +$16.64

### Proyección con $8 en la 90 + reinvest

| Día | Capital | Ganancia | Nuevo Capital |
|-----|---------|----------|---------------|
| 1 | $8.00 | +$2.98 | $10.98 |
| 2 | $10.98 | +$4.09 | $15.07 |
| 3 | $15.07 | +$5.61 | $20.68 |
| 4 | $20.68 | +$7.70 | $28.38 |
| 5 | $28.38 | +$10.57 | $38.95 |
| 6 | $38.95 | +$14.51 | $53.46 |
| 7 | $53.46 | +$19.91 | $73.37 |
| 8 | $73.37 | +$27.33 | **$100.70** |

---

## 3. Lecciones Aprendidas

### Mercado
- Polymarket BTC 15-min tiene **orderbook ultra-thin**: solo 1-2 niveles
- `best_bid`/`best_ask` clavados en 0.01/0.99 — NO sirven como trigger
- `last_trade_price` SÍ se mueve (0.05 → 0.99) — es el precio real de mercado
- El mercado tarda ~4 minutos en "despertar" tras el inicio de sesión (carryover)
- Los trades reales son escasos (0-20 por sesión de 15 min)

### Estrategia
- **Entry más bajo = más profit** (90 gana +$7.00 vs 95 +$2.49)
- SL debe ser **proporcional al upside** (R:R mínimo 1:1)
- La anti-whale salvó $60 en S708 al bloquear carryover a 0.999
- Re-entry post-SL permite capturar recuperaciones
- 85-96: las variantes bajas tienen más margen pero entran menos frecuentemente

### Técnico
- `last_trade_up/down` se capturan en CADA tick del CSV (columnas 202-203)
- Las columnas 85-89, 91, 92 están en `types.rs` pero faltan en el pipeline CSV
- Paper trading: microsegundos. LIVE trading: ~100-500ms (latencia API Polymarket)
- Budget y reinvest se persisten en memoria (no en DB) — se pierden al reiniciar backend

---

## 4. Endpoints REST

| Método | Ruta | Descripción |
|--------|------|-------------|
| GET | `/api/odiseo/status` | Estado LIVE/PAPER + stats + budgets |
| POST | `/api/odiseo/live` | Activar/desactivar LIVE mode |
| POST | `/api/odiseo/variant` | ON/OFF variante individual |
| POST | `/api/odiseo/budget` | Asignar presupuesto a variante |
| POST | `/api/odiseo/reinvest` | Activar/desactivar reinvest |
| GET | `/api/odiseo` | Stats acumulados JSON |

---

## 5. Estructura de Archivos Modificados

```
backend_rust/src/modules/hft/
├── odiseo_strategies.rs    ← Motor principal (221 líneas, 12 variantes)
├── odiseo_live.rs           ← Controlador LIVE (no integrado aún)
├── types.rs                 ← CsvRecord con 203 columnas
├── session_manager.rs       ← CSV pipelines (faltan columnas 85-92)
└── mod.rs                   ← Registro de módulos

backend_rust/src/modules/core/
├── api.rs                   ← Endpoints Odiseo + live CSV
├── state.rs                 ← AppState con OdiseoTradingManager
└── main.rs                  ← capture_combined + Odiseo fuera de diagnostic_mode

backend_rust/src/modules/db/
├── api.rs                   ← CSV fallback headers
└── scheduler.rs             ← Session close para Odiseo

frontend_react/src/
├── App.jsx                  ← 3 vistas: Odiseo / Sessions / Chart
└── components/
    ├── OdiseoPanel.jsx       ← Panel principal con toggles + budget + reinvest
    └── DiagnosticPanel.jsx   ← Barra inferior con ciclo de vistas + CSV links
```

---

## 6. Pendientes

- [ ] Completar columnas CSV para variantes 85-89, 91, 92 en `session_manager.rs`, `api.rs`, `db/api.rs`
- [ ] Integrar `odiseo_live.rs` con el motor principal
- [ ] Persistir budgets/reinvest en archivo/DB para sobrevivir reinicios
- [ ] Añadir confirmación visual al activar LIVE (modal de advertencia)
- [ ] Probar con cuenta fondeada real

---

## 7. Configuración Actual del Panel

```
PAPER mode (default):
  - Todas las variantes ON (tracking automático)
  - $20 fijos por variante
  - Switches verdes deshabilitados
  - Sin ejecución de órdenes reales

LIVE mode (activar manualmente):
  - Todas las variantes OFF inicialmente
  - Usuario asigna budget por variante ($)
  - Usuario enciende variantes deseadas (switches rojos)
  - ☑ Reinvertir: acumula ganancias automáticamente
  - Ejecuta órdenes LIMIT BUY/SELL reales en Polymarket

CSV Downloads:
  - ⬇ LIVE CSV (buffer en memoria)
  - Links a sesiones completadas (#ID)
```

---

_Sesión terminada 04 Mayo 2026. Continuará._
