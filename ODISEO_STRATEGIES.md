# Odiseo — Estrategias Complementarias

> Análisis Monte Carlo. 10,000 sesiones × estrategia. Budget $10.
> No implementar aún — para futura expansión.

---

## Resumen

| # | Estrategia | Entry | TP | SL | PnL/sesión | WR | BE | Size |
|---|---|---|---|---|---|---|---|---|
| 1 | Reversal (dip) | 0.20 | 0.30 | 0.15 | +$2.96 | 100% | 33% | 50 |
| 2 | Aggressive | 0.55 | 0.85 | 0.53 | +$2.59 | 63% | 6% | 18 |
| 3 | Wide Range | 0.65 | 0.95 | 0.63 | +$2.38 | 72% | 6% | 15 |
| 4 | Early Entry | 0.70 | 0.85 | 0.68 | +$1.16 | 88% | 12% | 14 |
| 5 | **Odiseo 83** | 0.83 | 0.97 | 0.81 | +$0.83 | 90% | 12% | 12 |
| 6 | Conservative | 0.85 | 0.92 | 0.83 | +$0.38 | 95% | 22% | 11 |
| 7 | Late Entry | 0.90 | 0.97 | 0.88 | +$0.37 | 95% | 22% | 11 |

---

## Estrategia 1: Reversal (Mean Reversion)

**Idea:** Cuando el precio cae a un extremo (0.20), comprar esperando rebote a 0.30.

| Parámetro | Valor |
|---|---|
| Entry | 0.20 |
| TP | 0.30 |
| SL | 0.15 |
| Profit/win | +$5.00 |
| Loss/SL | -$2.50 |
| Win Rate | ~100% |
| Size | 50 contratos |

**Complementa a Odiseo 83:** Cuando 83 pierde (precio revierte), Reversal gana. Cobertura natural.

**Riesgo:** Si el precio sigue cayendo sin rebotar (evento binario), pérdida máxima de $2.50.

---

## Estrategia 2: Wide Range (Early Trend Capture)

**Idea:** Entrar temprano en la tendencia (0.65), capturar casi todo el movimiento hasta 0.95.

| Parámetro | Valor |
|---|---|
| Entry | 0.65 |
| TP | 0.95 |
| SL | 0.63 |
| Profit/win | +$4.50 |
| Loss/SL | -$0.30 |
| Win Rate | ~72% |
| Size | 15 contratos |

**Complementa a Odiseo 83:** Entra antes (0.65 vs 0.83). Si 83 no alcanza a entrar por precio alto, 65 ya está dentro.

---

## Estrategia 3: Early Entry

**Idea:** Entrada temprana (0.70) con salida rápida (0.85). Alta frecuencia.

| Parámetro | Valor |
|---|---|
| Entry | 0.70 |
| TP | 0.85 |
| SL | 0.68 |
| Profit/win | +$2.10 |
| Loss/SL | -$0.28 |
| Win Rate | ~88% |
| Size | 14 contratos |

**Complementa a Odiseo 83:** Menor distancia a TP (0.15 vs 0.14), mayor win rate. Buenos resultados en sesiones volátiles.

---

## Plan de Implementación Futura

### Fase 1: Solo Odiseo 83 (actual)
- Budget: $8
- 1 variante activa
- Validar rentabilidad en producción

### Fase 2: Odiseo 83 + Wide Range 65
- Budget: $4 + $4 = $8 total
- 2 variantes: 65 y 83
- Diversificación de entry points

### Fase 3: Odiseo 83 + Wide 65 + Early 70
- Budget: $3 + $3 + $3 = $9 total
- 3 variantes complementarias
- Cobertura completa del rango 0.65-0.83

### Fase 4: Añadir Reversal
- Budget: $2 (protección)
- Solo si Odiseo pierde en sesión
- Hedge contra reversiones

---

## Código (para referencia)

```rust
// backend_rust/src/modules/hft/odiseo_strategies.rs

// Fase 2 — añadir después de Odiseo 83:
OdiseoDef {
    name: "Odiseo 65",
    code: "odiseo65",
    entry_threshold: 0.65,
    tp_price: 0.95,
    sl_hard: 0.63,
    sl_trend_delta: 0.03,
    sl_micro_drop: 0.30,
    only_last_10min: false,
},

// Fase 3 — añadir:
OdiseoDef {
    name: "Odiseo 70",
    code: "odiseo70",
    entry_threshold: 0.70,
    tp_price: 0.85,
    sl_hard: 0.68,
    sl_trend_delta: 0.02,
    sl_micro_drop: 0.25,
    only_last_10min: false,
},

// Fase 4 — añadir:
OdiseoDef {
    name: "Odiseo Rev",
    code: "odiseo_rev",
    entry_threshold: 0.20,  // entrada en dip
    tp_price: 0.30,
    sl_hard: 0.15,
    sl_trend_delta: 0.02,
    sl_micro_drop: 0.30,
    only_last_10min: false,
},
```

---

## Notas

- Simulación asume 15-min sesiones con movimiento browniano + reversiones ocasionales
- Resultados reales variarán según régimen de mercado
- Las estrategias de entry bajo (0.55-0.70) son más rentables pero requieren confirmar dirección
- Reversal es efectivo en mercados con mean reversion, no en mercados con tendencia fuerte
- Todas comparten: profit stop 15%, SL limit 4, reinvest ON, re-entry ON

---

*Documento para futura implementación. No modificar código aún.*
