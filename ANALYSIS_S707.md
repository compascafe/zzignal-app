# Análisis Session 707 — Primera sesión con Odiseo v4 funcional

**2026-05-04 17:45–18:00 UTC | BTC 15-min | Outcome: UP**

## Métricas base

| Métrica | Valor |
|---------|-------|
| BTC start | $80,275 |
| BTC end | $80,391 |
| BTC delta | +$115 (+0.14%) |
| BTC max intra-sesión | $80,625 (+$350) |
| Outcome | UP |
| Ticks totales | 2,092 |
| Trades reales | 0 (los precios vienen de CLOB `last_trade_price`) |
| Columnas CSV | 175 (incluye `last_trade_up`, `last_trade_down`) |

## Evolución del last_trade (precio real de mercado)

El `last_trade_up` captura el precio real al que se cruzan órdenes en Polymarket, tick a tick.
NO es el orderbook (best_bid/ask clavados en 0.01/0.99) — es el precio de mercado real.

```
17:45  UP=0.80  DN=0.20  BTC=80,266   (tranquilo)
17:49  UP=0.90  DN=0.12  BTC=80,439   ← Odiseo90 UP ENTRA
       ▸ vel BTC: +19.4 USD/s (subidón)
       ▸ 11 segundos después: vel -14.6, UP cae a 0.87
       ▸ Odiseo90 UP sale por SL-trend: -$0.66
       
17:50  UP=0.93  DN=0.09  BTC=80,471   ← Odiseo93 UP ENTRA
       ▸ vel: +24 USD/s (extremo)
       
17:51  UP=0.99  DN=0.01  BTC=80,594   ¡UP a 99¢!
       ▸ BTC subió +$328 en 6 minutos
       ▸ Mercado convencido: UP es seguro

17:54  UP=0.99  DN=0.006 BTC=80,625   (máximo absoluto de BTC)
       ▸ 15 segundos después...
       ▸ BTC se desploma: vel -21.5 USD/s
       ▸ UP cae de 0.99 a 0.92 en 13 SEGUNDOS
       ▸ Pánico: "¿UP va a perder?"
       
17:55  UP=0.91→0.99  BTC recupera
       ▸ vel +36.7, luego -20.1 — látigo total

17:56  UP=0.99  DN=0.01  BTC=80,493   (estabiliza)
17:59  UP=0.999 DN=0.001  BTC=80,461  (cierre)
```

## Resultados Odiseo v4

| Estrategia | Entry | Size | PnL | Balance | Qué pasó |
|-----------|-------|------|------|---------|----------|
| Odiseo 90 UP | 0.90 | 22 | -$0.66 | $19.34 | SL-trend sacó en la primera corrección |
| Odiseo 93 UP | 0.93 | 21 | +$1.16 | $21.15 | Aguantó todo y ganó (+5.8%) |
| Odiseo 95 UP | — | — | — | $20.00 | Sin entrada (precio < 0.95 hasta 17:50) |
| Odiseo 90 DN | — | — | — | $20.00 | DOWN nunca cruzó 0.90 |
| Odiseo 93 DN | — | — | — | $20.00 | DOWN nunca cruzó 0.93 |
| Odiseo 95 DN | — | — | — | $20.00 | DOWN nunca cruzó 0.95 |

## Observaciones clave

1. **El `last_trade` SÍ se mueve.** De 0.80 a 0.999 en 15 minutos, reflejando fielmente cada latigazo de BTC.

2. **Polymarket reacciona a BTC velocity en milisegundos.** Cada pico de `price_velocity` (+24, -21, +36) coincide con un movimiento brusco en el `last_trade_up`.

3. **La protección anti-whale funciona.** El limit-buy a 0.985 bloqueó entradas cuando el precio saltaba por encima del TP. Odiseo 90 y 93 entraron correctamente dentro del rango [entry_threshold, tp_price].

4. **El SL-trend es rápido.** Odiseo 90 UP fue sacado en 11 segundos cuando BTC corrigió -$36 y UP cayó 3 centavos. Protegió capital (-$0.66 vs potencial -$2+ si seguía cayendo).

5. **BTC dominó la narrativa.** La sesión fue una montaña rusa de BTC: +$350 en 6 min, luego crash -$170, luego recuperación. Polymarket simplemente reflejó BTC.

6. **Primera sesión con entradas reales de Odiseo.** Después de 6 sesiones con 0 entradas (usando orderbook), cambiar a `last_trade` desbloqueó el sistema.
