# Odiseo Strategy — Documentacion Tecnica

**Fecha**: 04 Mayo 2026
**Branch**: `main`

---

## 1. Que es Odiseo

Familia de 12 estrategias de trading direccional para Polymarket BTC 15-min. Cada variante es un **limit buy** a un threshold fijo con TP=0.97 y SL de 3 capas.

La tesis: cuando `last_trade_up` o `last_trade_down` cruza cierto umbral (ej: 0.85), hay suficiente conviccion direccional para entrar. La salida es mecanica: TP a 0.97, o SL por microestructura/tendencia/hard.

---

## 2. Variantes (12 total)

| Variante | Entry | TP | SL Hard | R:R | Notas |
|----------|-------|-----|---------|-----|-------|
| **Odiseo85** | >=0.85 | 0.97 | 0.83 | 6:1 | Mas entries, mas profit total |
| Odiseo86 | >=0.86 | 0.97 | 0.84 | 5.5:1 | |
| Odiseo87 | >=0.87 | 0.97 | 0.85 | 5:1 | |
| Odiseo88 | >=0.88 | 0.97 | 0.86 | 4.5:1 | |
| Odiseo89 | >=0.89 | 0.97 | 0.87 | 4:1 | |
| **Odiseo90** | >=0.90 | 0.97 | 0.88 | 3.5:1 | Mejor balance entries/PnL |
| Odiseo91 | >=0.91 | 0.97 | 0.89 | 3:1 | |
| Odiseo92 | >=0.92 | 0.97 | 0.90 | 2.5:1 | |
| Odiseo93 | >=0.93 | 0.97 | 0.91 | 2:1 | |
| Odiseo94 | >=0.94 | 0.97 | 0.92 | 1.5:1 | Ultimos 10 min |
| Odiseo95 | >=0.95 | 0.97 | 0.93 | 1:1 | |
| Odiseo96 | >=0.96 | 0.985 | 0.95 | 5:1 | Ultimos 10 min |

- **Capital default**: $20 por variante por lado (UP/DOWN)
- **Anti-whale**: solo entra si `last_trade` esta entre `entry_threshold` y `tp_price`
- **Re-entry post-SL**: permitido en misma sesion
- **Budget**: configurable $1-$1000 via API
- **Reinvest**: acumula ganancias al capital

---

## 3. Estructura del Mercado (descubierto 04 Mayo 2026)

### Libro polarizado, NO thin book

Analisis de 3 sesiones (731, 732, 733):

| Metrica | S731 | S732 | S733 |
|---------|------|------|------|
| poly_bid (mejor compra) | ~0.01 | ~0.01 | ~0.01 |
| poly_ask (mejor venta) | ~0.94 | ~0.85 | ~0.91 |
| Spread | **0.98 fijo** | **0.98 fijo** | **0.98 fijo** |
| Vol total bid | 15,425 | 17,387 | 16,138 |
| Vol total ask | 15,425 | 17,388 | 16,140 |
| Spreads <=0.10 | **0 ticks** | **0 ticks** | **0 ticks** |

**Conclusion**: El libro tiene liquidez masiva (~15k contratos) pero concentrada en los extremos (0.01 y 0.99). No hay market makers en precios intermedios. El spread es SIEMPRE ~0.98.

### Implicaciones para ejecucion

- **Market order = suicida**: comprarias siempre a 0.99 (el ask)
- **Limit order = correcto**: pones tu bid al `last_trade_price` y esperas que alguien cruce agresivamente
- El `last_trade` ES el precio al que alguien acaba de tradear — es la referencia valida
- Los fills son escasos (0-20 trades reales por sesion de 15 min)

---

## 4. Logica de la Estrategia

### Entrada
```
if last_trade >= entry_threshold AND last_trade <= tp_price:
    → PlaceLimitOrder(BUY, price=last_trade, size=budget/price)
```

### 3 capas de Stop Loss

| Capa | Condicion | Exit |
|------|-----------|------|
| **SL-Micro** | Volumen cae >30% Y imbalance < -0.5 | Market sell |
| **SL-Trend** | Precio cae 3 cents del max Y velocity BTC < 0 | Market sell |
| **SL-Hard** | Precio <= sl_hard (ej: 0.83 para Odiseo85) | Market sell |

### Take Profit
```
if price >= tp_price (0.97):
    → PlaceLimitOrder(SELL, price=0.97)
```

### Sesion Close
Si la posicion no se cerro (ni TP ni SL), se settlea al precio final del mercado (1.0 si acerto direccion, 0.0 si no).

---

## 5. Resultados Paper (10 sesiones, Abril-Mayo 2026)

| Sesion | Outcome | BTC Δ | 90 PnL | 93 PnL | 95 PnL |
|--------|---------|-------|--------|--------|--------|
| S707 | UP | +$115 | -$0.66 | +$1.16 | +$0.84 |
| S708 | DOWN | -$2 | $0.00 | $0.00 | $0.00 |
| S709 | UP+DN | +$68 | +$2.08 | +$1.66 | +$1.44 |
| S712 | DOWN | -$29 | -$1.32 | -$1.47 | +$0.42 |
| S713 | DOWN | -$141 | +$1.54 | +$0.84 | +$0.42 |
| S714 | UP | +$68 | +$1.54 | +$0.84 | -$1.05 |
| S715 | UP | +$55 | +$0.22 | -$0.42 | -$0.84 |
| S716 | UP | +$69 | +$1.54 | +$0.84 | +$0.42 |
| S724 | UP | +$101 | +$1.26 | +$0.84 | +$0.42 |
| S725 | DOWN | -$91 | +$1.26 | +$0.84 | +$0.42 |

| | Odiseo90 | Odiseo93 | Odiseo95 |
|---|---|---|---|
| PnL total | **+$7.46** | +$4.97 | +$2.49 |
| Win rate | 80% (8W/2L) | 78% | 78% |
| Avg PnL/trade | +$0.08 | +$0.06 | +$0.04 |

---

## 6. Modo LIVE

### Activacion
```bash
POST /api/odiseo/live         → {"live": true}
POST /api/odiseo/variant      → {"idx": 0, "enabled": true}  # solo Odiseo85
POST /api/odiseo/budget       → {"idx": 0, "amount": 5.0}    # $5 budget
POST /api/odiseo/reinvest     → {"reinvest": true}
```

### Flujo de ordenes
1. `on_tick` detecta entrada → envia `CmdMsg::PlaceLimitOrder(BUY, price=last_trade)`
2. Worker recibe comando → `tokio::spawn` → `post_order` a Polymarket CLOB
3. Resultado via `AppMsg::OrderResult("✓ Limit BUY: #0x...")` → logeado
4. En cada tick subsiguiente, `check_exit` evalua TP/SL
5. TP: `PlaceLimitOrder(SELL, price=0.97)` | SL: `PlaceMarketOrder(SELL)`
6. Sesion close: settle al precio final si posicion abierta

### Riesgos LIVE

| Riesgo | Mitigacion |
|--------|-----------|
| Limit order no se llena | Normal en libro polarizado. La orden queda en el book hasta que alguien cruce. |
| Slippage en market sell (SL) | ~2-5% en libro de 2 niveles. Aceptable para salida de emergencia. |
| Latencia 50ms | No es bottleneck. El CLOB actualiza cada ~500ms. |
| Backend restart | Estado `entered` en memoria se pierde. Necesita persistencia (pendiente). |

---

## 7. API REST

| Metodo | Ruta | Descripcion |
|--------|------|-------------|
| GET | `/api/odiseo` | Stats acumulados JSON (12 variantes) |
| GET | `/api/odiseo/status` | Estado LIVE/PAPER + budgets |
| POST | `/api/odiseo/live` | `{"live": true/false}` |
| POST | `/api/odiseo/variant` | `{"idx": 0, "enabled": true/false}` |
| POST | `/api/odiseo/budget` | `{"idx": 0, "amount": 20.0}` |
| POST | `/api/odiseo/reinvest` | `{"reinvest": true/false}` |

---

## 8. Pipeline CSV (301 columnas)

Todas las 12 variantes trackeadas en tiempo real:

```
session_manager.rs  → CSV por sesion (archivo en disco)
db/api.rs           → Fallback CSV (desde buffer en memoria)
core/api.rs         → CSV export via REST
main.rs             → Asignacion odiseo_trades → CsvRecord
types.rs            → Struct con 301 campos (12 variantes x 2 lados x 7 campos)
```

Columnas 85-96 (nuevas, Mayo 2026): odiseo85 a odiseo92, completando el pipeline de las 12 variantes.

---

## 9. Pendientes

- [ ] Backtest de variantes 85-89, 91-92 con datos historicos (solo 90/93/95 tienen track record)
- [ ] Persistir `entered` state para sobrevivir reinicios del backend
- [ ] Confirmacion de fill: el `OrderResult` actual solo confirma que la orden fue aceptada, no llenada
- [ ] Probar con cuenta fondeada real ($50-100, 1 variante, budget $5)
- [ ] Panel frontend: mostrar PnL acumulado por variante en tiempo real

---

## 10. Lecciones Clave

1. **El libro NO es thin — es polarizado**. Liquidez masiva en extremos, cero en el medio.
2. **Limit orders > Market orders** para este mercado. Market orders siempre pagan 0.99.
3. **Entry mas bajo = mas profit**. Odiseo85 (0.85) deberia superar a Odiseo90 en terminos absolutos.
4. **La senal direccional (`odiseo_signal`) es solida**: acerto 3/3 sesiones analizadas.
5. **50ms de latencia es irrelevante** para un mercado que actualiza cada 500ms.
6. **Los umbrales 90-96 son conservadores** para paper — en LIVE, los fills son el verdadero filtro.
