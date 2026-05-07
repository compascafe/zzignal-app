# REFACTOR — Estado actual

**Rama:** `REFACTOR`  
**Último commit:** `ae87e83` — remove groups E+F+G (Trades + Imba + Liqb)  
**Columnas:** 304 → **58** (-246)

## Progreso

| Commit | Qué se eliminó | Columnas |
|---|---|---|
| `a43c9a1` | Unificación serialización CSV | 304 |
| `e2cc0da` | Cerbero (9) + Fenix (49) + Hydra T5/T3 (6) | 249 |
| `59ae1b8` | Bollinger (10) + Adaptive Risk (11) | 228 |
| `2a994a8` | Odiseo variantes 86-96 (154) | 74 |
| `ae87e83` | Trades (4) + Imba (6) + Liqb (6) | **58** |

## Columnas actuales (58)

```
 1  ts_local                     ✅ KEEP
 2  ts_exchange                  🗑️  DELETE
 3  event_type                   ✅ KEEP
 4  latencia_ms                  🗑️  DELETE
 5  binance_price                🗑️  DELETE
 6  binance_micro_price          🗑️  DELETE
 7  binance_imbalance            🗑️  DELETE
 8  binance_vol_100ms            🗑️  DELETE
 9  binance_vol_24h              🗑️  DELETE
10  poly_bid                     ✅ KEEP
11  poly_ask                     ✅ KEEP
12  poly_mid                     ✅ KEEP
13  poly_spread                  ✅ KEEP
14  poly_bid_vol_all             ✅ KEEP
15  poly_ask_vol_all             ✅ KEEP
16  poly_imbalance               ✅ KEEP
17  trades_per_second            🗑️  DELETE
18  price_velocity               ✅ KEEP
19  poly_liquidity_delta         🗑️  DELETE
20  absorption_ratio             🗑️  DELETE
21  price_gap_ratio              🗑️  DELETE
22  spoofing_flag                ✅ KEEP
23  tape_speed_flag              ✅ KEEP
24  gap_alert_flag               ✅ KEEP
25  pnr_active                   🗑️  DELETE
26  pnr_seconds_left             ✅ KEEP
27  pnr_price                    🗑️  DELETE
28  pnr_return_up                🗑️  DELETE
29  pnr_return_down              🗑️  DELETE
30  pnr_volatility_1m            🗑️  DELETE
31  pnr_confidence               🗑️  DELETE
32  pnr_trend                    🗑️  DELETE
33  pnr_spread_pct               🗑️  DELETE
34  pressure_bid_floor           ✅ KEEP
35  pressure_ask_ceiling         ✅ KEEP
36  pressure_band                ✅ KEEP
37  pressure_index               ✅ KEEP
38  pressure_skew                ✅ KEEP
39  odiseo83_up_active           ✅ KEEP
40  odiseo83_up_entry_price      ✅ KEEP
41  odiseo83_up_size             ✅ KEEP
42  odiseo83_up_pnl              ✅ KEEP
43  odiseo83_up_exit_price       ✅ KEEP
44  odiseo83_up_exit_reason      ✅ KEEP
45  odiseo83_up_balance          ✅ KEEP
46  odiseo83_down_active         ✅ KEEP
47  odiseo83_down_entry_price    ✅ KEEP
48  odiseo83_down_size           ✅ KEEP
49  odiseo83_down_pnl            ✅ KEEP
50  odiseo83_down_exit_price     ✅ KEEP
51  odiseo83_down_exit_reason    ✅ KEEP
52  odiseo83_down_balance        ✅ KEEP
53  odiseo83_up_live_pnl         ✅ KEEP
54  odiseo83_down_live_pnl       ✅ KEEP
55  live_usdc_balance            ✅ KEEP
56  odiseo_signal                ✅ KEEP
57  last_trade_up                ✅ KEEP
58  last_trade_down              ✅ KEEP
```

## Pendiente para mañana

- [ ] Eliminar 19 columnas restantes (marcadas 🗑️)
- [ ] Limpiar managers huérfanos (t3_manager, adaptive_engine)
- [ ] Verificar CSV funciona con monitor TUI
- [ ] Merge a main cuando esté validado
