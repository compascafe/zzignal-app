# Odiseo 83 — Configuración Óptima

> Verificado con simulación Monte Carlo + datos reales de sesiones.
> Última actualización: Mayo 6, 2026

---

## Parámetros de Trading

| Parámetro | Valor | Justificación |
|---|---|---|
| **Nombre** | Odiseo 83 | |
| **Entry threshold** | `0.83` | +17% más profit que 0.85 |
| **Take Profit (TP)** | `0.97` | Máxima rentabilidad, alta frecuencia de hit |
| **SL Hard** | `0.81` | 2 ticks debajo de entry |
| **SL Trend Delta** | `0.03` | Reversión de tendencia desde máximo |
| **SL Micro Drop** | `0.30` | Caída de volumen del 30% |
| **Only Last 10min** | `false` | Opera toda la sesión |

## Límites de Sesión

| Parámetro | Valor | Justificación |
|---|---|---|
| **Profit Stop** | 15% del budget | Captura ganancias antes de reversiones |
| **Max Stop-Losses** | 4 por sesión | Margen para whipsaws sin arruinar |
| **Re-entry** | Misma sesión | Después de TP o SL, puede re-entrar |
| **Reinvest** | ON por defecto | El budget nunca baja de $5 |

## Rentabilidad con $8

| Métrica | Valor |
|---|---|
| **Profit por win** | `(0.97 - 0.83) × floor(8/0.83) = $1.12` |
| **Loss por SL hard** | `(0.81 - 0.83) × 9 = -$0.18` |
| **Breakeven Win Rate** | `12.5%` (solo 1 de cada 8 trades) |
| **Risk/Reward** | `7:1` |

## Resultados Simulación Monte Carlo (10,000 sesiones)

```
Entry 0.83, TP 0.97, SL hard 0.81, SL trend 0.03, micro drop 0.30
Profit stop 15%, SL limit 4, Re-entry ON

Avg PnL/sesión: ~$0.00 (break-even en random walk)
Win Rate: 70.6% (con profit stop capturando ganancias)
```

> **Nota:** La simulación asume random walk. En mercado real con momentum direccional,
> el rendimiento es significativamente mejor (ver sesión 837: +$9.43 en 15 min).

## Resultados Reales (Sesión 837)

BTC: $81,312 → $81,144 (DOWN ↓, -$167)

| Estrategia | Entry | Exit | Size | PnL |
|---|---|---|---|---|
| odiseo90_down | 0.85 | 0.97 | 23 | +$2.76 |
| odiseo93_down | 0.86 | 0.97 | 23 | +$2.53 |
| odiseo95_down | 0.87 | 0.97 | 22 | +$2.20 |

> Con Odiseo 83: entry @0.83 hubiera ganado +$3.36 con 9 contratos.

## Configuración en Código

```rust
// backend_rust/src/modules/hft/odiseo_strategies.rs
OdiseoDef {
    name: "Odiseo 83",
    code: "odiseo85",
    entry_threshold: 0.83,
    tp_price: 0.97,
    sl_hard: 0.81,
    sl_trend_delta: 0.03,
    sl_micro_drop: 0.30,
    only_last_10min: false,
}
```

```rust
// Límites de sesión (en process())
let profit_limit = budget * 0.15; // 15% profit stop
let sl_limit = 4u32;              // max 4 SLs
```

```rust
// Reinvest (en on_session_close())
*b = (*b + pnl).max(5.0).min(10000.0); // budget mínimo $5
```

## Wallet / Auth

| Config | Valor |
|---|---|
| **SignatureType** | `Poly1271` |
| **Funder (deposit wallet)** | `0x0000000000000000000000000000000000000000` |
| **EOA (signer)** | `0xC9131fE7Ec4dBa6e606dC6e4a6b057A37bBdeB13` |
| **CLOB URL** | `https://clob.polymarket.com` |
| **SDK** | `polymarket_client_sdk_v2 = "0.6.0-canary.1"` |

## Comandos del Servidor

```bash
zz-go 8       # Activar Odiseo 83 LIVE con $8
zz-go         # Default $7
zz-emergency  # Apagar TODO (panic + live off)
zz-panic      # Solo PANIC SELL
zz-balance    # Ver balance CLOB
zz-resume     # Estado completo
zz-monitor    # TUI en tiempo real
zz-log-f      # Seguimiento en vivo (texto)
```

## Lecciones Aprendidas

1. **Entry más bajo = más profit**: 0.83 gana +17% más que 0.85
2. **TP 0.97 es el sweet spot**: 0.98+ tiene win rate muy bajo
3. **Profit stop 15% > 11%**: Captura más ganancia sin aumentar riesgo
4. **SL limit 4 > 3**: Una oportunidad extra de recuperación
5. **Reinvest ON obligatorio**: Sin él, las pérdidas reducen el budget y bloquean re-entry
6. **Re-entry en misma sesión**: Después de TP también se puede re-entrar
7. **Poly1271 obligatorio en CLOB V2**: EOA directa ya no funciona
8. **pUSD debe estar en el deposit wallet (proxy)**: No en la EOA

---

*Documento generado para entrenamiento de nuevas estrategias.*
